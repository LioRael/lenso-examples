use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

use lenso_capability_agent_tool_provider::{self as tools, ExecuteRequest};
use lenso_capability_http_endpoint::{
    prelude::*,
    response::{self, Problem, StatusCode},
};
use serde::{Deserialize, Serialize};

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
    note_id: &'a str,
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
struct ProcessExcerptResponse {
    excerpt: String,
    job_id: String,
    note_id: String,
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

#[lenso::plugin]
#[derive(Clone, Debug)]
pub struct KnowledgeBase {
    #[dependency(id = "excerpt")]
    excerpt: tools::ToolProviderClient,
    next_id: Rc<Cell<u64>>,
    notes: Rc<RefCell<BTreeMap<String, Note>>>,
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
        Json(input): Json<CreateNote>,
    ) -> Result<(StatusCode, Json<Note>), Problem> {
        let (title, body) = validated_note(&input)?;
        let sequence = self.next_id.get() + 1;
        self.next_id.set(sequence);
        let note_id = format!("note-{sequence}");
        let queued = self
            .execute_tool::<_, EnqueueExcerptResponse>(
                "knowledge.enqueue-excerpt",
                &EnqueueExcerptRequest {
                    note_id: &note_id,
                    text: body,
                },
            )
            .await?;
        if !matches!(queued.status.as_str(), "queued" | "succeeded") {
            return Err(Problem::new(
                StatusCode::BAD_GATEWAY,
                "excerpt_failed",
                "excerpt processor returned an invalid status",
            ));
        }
        let note = Note {
            id: note_id,
            title: title.to_owned(),
            body: body.to_owned(),
            excerpt: queued.excerpt,
            job_id: queued.job_id,
            processing_status: queued.status,
        };
        self.notes
            .borrow_mut()
            .insert(note.id.clone(), note.clone());
        Ok((StatusCode::CREATED, Json(note)))
    }

    #[get("knowledge-base.notes.read", "/notes/{note_id}")]
    async fn read(&self, Path(path): Path<NotePath>) -> Result<Json<Note>, Problem> {
        self.notes
            .borrow()
            .get(&path.note_id)
            .cloned()
            .map(Json)
            .ok_or_else(|| {
                Problem::new(
                    StatusCode::NOT_FOUND,
                    "note_not_found",
                    "the note does not exist",
                )
            })
    }

    #[post("knowledge-base.jobs.process-next", "/jobs/process-next")]
    async fn process_next(&self) -> Result<Json<Note>, Problem> {
        let processed = self
            .execute_tool::<_, ProcessExcerptResponse>(
                "knowledge.process-excerpt",
                &serde_json::json!({}),
            )
            .await?;
        let mut notes = self.notes.borrow_mut();
        let note = notes.get_mut(&processed.note_id).ok_or_else(|| {
            Problem::new(
                StatusCode::CONFLICT,
                "orphaned_excerpt_job",
                "the claimed excerpt job does not match a note",
            )
        })?;
        if note.job_id != processed.job_id {
            return Err(Problem::new(
                StatusCode::CONFLICT,
                "excerpt_job_mismatch",
                "the claimed excerpt job identity does not match the note",
            ));
        }
        note.excerpt = processed.excerpt;
        note.processing_status = "succeeded".to_owned();
        Ok(Json(note.clone()))
    }

    #[get("knowledge-base.jobs.inspect", "/job-status/{job_id}")]
    async fn inspect_job(
        &self,
        Path(path): Path<JobPath>,
    ) -> Result<Json<InspectExcerptResponse>, Problem> {
        self.execute_tool::<_, InspectExcerptResponse>(
            "knowledge.inspect-excerpt",
            &InspectExcerptRequest {
                job_id: &path.job_id,
            },
        )
        .await
        .map(Json)
    }
}

impl KnowledgeBase {
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
            .map_err(|error| {
                Problem::new(
                    StatusCode::BAD_GATEWAY,
                    "excerpt_failed",
                    format!("{error:?}"),
                )
            })?;
        serde_json::from_str(&response.content).map_err(|error| {
            Problem::new(
                StatusCode::BAD_GATEWAY,
                "excerpt_failed",
                format!("invalid Tool response: {error}"),
            )
        })
    }
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
