use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    fmt,
    rc::Rc,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use lenso::{ActivateContext, DeactivateContext, Lifecycle, Port};
use lenso_auth_sdk::{AuthOutcome, CredentialEvidence, authenticate_request, decode_auth_response};
use lenso_capability_agent_tool_provider::{self as tools, ExecuteRequest};
use lenso_capability_auth as auth;
use lenso_capability_http_endpoint::{
    EndpointHandleInvocationError, ExtractorFuture, ExtractorRejection, FromRequest, HandleRequest,
    prelude::*,
    response::{self, Problem, StatusCode},
};
use lenso_capability_secrets as secrets;
use lenso_capability_secrets::{ResolveRequest, SecretsInvocationError};
use lenso_kernel::{InvocationContext, RuntimeFailure};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use uuid::Uuid;
use zeroize::Zeroizing;

const DATABASE_URL_SECRET: &str = "knowledge/database-url";
const DEFAULT_EXCERPT_LIMIT: i64 = 96;
const MAX_ATTACHMENT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CreateNote {
    title: String,
    body: String,
}

#[derive(Debug, Deserialize)]
struct NotePath {
    note_id: String,
}

#[derive(Debug, Deserialize)]
struct JobPath {
    job_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Note {
    id: String,
    title: String,
    body: String,
    excerpt: String,
    job_id: String,
    processing_status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EnqueueExcerptRequest<'a> {
    excerpt_limit: i64,
    note_id: &'a str,
    owner_id: &'a str,
    text: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnqueueExcerptResponse {
    excerpt: String,
    job_id: String,
    status: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaimExcerptResponse {
    excerpt: String,
    job_id: String,
    lease_token: String,
    note_id: String,
    owner_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompleteExcerptRequest<'a> {
    job_id: &'a str,
    lease_token: &'a str,
}

#[derive(Debug, Deserialize)]
struct CompleteExcerptResponse {
    completed: bool,
}

#[derive(Debug, Serialize)]
struct ProcessExcerptResult {
    processed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InspectExcerptRequest<'a> {
    job_id: &'a str,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct InspectExcerptResponse {
    attempts: u64,
    job_id: String,
    status: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct BusinessSettings {
    excerpt_limit: i64,
    revision: i64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UpdateBusinessSettings {
    excerpt_limit: i64,
    predecessor_revision: i64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UploadAttachment {
    content_base64: String,
    filename: String,
    media_type: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Attachment {
    id: String,
    filename: String,
    media_type: String,
    note_id: String,
    size: usize,
}

#[derive(Debug)]
struct AuthenticatedUser(String);

#[lenso::plugin(lifecycle)]
#[derive(Clone)]
pub struct KnowledgeBase {
    excerpt: Port<tools::ToolProviderClient>,
    auth: Port<auth::AuthClient>,
    secrets: Port<secrets::SecretsClient>,
    next_id: Rc<Cell<u64>>,
    notes: Rc<RefCell<BTreeMap<String, (String, Note)>>>,
    database: Rc<RefCell<Option<PgPool>>>,
}

impl fmt::Debug for KnowledgeBase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KnowledgeBase")
            .field("database_ready", &self.database.borrow().is_some())
            .finish_non_exhaustive()
    }
}

impl Lifecycle for KnowledgeBase {
    async fn activate(&self, context: ActivateContext) -> Result<(), RuntimeFailure> {
        let invocation = context
            .dependencies()
            .invocation_context_after(Duration::from_secs(10), context.cancellation())?;
        let database_url = self
            .secrets
            .resolve_with_context(
                invocation,
                ResolveRequest {
                    reference: DATABASE_URL_SECRET.to_owned(),
                },
            )
            .await
            .map_err(|error| match error {
                SecretsInvocationError::Domain(_) => RuntimeFailure::PluginFailure {
                    detail: "Knowledge database secret was rejected".to_owned(),
                },
                SecretsInvocationError::Runtime(error) => error,
            })?;
        let database_url = Zeroizing::new(database_url.value);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&database_url)
            .await
            .map_err(|_| RuntimeFailure::PluginFailure {
                detail: "Knowledge database connection failed".to_owned(),
            })?;
        sqlx::query("SELECT revision FROM knowledge_reference.settings LIMIT 1")
            .execute(&pool)
            .await
            .map_err(|_| RuntimeFailure::PluginFailure {
                detail: "Knowledge database schema is not prepared".to_owned(),
            })?;
        self.database.replace(Some(pool));
        Ok(())
    }

    async fn deactivate(&self, _context: DeactivateContext) -> Result<(), RuntimeFailure> {
        let database = self.database.borrow_mut().take();
        if let Some(database) = database {
            database.close().await;
        }
        Ok(())
    }
}

impl FromRequest<KnowledgeBase> for AuthenticatedUser {
    fn from_request<'a>(
        provider: &'a KnowledgeBase,
        context: &'a mut InvocationContext,
        request: &'a HandleRequest,
    ) -> ExtractorFuture<'a, Self> {
        Box::pin(async move {
            let evidence = request
                .credential
                .as_ref()
                .map(|credential| CredentialEvidence::new(&credential.scheme, &credential.value));
            let response = provider
                .auth
                .authenticate_with_context(context.clone(), authenticate_request(evidence))
                .await
                .map_err(|error| -> ExtractorRejection {
                    match error {
                        auth::AuthInvocationError::Domain(_) => authentication_problem().into(),
                        auth::AuthInvocationError::Runtime(error) => {
                            EndpointHandleInvocationError::Runtime(error).into()
                        }
                    }
                })?;
            let outcome = decode_auth_response(response).map_err(|_| {
                EndpointHandleInvocationError::Runtime(RuntimeFailure::ProtocolViolation {
                    capability: auth::CAPABILITY_ID,
                })
            })?;
            let AuthOutcome::Authenticated(assertion) = outcome else {
                return Err(authentication_problem().into());
            };
            if assertion.actor_kind() != "user" {
                return Err(response::problem(
                    StatusCode::FORBIDDEN,
                    "unsupported_actor",
                    "The knowledge workspace requires a user actor.",
                )
                .into());
            }
            let subject = assertion.subject().to_owned();
            *context = assertion.attach(context.clone()).map_err(|error| {
                EndpointHandleInvocationError::Runtime(RuntimeFailure::Internal {
                    detail: format!("could not attach authenticated assertion: {error}"),
                })
            })?;
            Ok(Self(subject))
        })
    }
}

#[endpoint]
#[allow(unknown_lints, clippy::unused_async, clippy::unused_async_trait_impl)]
impl KnowledgeBase {
    #[get("knowledge-base.home", "/")]
    async fn home(&self) -> Result<HandleResponse, Problem> {
        let mut response = response::text(StatusCode::OK, include_str!("../public/index.html"));
        response.headers[0].value = "text/html; charset=utf-8".into();
        Ok(response)
    }

    #[get("knowledge-base.assets.js", "/assets/app.js")]
    async fn javascript(&self) -> Result<HandleResponse, Problem> {
        let mut response = response::text(StatusCode::OK, include_str!("../public/assets/app.js"));
        response.headers[0].value = "text/javascript; charset=utf-8".into();
        Ok(response)
    }

    #[get("knowledge-base.assets.css", "/assets/index.css")]
    async fn stylesheet(&self) -> Result<HandleResponse, Problem> {
        let mut response =
            response::text(StatusCode::OK, include_str!("../public/assets/index.css"));
        response.headers[0].value = "text/css; charset=utf-8".into();
        Ok(response)
    }

    #[post("knowledge-base.notes.create", "/notes")]
    async fn create(
        &self,
        user: AuthenticatedUser,
        Json(input): Json<CreateNote>,
    ) -> Result<(StatusCode, Json<Note>), Problem> {
        let (title, body) = validated_note(&input)?;
        let settings = self.settings_for(&user.0).await?;
        let note_id = if self.database().is_some() {
            format!("note-{}", Uuid::now_v7())
        } else {
            let sequence = self.next_id.get() + 1;
            self.next_id.set(sequence);
            format!("note-{sequence}")
        };
        let queued = self
            .execute_tool::<_, EnqueueExcerptResponse>(
                "knowledge.enqueue-excerpt",
                &EnqueueExcerptRequest {
                    excerpt_limit: settings.excerpt_limit,
                    note_id: &note_id,
                    owner_id: &user.0,
                    text: body,
                },
            )
            .await?;
        if !matches!(queued.status.as_str(), "queued" | "succeeded") {
            return Err(bad_gateway("excerpt processor returned an invalid status"));
        }
        let note = Note {
            id: note_id,
            title: title.to_owned(),
            body: body.to_owned(),
            excerpt: queued.excerpt,
            job_id: queued.job_id,
            processing_status: queued.status,
        };
        self.store_note(&user.0, &note, settings.revision).await?;
        Ok((StatusCode::CREATED, Json(note)))
    }

    #[get("knowledge-base.notes.read", "/notes/{note_id}")]
    async fn read(
        &self,
        user: AuthenticatedUser,
        Path(path): Path<NotePath>,
    ) -> Result<Json<Note>, Problem> {
        self.load_note(&user.0, &path.note_id).await.map(Json)
    }

    #[post("knowledge-base.jobs.process-next", "/jobs/process-next")]
    async fn process_next(
        &self,
        _user: AuthenticatedUser,
    ) -> Result<Json<ProcessExcerptResult>, Problem> {
        let claimed = self
            .execute_tool::<_, ClaimExcerptResponse>(
                "knowledge.claim-excerpt",
                &serde_json::json!({}),
            )
            .await?;
        self.persist_excerpt(
            &claimed.owner_id,
            &claimed.note_id,
            &claimed.job_id,
            &claimed.excerpt,
        )
        .await?;
        let completed = self
            .execute_tool::<_, CompleteExcerptResponse>(
                "knowledge.complete-excerpt",
                &CompleteExcerptRequest {
                    job_id: &claimed.job_id,
                    lease_token: &claimed.lease_token,
                },
            )
            .await?;
        if !completed.completed {
            return Err(bad_gateway("excerpt job lease was not completed"));
        }
        Ok(Json(ProcessExcerptResult { processed: true }))
    }

    #[get("knowledge-base.jobs.inspect", "/job-status/{job_id}")]
    async fn inspect_job(
        &self,
        user: AuthenticatedUser,
        Path(path): Path<JobPath>,
    ) -> Result<Json<InspectExcerptResponse>, Problem> {
        self.ensure_job_owner(&user.0, &path.job_id).await?;
        self.execute_tool::<_, InspectExcerptResponse>(
            "knowledge.inspect-excerpt",
            &InspectExcerptRequest {
                job_id: &path.job_id,
            },
        )
        .await
        .map(Json)
    }

    #[get("knowledge-base.settings.read", "/settings")]
    async fn read_settings(
        &self,
        user: AuthenticatedUser,
    ) -> Result<Json<BusinessSettings>, Problem> {
        self.settings_for(&user.0).await.map(Json)
    }

    #[put("knowledge-base.settings.update", "/settings")]
    async fn update_settings(
        &self,
        user: AuthenticatedUser,
        Json(input): Json<UpdateBusinessSettings>,
    ) -> Result<Json<BusinessSettings>, Problem> {
        if !(16..=512).contains(&input.excerpt_limit) {
            return Err(Problem::new(
                StatusCode::BAD_REQUEST,
                "invalid_excerpt_limit",
                "excerpt_limit must be from 16 through 512",
            ));
        }
        let Some(database) = self.database() else {
            return Err(Problem::new(
                StatusCode::CONFLICT,
                "dynamic_configuration_unavailable",
                "dynamic settings require the PostgreSQL-backed application mode",
            ));
        };
        self.settings_for(&user.0).await?;
        let row = sqlx::query(
            "UPDATE knowledge_reference.settings SET revision = revision + 1, excerpt_limit = $1 \
             WHERE owner_id = $2 AND revision = $3 RETURNING revision, excerpt_limit",
        )
        .bind(input.excerpt_limit)
        .bind(&user.0)
        .bind(input.predecessor_revision)
        .fetch_optional(&database)
        .await
        .map_err(database_error)?
        .ok_or_else(|| {
            Problem::new(
                StatusCode::CONFLICT,
                "stale_settings_revision",
                "the settings predecessor revision is stale",
            )
        })?;
        Ok(Json(BusinessSettings {
            excerpt_limit: row.get("excerpt_limit"),
            revision: row.get("revision"),
        }))
    }

    #[post("knowledge-base.attachments.upload", "/note-attachments/{note_id}")]
    async fn upload_attachment(
        &self,
        user: AuthenticatedUser,
        Path(path): Path<NotePath>,
        Json(input): Json<UploadAttachment>,
    ) -> Result<(StatusCode, Json<Attachment>), Problem> {
        self.load_note(&user.0, &path.note_id).await?;
        let Some(database) = self.database() else {
            return Err(Problem::new(
                StatusCode::CONFLICT,
                "persistent_storage_unavailable",
                "file upload requires the PostgreSQL-backed application mode",
            ));
        };
        if input.filename.trim().is_empty()
            || input.filename.len() > 255
            || input.media_type.trim().is_empty()
            || input.media_type.len() > 127
        {
            return Err(Problem::new(
                StatusCode::BAD_REQUEST,
                "invalid_attachment",
                "filename and media_type must be bounded non-empty strings",
            ));
        }
        let content = STANDARD.decode(&input.content_base64).map_err(|_| {
            Problem::new(
                StatusCode::BAD_REQUEST,
                "invalid_attachment",
                "content_base64 is not valid Base64",
            )
        })?;
        if content.is_empty() || content.len() > MAX_ATTACHMENT_BYTES {
            return Err(Problem::new(
                StatusCode::BAD_REQUEST,
                "invalid_attachment",
                "attachment size must be from 1 byte through 1 MiB",
            ));
        }
        let attachment = Attachment {
            id: format!("attachment-{}", Uuid::now_v7()),
            filename: input.filename,
            media_type: input.media_type,
            note_id: path.note_id,
            size: content.len(),
        };
        sqlx::query(
            "INSERT INTO knowledge_reference.attachments \
             (owner_id, attachment_id, note_id, filename, media_type, content) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&user.0)
        .bind(&attachment.id)
        .bind(&attachment.note_id)
        .bind(&attachment.filename)
        .bind(&attachment.media_type)
        .bind(&content)
        .execute(&database)
        .await
        .map_err(database_error)?;
        Ok((StatusCode::CREATED, Json(attachment)))
    }
}

impl KnowledgeBase {
    fn database(&self) -> Option<PgPool> {
        self.database.borrow().clone()
    }

    async fn settings_for(&self, owner_id: &str) -> Result<BusinessSettings, Problem> {
        let Some(database) = self.database() else {
            return Ok(BusinessSettings {
                excerpt_limit: DEFAULT_EXCERPT_LIMIT,
                revision: 1,
            });
        };
        sqlx::query(
            "INSERT INTO knowledge_reference.settings (owner_id, revision, excerpt_limit) \
             VALUES ($1, 1, $2) ON CONFLICT (owner_id) DO NOTHING",
        )
        .bind(owner_id)
        .bind(DEFAULT_EXCERPT_LIMIT)
        .execute(&database)
        .await
        .map_err(database_error)?;
        let row = sqlx::query(
            "SELECT revision, excerpt_limit FROM knowledge_reference.settings WHERE owner_id = $1",
        )
        .bind(owner_id)
        .fetch_one(&database)
        .await
        .map_err(database_error)?;
        Ok(BusinessSettings {
            excerpt_limit: row.get("excerpt_limit"),
            revision: row.get("revision"),
        })
    }

    async fn store_note(
        &self,
        owner_id: &str,
        note: &Note,
        config_revision: i64,
    ) -> Result<(), Problem> {
        if let Some(database) = self.database() {
            sqlx::query(
                "INSERT INTO knowledge_reference.notes \
                 (owner_id, note_id, title, body, excerpt, job_id, processing_status, config_revision) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(owner_id)
            .bind(&note.id)
            .bind(&note.title)
            .bind(&note.body)
            .bind(&note.excerpt)
            .bind(&note.job_id)
            .bind(&note.processing_status)
            .bind(config_revision)
            .execute(&database)
            .await
            .map_err(database_error)?;
        } else {
            self.notes
                .borrow_mut()
                .insert(note.id.clone(), (owner_id.to_owned(), note.clone()));
        }
        Ok(())
    }

    async fn load_note(&self, owner_id: &str, note_id: &str) -> Result<Note, Problem> {
        if let Some(database) = self.database() {
            let row = sqlx::query(
                "SELECT note_id, title, body, excerpt, job_id, processing_status \
                 FROM knowledge_reference.notes WHERE owner_id = $1 AND note_id = $2",
            )
            .bind(owner_id)
            .bind(note_id)
            .fetch_optional(&database)
            .await
            .map_err(database_error)?
            .ok_or_else(note_not_found)?;
            return Ok(note_from_row(&row));
        }
        self.notes
            .borrow()
            .get(note_id)
            .filter(|(owner, _)| owner == owner_id)
            .map(|(_, note)| note.clone())
            .ok_or_else(note_not_found)
    }

    async fn persist_excerpt(
        &self,
        owner_id: &str,
        note_id: &str,
        job_id: &str,
        excerpt: &str,
    ) -> Result<(), Problem> {
        if let Some(database) = self.database() {
            let result = sqlx::query(
                "UPDATE knowledge_reference.notes SET excerpt = $1, processing_status = 'succeeded' \
                 WHERE owner_id = $2 AND note_id = $3 AND job_id = $4",
            )
            .bind(excerpt)
            .bind(owner_id)
            .bind(note_id)
            .bind(job_id)
            .execute(&database)
            .await
            .map_err(database_error)?;
            if result.rows_affected() != 1 {
                return Err(Problem::new(
                    StatusCode::CONFLICT,
                    "orphaned_excerpt_job",
                    "the claimed excerpt job does not match a note",
                ));
            }
            return Ok(());
        }
        let mut notes = self.notes.borrow_mut();
        let (owner, note) = notes.get_mut(note_id).ok_or_else(note_not_found)?;
        if owner != owner_id || note.job_id != job_id {
            return Err(Problem::new(
                StatusCode::CONFLICT,
                "excerpt_job_mismatch",
                "the claimed excerpt job identity does not match the note",
            ));
        }
        note.excerpt = excerpt.to_owned();
        note.processing_status = "succeeded".to_owned();
        Ok(())
    }

    async fn ensure_job_owner(&self, owner_id: &str, job_id: &str) -> Result<(), Problem> {
        if let Some(database) = self.database() {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM knowledge_reference.notes WHERE owner_id = $1 AND job_id = $2)",
            )
            .bind(owner_id)
            .bind(job_id)
            .fetch_one(&database)
            .await
            .map_err(database_error)?;
            if !exists {
                return Err(note_not_found());
            }
            return Ok(());
        }
        if self
            .notes
            .borrow()
            .values()
            .any(|(owner, note)| owner == owner_id && note.job_id == job_id)
        {
            Ok(())
        } else {
            Err(note_not_found())
        }
    }

    async fn execute_tool<Input, Output>(
        &self,
        name: &str,
        input: &Input,
    ) -> Result<Output, Problem>
    where
        Input: Serialize,
        Output: for<'de> Deserialize<'de>,
    {
        let arguments = serde_json::to_string(input).map_err(|error| {
            Problem::new(StatusCode::BAD_REQUEST, "invalid_note", error.to_string())
        })?;
        let response = self
            .excerpt
            .execute(ExecuteRequest {
                name: name.to_owned(),
                arguments_json: arguments.try_into().map_err(|error| {
                    Problem::new(
                        StatusCode::BAD_REQUEST,
                        "invalid_note",
                        format!("{error:?}"),
                    )
                })?,
            })
            .await
            .map_err(|error| bad_gateway(&format!("{error:?}")))?;
        serde_json::from_str(&response.content)
            .map_err(|error| bad_gateway(&format!("invalid Tool response: {error}")))
    }
}

fn note_from_row(row: &sqlx::postgres::PgRow) -> Note {
    Note {
        id: row.get("note_id"),
        title: row.get("title"),
        body: row.get("body"),
        excerpt: row.get("excerpt"),
        job_id: row.get("job_id"),
        processing_status: row.get("processing_status"),
    }
}

fn authentication_problem() -> HandleResponse {
    response::problem(
        StatusCode::UNAUTHORIZED,
        "authentication_required",
        "Provide a valid Bearer credential.",
    )
}

fn database_error(_error: sqlx::Error) -> Problem {
    Problem::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "knowledge_storage_unavailable",
        "knowledge storage is temporarily unavailable",
    )
}

fn bad_gateway(detail: &str) -> Problem {
    Problem::new(StatusCode::BAD_GATEWAY, "excerpt_failed", detail)
}

fn note_not_found() -> Problem {
    Problem::new(
        StatusCode::NOT_FOUND,
        "note_not_found",
        "the note does not exist",
    )
}

fn validated_note(input: &CreateNote) -> Result<(&str, &str), Problem> {
    let title = input.title.trim();
    let body = input.body.trim();
    if title.is_empty() || body.is_empty() {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_note",
            "title and body must not be empty",
        ));
    }
    Ok((title, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_valid_notes_and_rejects_empty_fields_before_invocation() {
        let valid = CreateNote {
            title: " First note ".to_owned(),
            body: " Created through the public App path. ".to_owned(),
        };
        assert_eq!(
            validated_note(&valid).unwrap(),
            ("First note", "Created through the public App path.")
        );

        for invalid in [
            CreateNote {
                title: " ".to_owned(),
                body: "content".to_owned(),
            },
            CreateNote {
                title: "title".to_owned(),
                body: "\n".to_owned(),
            },
        ] {
            assert!(validated_note(&invalid).is_err());
        }
    }
}
