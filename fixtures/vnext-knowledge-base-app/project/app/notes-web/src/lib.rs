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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Note {
    id: String,
    title: String,
    body: String,
    excerpt: String,
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

        let arguments =
            serde_json::to_string(&serde_json::json!({ "text": body })).map_err(|error| {
                Problem::new(StatusCode::BAD_REQUEST, "invalid_note", error.to_string())
            })?;
        let excerpt = self
            .excerpt
            .execute(ExecuteRequest {
                name: "knowledge.excerpt".into(),
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
        let excerpt = serde_json::from_str::<String>(&excerpt.content).unwrap_or(excerpt.content);
        let sequence = self.next_id.get() + 1;
        self.next_id.set(sequence);
        let note = Note {
            id: format!("note-{sequence}"),
            title: title.to_owned(),
            body: body.to_owned(),
            excerpt,
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
