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
    JsonSchema,
    prelude::*,
    response::{self, Problem, StatusCode},
};
use lenso_capability_secrets as secrets;
use lenso_capability_secrets::{ResolveRequest, SecretsInvocationError};
use lenso_kernel::{CancellationToken, InvocationContext, PluginDependencies, RuntimeFailure};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use uuid::Uuid;
use zeroize::Zeroizing;

pub mod settings_core;
pub mod settings_store;

use settings_core::{
    BusinessSettings, DEFAULT_EXCERPT_LIMIT, SettingsCommandError, SettingsFailure,
    UpdateBusinessSettings, prepare_settings_command, problem_spec,
};
use settings_store::{SettingsStoreOutcome, compare_and_set_settings, get_or_create_settings};

const DATABASE_URL_SECRET: &str = "knowledge/database-url";
const MAX_ATTACHMENT_BYTES: usize = 1024 * 1024;
const BACKGROUND_IDLE_POLL: Duration = Duration::from_millis(500);
const BACKGROUND_RETRY: Duration = Duration::from_secs(2);
const BACKGROUND_TOOL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentPolicy {
    #[serde(deserialize_with = "bounded_attachment_limit")]
    pub max_attachment_bytes: u32,
}

pub fn attachment_policy_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "max_attachment_bytes": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAX_ATTACHMENT_BYTES
            }
        },
        "required": ["max_attachment_bytes"]
    })
}

fn bounded_attachment_limit<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let limit = u32::deserialize(deserializer)?;
    if (1..=MAX_ATTACHMENT_BYTES as u32).contains(&limit) {
        Ok(limit)
    } else {
        Err(D::Error::custom(
            "max_attachment_bytes must be from 1 through 1048576",
        ))
    }
}

pub struct PinnedAttachmentPolicy {
    pub revision: u64,
    pub value: AttachmentPolicy,
}

#[derive(Clone, Copy, Debug)]
pub struct AttachmentPolicyUnavailable;

pub trait AttachmentPolicySource {
    fn capture(&self) -> Result<PinnedAttachmentPolicy, AttachmentPolicyUnavailable>;
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct CreateNote {
    title: String,
    body: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct NotePath {
    note_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct JobPath {
    job_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
struct Note {
    id: String,
    title: String,
    body: String,
    excerpt: String,
    #[schemars(required)]
    job_id: Option<String>,
    processing_status: NoteProcessingStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum NoteProcessingStatus {
    DispatchPending,
    Queued,
    Succeeded,
    Failed,
}

impl NoteProcessingStatus {
    fn from_wire(value: &str) -> Option<Self> {
        match value {
            "dispatch_pending" => Some(Self::DispatchPending),
            "queued" => Some(Self::Queued),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::DispatchPending => "dispatch_pending",
            Self::Queued => "queued",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EnqueueExcerptRequest<'a> {
    available_at: &'a str,
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
#[serde(rename_all = "camelCase")]
struct InspectExcerptRequest<'a> {
    job_id: &'a str,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
struct InspectExcerptResponse {
    attempts: u64,
    job_id: String,
    status: String,
}

struct PendingNote {
    owner_id: String,
    note_id: String,
    job_id: String,
}

struct DispatchNote {
    available_at: String,
    body: String,
    excerpt_limit: i64,
    note_id: String,
    owner_id: String,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct UploadAttachment {
    content_base64: String,
    filename: String,
    media_type: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
struct Attachment {
    id: String,
    filename: String,
    media_type: String,
    note_id: String,
    size: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_revision: Option<String>,
}

#[derive(Debug)]
struct AuthenticatedUser(String);

#[derive(Debug)]
struct IdempotencyKey(Option<String>);

#[lenso::plugin(lifecycle)]
#[derive(Clone)]
pub struct KnowledgeBase {
    excerpt: Port<tools::ToolProviderClient>,
    auth: Port<auth::AuthClient>,
    secrets: Port<secrets::SecretsClient>,
    next_id: Rc<Cell<u64>>,
    notes: Rc<RefCell<BTreeMap<String, (String, Note)>>>,
    database: Rc<RefCell<Option<PgPool>>>,
    dispatch_context: Rc<RefCell<Option<(PluginDependencies, CancellationToken)>>>,
    attachment_policy: Option<Rc<dyn AttachmentPolicySource>>,
}

impl fmt::Debug for KnowledgeBase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KnowledgeBase")
            .field("database_ready", &self.database.borrow().is_some())
            .field("attachment_policy_bound", &self.attachment_policy.is_some())
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
        let worker = self.clone();
        let dependencies = context.dependencies().clone();
        let ready = context.ready_gate();
        let cancellation = context.cancellation();
        self.dispatch_context
            .replace(Some((dependencies.clone(), cancellation.clone())));
        context
            .tasks()
            .spawn_local(Box::pin(async move {
                tokio::select! {
                    _ = ready.wait() => {}
                    _ = cancellation.cancelled() => return,
                }
                loop {
                    let result = tokio::select! {
                        _ = cancellation.cancelled() => return,
                        result = worker.process_pending_note(&dependencies, cancellation.clone()) => result,
                    };
                    let delay = match result {
                        Ok(true) => Duration::ZERO,
                        Ok(false) => BACKGROUND_IDLE_POLL,
                        Err(_) => {
                            eprintln!("Knowledge background processing failed; retrying");
                            BACKGROUND_RETRY
                        }
                    };
                    tokio::select! {
                        _ = cancellation.cancelled() => return,
                        _ = tokio::time::sleep(delay) => {}
                    }
                }
            }))
            .map_err(|error| RuntimeFailure::PluginFailure {
                detail: format!("Knowledge background worker could not start: {error:?}"),
            })?;
        Ok(())
    }

    async fn deactivate(&self, _context: DeactivateContext) -> Result<(), RuntimeFailure> {
        self.dispatch_context.replace(None);
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

impl FromRequest<KnowledgeBase> for IdempotencyKey {
    fn from_request<'a>(
        _provider: &'a KnowledgeBase,
        _context: &'a mut InvocationContext,
        request: &'a HandleRequest,
    ) -> ExtractorFuture<'a, Self> {
        Box::pin(async move {
            let mut matching = request
                .headers
                .iter()
                .filter(|header| header.name.eq_ignore_ascii_case("idempotency-key"));
            let key = matching.next().map(|header| header.value.clone());
            if matching.next().is_some() {
                return Err(response::problem(
                    StatusCode::BAD_REQUEST,
                    "invalid_idempotency_key",
                    "Provide at most one Idempotency-Key header.",
                )
                .into());
            }
            Ok(Self(key))
        })
    }
}

#[endpoint]
#[allow(unknown_lints, clippy::unused_async, clippy::unused_async_trait_impl)]
impl KnowledgeBase {
    pub fn bind_attachment_policy(&mut self, source: Rc<dyn AttachmentPolicySource>) {
        self.attachment_policy = Some(source);
    }

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
    #[openapi(r##"{
        "security": [{"bearerAuth": []}],
        "requestBody": {
            "required": true,
            "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CreateNote"}}}
        },
        "responses": {
            "201": {"description": "Created note", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Note"}}}},
            "202": {"description": "Saved note awaiting background dispatch", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Note"}}}},
            "400": {"description": "Invalid note", "content": {"application/problem+json": {"schema": {"$ref": "#/components/schemas/Problem"}}}}
        }
    }"##)]
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
        let mut note = Note {
            id: note_id.clone(),
            title: title.to_owned(),
            body: body.to_owned(),
            excerpt: String::new(),
            job_id: None,
            processing_status: NoteProcessingStatus::DispatchPending,
        };
        let available_at = if self.database().is_some() {
            self.store_pending_note(&user.0, &note, &settings).await?
        } else {
            "1970-01-01T00:00:00Z".to_owned()
        };
        let request = EnqueueExcerptRequest {
            available_at: &available_at,
            excerpt_limit: settings.excerpt_limit,
            note_id: &note_id,
            owner_id: &user.0,
            text: body,
        };
        let dispatched = if self.database().is_some() {
            let context = self.dispatch_context.borrow().clone();
            if let Some((dependencies, cancellation)) = context {
                self.execute_background_tool::<_, EnqueueExcerptResponse>(
                    &dependencies,
                    cancellation,
                    "knowledge.enqueue-excerpt",
                    &request,
                )
                .await
            } else {
                Err(bad_gateway("durable dispatch is unavailable"))
            }
        } else {
            self.execute_tool::<_, EnqueueExcerptResponse>("knowledge.enqueue-excerpt", &request)
                .await
        };
        if self.database().is_some() {
            if let Ok(dispatched) = dispatched {
                if self
                    .record_dispatch(&user.0, &note_id, &dispatched)
                    .await
                    .is_ok()
                {
                    if let Ok(current) = self.load_note(&user.0, &note_id).await {
                        return Ok((StatusCode::CREATED, Json(current)));
                    }
                }
            }
            return Ok((StatusCode::ACCEPTED, Json(note)));
        }
        let dispatched = dispatched?;
        if dispatched.status != "succeeded" {
            return Err(bad_gateway("inline excerpt did not complete"));
        }
        note.excerpt = dispatched.excerpt;
        note.job_id = Some(dispatched.job_id);
        note.processing_status = NoteProcessingStatus::Succeeded;
        self.store_note(&user.0, &note, settings.revision).await?;
        Ok((StatusCode::CREATED, Json(note)))
    }

    #[get("knowledge-base.notes.read", "/notes/{note_id}")]
    #[openapi(r##"{
        "security": [{"bearerAuth": []}],
        "parameters": [{"name": "note_id", "in": "path", "required": true, "schema": {"type": "string"}}],
        "responses": {
            "200": {"description": "Note", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Note"}}}},
            "400": {"description": "Invalid path", "content": {"application/problem+json": {"schema": {"$ref": "#/components/schemas/Problem"}}}},
            "404": {"description": "Missing note", "content": {"application/problem+json": {"schema": {"$ref": "#/components/schemas/Problem"}}}}
        }
    }"##)]
    async fn read(
        &self,
        user: AuthenticatedUser,
        Path(path): Path<NotePath>,
    ) -> Result<Json<Note>, Problem> {
        self.load_note(&user.0, &path.note_id).await.map(Json)
    }

    #[get("knowledge-base.jobs.inspect", "/job-status/{job_id}")]
    #[openapi(r##"{
        "security": [{"bearerAuth": []}],
        "parameters": [{"name": "job_id", "in": "path", "required": true, "schema": {"type": "string"}}],
        "responses": {
            "200": {"description": "Durable job state", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/JobState"}}}},
            "502": {"description": "Inspection failed", "content": {"application/problem+json": {"schema": {"$ref": "#/components/schemas/Problem"}}}}
        }
    }"##)]
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
    #[openapi(r##"{
        "security": [{"bearerAuth": []}],
        "responses": {
            "200": {"description": "Current request-captured settings", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/BusinessSettings"}}}}
        }
    }"##)]
    async fn read_settings(
        &self,
        user: AuthenticatedUser,
    ) -> Result<Json<BusinessSettings>, Problem> {
        self.settings_for(&user.0).await.map(Json)
    }

    #[put("knowledge-base.settings.update", "/settings")]
    #[openapi(r##"{
        "security": [{"bearerAuth": []}],
        "requestBody": {
            "required": true,
            "content": {"application/json": {"schema": {"$ref": "#/components/schemas/UpdateBusinessSettings"}}}
        },
        "responses": {
            "200": {"description": "Updated settings", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/BusinessSettings"}}}},
            "409": {"description": "Stale predecessor", "content": {"application/problem+json": {"schema": {"$ref": "#/components/schemas/Problem"}}}}
        }
    }"##)]
    async fn update_settings(
        &self,
        user: AuthenticatedUser,
        idempotency_key: IdempotencyKey,
        Json(input): Json<UpdateBusinessSettings>,
    ) -> Result<Json<BusinessSettings>, Problem> {
        let command = prepare_settings_command(input, idempotency_key.0.as_deref())
            .map_err(settings_command_problem)?;
        let Some(database) = self.database() else {
            return Err(Problem::new(
                StatusCode::CONFLICT,
                "dynamic_configuration_unavailable",
                "dynamic settings require the PostgreSQL-backed application mode",
            ));
        };
        match compare_and_set_settings(&database, &user.0, &command)
            .await
            .map_err(database_error)?
        {
            SettingsStoreOutcome::Applied(settings) => Ok(Json(settings)),
            SettingsStoreOutcome::StaleRevision => {
                Err(settings_failure_problem(SettingsFailure::StaleRevision))
            }
            SettingsStoreOutcome::IdempotencyConflict => Err(settings_failure_problem(
                SettingsFailure::IdempotencyConflict,
            )),
        }
    }

    #[post("knowledge-base.attachments.upload", "/note-attachments/{note_id}")]
    #[openapi(r##"{
        "security": [{"bearerAuth": []}],
        "parameters": [{"name": "note_id", "in": "path", "required": true, "schema": {"type": "string"}}],
        "requestBody": {
            "required": true,
            "content": {"application/json": {"schema": {"$ref": "#/components/schemas/UploadAttachment"}}}
        },
        "responses": {
            "201": {"description": "Stored attachment", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Attachment"}}}},
            "404": {"description": "Missing or inaccessible note", "content": {"application/problem+json": {"schema": {"$ref": "#/components/schemas/Problem"}}}}
        }
    }"##)]
    async fn upload_attachment(
        &self,
        user: AuthenticatedUser,
        Path(path): Path<NotePath>,
        Json(input): Json<UploadAttachment>,
    ) -> Result<(StatusCode, Json<Attachment>), Problem> {
        self.load_note(&user.0, &path.note_id).await?;
        let policy = capture_attachment_policy(self.attachment_policy.as_ref())?;
        let Some(database) = self.database() else {
            return Err(Problem::new(
                StatusCode::CONFLICT,
                "persistent_storage_unavailable",
                "file upload requires the PostgreSQL-backed application mode",
            ));
        };
        let (attachment, content) = prepare_attachment(path.note_id, input, policy)?;
        store_attachment(&database, &user.0, &attachment, &content).await?;
        Ok((StatusCode::CREATED, Json(attachment)))
    }
}

impl KnowledgeBase {
    fn database(&self) -> Option<PgPool> {
        self.database.borrow().clone()
    }

    async fn process_pending_note(
        &self,
        dependencies: &PluginDependencies,
        cancellation: CancellationToken,
    ) -> Result<bool, Problem> {
        let Some(database) = self.database() else {
            return Ok(false);
        };
        let undispatched = sqlx::query(
            "SELECT owner_id, note_id, body, excerpt_limit, available_at \
             FROM knowledge_reference.notes \
             WHERE processing_status = 'dispatch_pending' \
               AND next_dispatch_at <= transaction_timestamp() \
             ORDER BY next_dispatch_at, created_at, note_id LIMIT 1",
        )
        .fetch_optional(&database)
        .await
        .map_err(database_error)?;
        if let Some(row) = undispatched {
            let dispatch = DispatchNote {
                owner_id: row.try_get("owner_id").map_err(database_error)?,
                note_id: row.try_get("note_id").map_err(database_error)?,
                body: row.try_get("body").map_err(database_error)?,
                excerpt_limit: row.try_get("excerpt_limit").map_err(database_error)?,
                available_at: row.try_get("available_at").map_err(database_error)?,
            };
            let queued = self
                .execute_background_tool::<_, EnqueueExcerptResponse>(
                    dependencies,
                    cancellation.clone(),
                    "knowledge.enqueue-excerpt",
                    &EnqueueExcerptRequest {
                        available_at: &dispatch.available_at,
                        excerpt_limit: dispatch.excerpt_limit,
                        note_id: &dispatch.note_id,
                        owner_id: &dispatch.owner_id,
                        text: &dispatch.body,
                    },
                )
                .await;
            if let Ok(queued) = queued {
                if self
                    .record_dispatch(&dispatch.owner_id, &dispatch.note_id, &queued)
                    .await
                    .is_ok()
                {
                    return Ok(true);
                }
            }
            self.defer_dispatch(&dispatch.owner_id, &dispatch.note_id)
                .await?;
        }
        let row = sqlx::query(
            "SELECT owner_id, note_id, job_id FROM knowledge_reference.notes \
             WHERE processing_status = 'queued' ORDER BY created_at, note_id LIMIT 1",
        )
        .fetch_optional(&database)
        .await
        .map_err(database_error)?;
        let Some(row) = row else {
            return Ok(false);
        };
        let pending = PendingNote {
            owner_id: row.try_get("owner_id").map_err(database_error)?,
            note_id: row.try_get("note_id").map_err(database_error)?,
            job_id: row.try_get("job_id").map_err(database_error)?,
        };
        let state = self
            .execute_background_tool::<_, InspectExcerptResponse>(
                dependencies,
                cancellation.clone(),
                "knowledge.inspect-excerpt",
                &InspectExcerptRequest {
                    job_id: &pending.job_id,
                },
            )
            .await?;
        if state.job_id != pending.job_id {
            return Err(bad_gateway(
                "excerpt job identity changed during inspection",
            ));
        }
        match state.status.as_str() {
            "succeeded" => {
                self.finish_note(
                    &pending.owner_id,
                    &pending.note_id,
                    &pending.job_id,
                    "succeeded",
                )
                .await?;
                return Ok(true);
            }
            "failed" => {
                self.finish_note(
                    &pending.owner_id,
                    &pending.note_id,
                    &pending.job_id,
                    "failed",
                )
                .await?;
                return Ok(true);
            }
            "queued" | "running" => {}
            _ => return Err(bad_gateway("excerpt job returned an invalid status")),
        }
        let claimed = self
            .execute_background_tool::<_, ClaimExcerptResponse>(
                dependencies,
                cancellation.clone(),
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
            .execute_background_tool::<_, CompleteExcerptResponse>(
                dependencies,
                cancellation,
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
        self.finish_note(
            &claimed.owner_id,
            &claimed.note_id,
            &claimed.job_id,
            "succeeded",
        )
        .await?;
        Ok(true)
    }

    async fn defer_dispatch(&self, owner_id: &str, note_id: &str) -> Result<(), Problem> {
        let database = self
            .database()
            .ok_or_else(|| bad_gateway("durable dispatch is unavailable"))?;
        sqlx::query(
            "UPDATE knowledge_reference.notes \
             SET dispatch_attempts = dispatch_attempts + 1, \
                 next_dispatch_at = transaction_timestamp() + \
                   (LEAST(30, 2 * (LEAST(dispatch_attempts, 14) + 1)) * interval '1 second') \
             WHERE owner_id = $1 AND note_id = $2 \
               AND processing_status = 'dispatch_pending'",
        )
        .bind(owner_id)
        .bind(note_id)
        .execute(&database)
        .await
        .map_err(database_error)?;
        eprintln!("Knowledge dispatch deferred; retrying another pending note");
        Ok(())
    }

    async fn finish_note(
        &self,
        owner_id: &str,
        note_id: &str,
        job_id: &str,
        status: &str,
    ) -> Result<(), Problem> {
        if let Some(database) = self.database() {
            let result = sqlx::query(
                "UPDATE knowledge_reference.notes SET processing_status = $1 \
                 WHERE owner_id = $2 AND note_id = $3 AND job_id = $4 \
                 AND processing_status = 'queued'",
            )
            .bind(status)
            .bind(owner_id)
            .bind(note_id)
            .bind(job_id)
            .execute(&database)
            .await
            .map_err(database_error)?;
            if result.rows_affected() != 1 {
                return Err(bad_gateway("excerpt note changed before job completion"));
            }
            return Ok(());
        }
        let mut notes = self.notes.borrow_mut();
        let (owner, note) = notes.get_mut(note_id).ok_or_else(note_not_found)?;
        if owner != owner_id
            || note.job_id.as_deref() != Some(job_id)
            || note.processing_status != NoteProcessingStatus::Queued
        {
            return Err(bad_gateway("excerpt note changed before job completion"));
        }
        note.processing_status = NoteProcessingStatus::from_wire(status)
            .ok_or_else(|| bad_gateway("invalid completed note status"))?;
        Ok(())
    }

    async fn settings_for(&self, owner_id: &str) -> Result<BusinessSettings, Problem> {
        let Some(database) = self.database() else {
            return Ok(BusinessSettings {
                excerpt_limit: DEFAULT_EXCERPT_LIMIT,
                revision: 1,
            });
        };
        get_or_create_settings(&database, owner_id)
            .await
            .map_err(database_error)
    }

    async fn store_note(
        &self,
        owner_id: &str,
        note: &Note,
        _config_revision: i64,
    ) -> Result<(), Problem> {
        self.notes
            .borrow_mut()
            .insert(note.id.clone(), (owner_id.to_owned(), note.clone()));
        Ok(())
    }

    async fn store_pending_note(
        &self,
        owner_id: &str,
        note: &Note,
        settings: &BusinessSettings,
    ) -> Result<String, Problem> {
        let database = self
            .database()
            .ok_or_else(|| bad_gateway("durable note registration requires a database"))?;
        sqlx::query_scalar(
            "INSERT INTO knowledge_reference.notes \
             (owner_id, note_id, title, body, excerpt, job_id, processing_status, \
              config_revision, excerpt_limit, available_at) \
             VALUES ($1, $2, $3, $4, '', NULL, 'dispatch_pending', $5, $6, \
              to_char(transaction_timestamp() AT TIME ZONE 'UTC', \
                'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')) \
             RETURNING available_at",
        )
        .bind(owner_id)
        .bind(&note.id)
        .bind(&note.title)
        .bind(&note.body)
        .bind(settings.revision)
        .bind(settings.excerpt_limit)
        .fetch_one(&database)
        .await
        .map_err(database_error)
    }

    async fn record_dispatch(
        &self,
        owner_id: &str,
        note_id: &str,
        dispatched: &EnqueueExcerptResponse,
    ) -> Result<(), Problem> {
        let status = NoteProcessingStatus::from_wire(&dispatched.status)
            .filter(|status| {
                matches!(
                    status,
                    NoteProcessingStatus::Queued | NoteProcessingStatus::Succeeded
                )
            })
            .ok_or_else(|| bad_gateway("excerpt processor returned an invalid status"))?;
        let database = self
            .database()
            .ok_or_else(|| bad_gateway("durable note registration requires a database"))?;
        let result = sqlx::query(
            "UPDATE knowledge_reference.notes \
             SET job_id = $1, excerpt = $2, processing_status = $3 \
             WHERE owner_id = $4 AND note_id = $5 \
               AND job_id IS NULL AND processing_status = 'dispatch_pending'",
        )
        .bind(&dispatched.job_id)
        .bind(&dispatched.excerpt)
        .bind(status.as_str())
        .bind(owner_id)
        .bind(note_id)
        .execute(&database)
        .await
        .map_err(database_error)?;
        if result.rows_affected() == 1 {
            return Ok(());
        }
        let current = self.load_note(owner_id, note_id).await?;
        if current.job_id.as_deref() == Some(dispatched.job_id.as_str())
            && matches!(
                current.processing_status,
                NoteProcessingStatus::Queued
                    | NoteProcessingStatus::Succeeded
                    | NoteProcessingStatus::Failed
            )
        {
            return Ok(());
        }
        Err(bad_gateway("excerpt job identity changed during dispatch"))
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
                "UPDATE knowledge_reference.notes \
                 SET excerpt = $1, job_id = $4, processing_status = 'queued' \
                 WHERE owner_id = $2 AND note_id = $3 \
                   AND (job_id = $4 OR (job_id IS NULL AND processing_status = 'dispatch_pending')) \
                   AND processing_status IN ('dispatch_pending', 'queued')",
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
        if owner != owner_id || note.job_id.as_deref() != Some(job_id) {
            return Err(Problem::new(
                StatusCode::CONFLICT,
                "excerpt_job_mismatch",
                "the claimed excerpt job identity does not match the note",
            ));
        }
        note.excerpt = excerpt.to_owned();
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
            .any(|(owner, note)| owner == owner_id && note.job_id.as_deref() == Some(job_id))
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
        self.execute_tool_with_context(None, name, input).await
    }

    async fn execute_background_tool<Input, Output>(
        &self,
        dependencies: &PluginDependencies,
        cancellation: CancellationToken,
        name: &str,
        input: &Input,
    ) -> Result<Output, Problem>
    where
        Input: Serialize,
        Output: for<'de> Deserialize<'de>,
    {
        let context = dependencies
            .invocation_context_after(BACKGROUND_TOOL_TIMEOUT, cancellation)
            .map_err(|_| bad_gateway("background Tool is unavailable"))?;
        self.execute_tool_with_context(Some(context), name, input)
            .await
    }

    async fn execute_tool_with_context<Input, Output>(
        &self,
        context: Option<InvocationContext>,
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
        let request = ExecuteRequest {
            name: name.to_owned(),
            arguments_json: arguments.try_into().map_err(|error| {
                Problem::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_note",
                    format!("{error:?}"),
                )
            })?,
        };
        let response = match context {
            Some(context) => self.excerpt.execute_with_context(context, request).await,
            None => self.excerpt.execute(request).await,
        }
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
        processing_status: NoteProcessingStatus::from_wire(
            &row.get::<String, _>("processing_status"),
        )
        .expect("knowledge_reference.notes constrains processing_status"),
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

fn settings_command_problem(error: SettingsCommandError) -> Problem {
    settings_failure_problem(error.into())
}

fn settings_failure_problem(failure: SettingsFailure) -> Problem {
    let spec = problem_spec(failure);
    Problem::new(
        StatusCode::from_u16(spec.status).expect("settings problem has valid HTTP status"),
        spec.code,
        spec.detail,
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

struct CapturedAttachmentPolicy {
    max_bytes: usize,
    revision: Option<u64>,
}

fn prepare_attachment(
    note_id: String,
    input: UploadAttachment,
    policy: CapturedAttachmentPolicy,
) -> Result<(Attachment, Vec<u8>), Problem> {
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
    let content = decode_attachment_content(&input.content_base64, policy.max_bytes)?;
    let attachment = Attachment {
        id: format!("attachment-{}", Uuid::now_v7()),
        filename: input.filename,
        media_type: input.media_type,
        note_id,
        size: content.len(),
        policy_revision: policy.revision.map(|revision| revision.to_string()),
    };
    Ok((attachment, content))
}

async fn store_attachment(
    database: &PgPool,
    owner_id: &str,
    attachment: &Attachment,
    content: &[u8],
) -> Result<(), Problem> {
    sqlx::query(
        "INSERT INTO knowledge_reference.attachments \
         (owner_id, attachment_id, note_id, filename, media_type, content, policy_revision) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(owner_id)
    .bind(&attachment.id)
    .bind(&attachment.note_id)
    .bind(&attachment.filename)
    .bind(&attachment.media_type)
    .bind(content)
    .bind(&attachment.policy_revision)
    .execute(database)
    .await
    .map_err(database_error)?;
    Ok(())
}

fn capture_attachment_policy(
    source: Option<&Rc<dyn AttachmentPolicySource>>,
) -> Result<CapturedAttachmentPolicy, Problem> {
    let Some(source) = source else {
        return Ok(CapturedAttachmentPolicy {
            max_bytes: MAX_ATTACHMENT_BYTES,
            revision: None,
        });
    };
    let pinned = source
        .capture()
        .map_err(|_| attachment_policy_unavailable())?;
    let max_bytes = usize::try_from(pinned.value.max_attachment_bytes)
        .map_err(|_| attachment_policy_unavailable())?;
    if !(1..=MAX_ATTACHMENT_BYTES).contains(&max_bytes) || pinned.revision == 0 {
        return Err(attachment_policy_unavailable());
    }
    Ok(CapturedAttachmentPolicy {
        max_bytes,
        revision: Some(pinned.revision),
    })
}

fn attachment_policy_unavailable() -> Problem {
    Problem::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "attachment_policy_unavailable",
        "the authorized attachment policy is unavailable",
    )
}

fn decode_attachment_content(encoded: &str, max_bytes: usize) -> Result<Vec<u8>, Problem> {
    // Bound the decode allocation before inspecting attacker-controlled Base64.
    let max_encoded_len = max_bytes.div_ceil(3) * 4;
    if encoded.len() > max_encoded_len {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_attachment",
            format!("attachment size must be from 1 through {max_bytes} bytes"),
        ));
    }
    let content = STANDARD.decode(encoded).map_err(|_| {
        Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_attachment",
            "content_base64 is not valid Base64",
        )
    })?;
    if content.is_empty() || content.len() > max_bytes {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_attachment",
            format!("attachment size must be from 1 through {max_bytes} bytes"),
        ));
    }
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_openapi_operations_match_compiled_endpoint_routes() {
        use std::collections::BTreeSet;

        let document: serde_json::Value =
            serde_json::from_str(include_str!("../../../frontend/openapi.json")).unwrap();
        let paths = document["paths"].as_object().unwrap();
        let mut documented = BTreeSet::new();
        for (path, item) in paths {
            for (method, operation) in item.as_object().unwrap() {
                let operation_id = operation["operationId"].as_str().unwrap();
                assert_eq!(
                    operation["security"],
                    serde_json::json!([{"bearerAuth": []}]),
                    "{method} {path} must require the public bearer scheme"
                );
                assert!(documented.insert((
                    path.to_owned(),
                    method.to_ascii_uppercase(),
                    operation_id.to_owned(),
                )));
            }
        }

        let routes = <KnowledgeBase as lenso_capability_http_endpoint::HttpEndpoint>::ROUTES;
        let static_routes = routes
            .iter()
            .filter(|route| {
                matches!(
                    route.route_id(),
                    "knowledge-base.home"
                        | "knowledge-base.assets.js"
                        | "knowledge-base.assets.css"
                )
            })
            .map(|route| (route.path(), route.method(), route.route_id()))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            static_routes,
            BTreeSet::from([
                ("/", "GET", "knowledge-base.home"),
                ("/assets/app.js", "GET", "knowledge-base.assets.js"),
                ("/assets/index.css", "GET", "knowledge-base.assets.css"),
            ])
        );

        let public_routes = routes
            .iter()
            .filter(|route| {
                !matches!(
                    route.route_id(),
                    "knowledge-base.home"
                        | "knowledge-base.assets.js"
                        | "knowledge-base.assets.css"
                )
            })
            .map(|route| {
                (
                    route.path().to_owned(),
                    route.method().to_owned(),
                    route.route_id().to_owned(),
                )
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(public_routes.len(), 6);
        assert_eq!(documented, public_routes);
    }

    struct TestAttachmentPolicySource {
        revision: Cell<u64>,
        calls: Cell<u32>,
    }

    impl AttachmentPolicySource for TestAttachmentPolicySource {
        fn capture(&self) -> Result<PinnedAttachmentPolicy, AttachmentPolicyUnavailable> {
            self.calls.set(self.calls.get() + 1);
            let revision = self.revision.get();
            if revision == 0 {
                return Err(AttachmentPolicyUnavailable);
            }
            Ok(PinnedAttachmentPolicy {
                revision,
                value: AttachmentPolicy {
                    max_attachment_bytes: if revision == 1 { 32 } else { 64 },
                },
            })
        }
    }

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

    #[test]
    fn attachment_decoder_bounds_allocations_and_rejects_invalid_payloads() {
        assert_eq!(
            decode_attachment_content("cmVmZXJlbmNl", MAX_ATTACHMENT_BYTES).unwrap(),
            b"reference"
        );
        assert!(decode_attachment_content("", MAX_ATTACHMENT_BYTES).is_err());
        assert!(decode_attachment_content("not Base64", MAX_ATTACHMENT_BYTES).is_err());
        assert_eq!(
            decode_attachment_content(
                &STANDARD.encode(vec![0; MAX_ATTACHMENT_BYTES]),
                MAX_ATTACHMENT_BYTES
            )
            .unwrap()
            .len(),
            MAX_ATTACHMENT_BYTES
        );
        assert!(
            decode_attachment_content(
                &STANDARD.encode(vec![0; MAX_ATTACHMENT_BYTES + 1]),
                MAX_ATTACHMENT_BYTES
            )
            .is_err()
        );
        assert!(
            decode_attachment_content(
                &"A".repeat(MAX_ATTACHMENT_BYTES.div_ceil(3) * 4 + 1),
                MAX_ATTACHMENT_BYTES
            )
            .is_err()
        );
    }

    #[test]
    fn attachment_policy_is_pinned_once_per_upload() {
        let default = capture_attachment_policy(None).unwrap();
        assert_eq!(default.max_bytes, MAX_ATTACHMENT_BYTES);
        assert_eq!(default.revision, None);

        let source = Rc::new(TestAttachmentPolicySource {
            revision: Cell::new(1),
            calls: Cell::new(0),
        });
        let bound: Rc<dyn AttachmentPolicySource> = source.clone();
        let first = capture_attachment_policy(Some(&bound)).unwrap();
        assert_eq!((first.max_bytes, first.revision), (32, Some(1)));
        source.revision.set(2);
        assert!(decode_attachment_content(&STANDARD.encode(vec![0; 33]), first.max_bytes).is_err());
        assert_eq!(source.calls.get(), 1);

        let second = capture_attachment_policy(Some(&bound)).unwrap();
        assert_eq!((second.max_bytes, second.revision), (64, Some(2)));
        assert_eq!(
            decode_attachment_content(&STANDARD.encode(vec![0; 33]), second.max_bytes)
                .unwrap()
                .len(),
            33
        );
        assert_eq!(source.calls.get(), 2);
        source.revision.set(0);
        assert!(capture_attachment_policy(Some(&bound)).is_err());
    }

    #[test]
    fn attachment_policy_rejects_unbounded_or_unknown_values() {
        for value in [
            serde_json::json!({"max_attachment_bytes": 0}),
            serde_json::json!({"max_attachment_bytes": MAX_ATTACHMENT_BYTES + 1}),
            serde_json::json!({"max_attachment_bytes": 32, "extra": true}),
        ] {
            assert!(serde_json::from_value::<AttachmentPolicy>(value).is_err());
        }
        assert!(
            serde_json::from_value::<AttachmentPolicy>(
                serde_json::json!({"max_attachment_bytes": 32})
            )
            .is_ok()
        );
        assert_eq!(
            attachment_policy_schema()["properties"]["max_attachment_bytes"]["maximum"],
            serde_json::json!(MAX_ATTACHMENT_BYTES)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires a disposable PostgreSQL database prepared by knowledge-operator setup"]
    async fn pinned_attachment_revision_reaches_the_durable_write() {
        let database_url = std::env::var("LENSO_KNOWLEDGE_DATABASE_URL")
            .expect("set LENSO_KNOWLEDGE_DATABASE_URL to a disposable PostgreSQL database");
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .unwrap();
        let owner = format!("policy-test-{}", Uuid::now_v7());
        let note_id = format!("note-{}", Uuid::now_v7());
        let job_id = format!("job-{}", Uuid::now_v7());
        sqlx::query(
            "INSERT INTO knowledge_reference.settings (owner_id, revision, excerpt_limit) VALUES ($1, 2, 48)",
        )
        .bind(&owner)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO knowledge_reference.notes \
             (owner_id, note_id, title, body, excerpt, job_id, processing_status, config_revision) \
             VALUES ($1, $2, 'policy test', 'body', '', $3, 'succeeded', 2)",
        )
        .bind(&owner)
        .bind(&note_id)
        .bind(&job_id)
        .execute(&pool)
        .await
        .unwrap();

        let source = Rc::new(TestAttachmentPolicySource {
            revision: Cell::new(1),
            calls: Cell::new(0),
        });
        let bound: Rc<dyn AttachmentPolicySource> = source.clone();
        let pinned = capture_attachment_policy(Some(&bound)).unwrap();
        source.revision.set(2);
        let (attachment, content) = prepare_attachment(
            note_id,
            UploadAttachment {
                content_base64: STANDARD.encode(b"policy write"),
                filename: "policy.txt".to_owned(),
                media_type: "text/plain".to_owned(),
            },
            pinned,
        )
        .unwrap();
        store_attachment(&pool, &owner, &attachment, &content)
            .await
            .unwrap();
        let row = sqlx::query(
            "SELECT policy_revision, content FROM knowledge_reference.attachments \
             WHERE owner_id = $1 AND attachment_id = $2",
        )
        .bind(&owner)
        .bind(&attachment.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            row.get::<Option<String>, _>("policy_revision"),
            Some("1".to_owned())
        );
        assert_eq!(row.get::<Vec<u8>, _>("content"), b"policy write");
        assert_eq!(attachment.policy_revision.as_deref(), Some("1"));
        assert_eq!(source.calls.get(), 1);

        let settings = sqlx::query(
            "SELECT revision, excerpt_limit FROM knowledge_reference.settings WHERE owner_id = $1",
        )
        .bind(&owner)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(settings.get::<i64, _>("revision"), 2);
        assert_eq!(settings.get::<i64, _>("excerpt_limit"), 48);
        pool.close().await;
    }
}

#[cfg(test)]
mod openapi_schema_tests;
