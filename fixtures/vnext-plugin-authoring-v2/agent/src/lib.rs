use lenso_agent_tool_sdk::prelude::*;
use lenso_capability_document_sync::{
    DocumentSyncClient, DocumentSyncInvocationError, SyncRequest,
};
use schemars::JsonSchema;

#[derive(Debug, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SyncDocumentArguments {
    #[schemars(length(min = 1, max = 128))]
    document: String,
}

#[lenso::plugin]
#[derive(Clone, Debug)]
struct DocumentSyncTools {
    sync: DocumentSyncClient,
}

#[tool_provider]
impl DocumentSyncTools {
    #[tool(
        name = "sync_document",
        description = "Copy one document from the selected source account to the selected destination account.",
        execution = "exclusive"
    )]
    async fn sync_document(
        &self,
        arguments: SyncDocumentArguments,
    ) -> Result<ExecuteResponse, ExecuteError> {
        let response = self
            .sync
            .sync(SyncRequest {
                document: arguments.document,
            })
            .await
            .map_err(|error| {
                let (reason_code, message) = match error {
                    DocumentSyncInvocationError::Domain(error) => {
                        ("document_sync_failed", format!("{error:?}"))
                    }
                    DocumentSyncInvocationError::Runtime(error) => {
                        ("document_sync_unavailable", format!("{error:?}"))
                    }
                };
                ExecuteError::ExecutionFailed {
                    payload: tool_provider_contract::ExecutionFailedPayload {
                        details_json: "{}".try_into().expect("empty details are valid JSON"),
                        message,
                        reason_code: reason_code.to_owned(),
                    },
                }
            })?;
        Ok(ExecuteResponse {
            content_blocks: None,
            content: "updated".to_owned(),
            content_type: ContentType::Text,
            metadata_json: format!(
                "{{\"document\":{},\"text\":{}}}",
                serde_json::to_string(&response.document).expect("document is JSON"),
                serde_json::to_string(&response.text).expect("text is JSON")
            )
            .try_into()
            .expect("generated Tool metadata is JSON"),
        })
    }
}
