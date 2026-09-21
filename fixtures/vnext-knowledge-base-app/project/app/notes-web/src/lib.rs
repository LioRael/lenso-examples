use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

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
}

#[lenso::plugin]
#[derive(Clone, Debug, Default)]
pub struct KnowledgeBase {
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

    #[post("knowledge-base.notes.create", "/notes")]
    async fn create(
        &self,
        Json(input): Json<CreateNote>,
    ) -> Result<(StatusCode, Json<Note>), Problem> {
        let title = input.title.trim();
        let body = input.body.trim();
        if title.is_empty() || body.is_empty() {
            return Err(Problem::new(
                StatusCode::BAD_REQUEST,
                "invalid_note",
                "title and body must not be empty",
            ));
        }

        let sequence = self.next_id.get() + 1;
        self.next_id.set(sequence);
        let note = Note {
            id: format!("note-{sequence}"),
            title: title.to_owned(),
            body: body.to_owned(),
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

#[cfg(test)]
mod tests {
    use lenso_capability_http_endpoint::testing::EndpointTest;

    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn creates_and_reads_a_note_without_opening_a_socket() {
        let endpoint = EndpointTest::new(KnowledgeBase::default());
        let created = endpoint
            .request("knowledge-base.notes.create")
            .json(&CreateNote {
                title: "First note".to_owned(),
                body: "Created through the public App path.".to_owned(),
            })
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let note = created.json::<Note>().unwrap();

        let read = endpoint
            .request("knowledge-base.notes.read")
            .path_parameter("note_id", &note.id)
            .send()
            .await
            .unwrap();
        assert_eq!(read.status(), StatusCode::OK);
        assert_eq!(read.json::<Note>().unwrap(), note);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_empty_notes_and_missing_ids() {
        let endpoint = EndpointTest::new(KnowledgeBase::default());
        let invalid = endpoint
            .request("knowledge-base.notes.create")
            .json(&CreateNote {
                title: " ".to_owned(),
                body: "content".to_owned(),
            })
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

        let missing = endpoint
            .request("knowledge-base.notes.read")
            .path_parameter("note_id", "missing")
            .send()
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }
}
