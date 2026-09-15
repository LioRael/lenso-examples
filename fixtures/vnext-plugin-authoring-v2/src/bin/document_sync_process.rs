// Capability bindings are generated from the portable contract. Keep their
// public by-value request API stable and do not rewrite generated macro bodies.
#![allow(clippy::manual_let_else, clippy::needless_pass_by_value)]

use std::sync::atomic::{AtomicBool, Ordering};

#[allow(dead_code)]
mod document_store {
    include!("../generated/document_store.rs");
}

include!("../generated/document_sync.rs");

use document_store::{DocumentStoreClient, PutRequest, ReadError, ReadRequest};
use lenso_plugin_sdk::{CallError, CreateContext, Ctx, Plugin, Requirement};

const REQUIREMENTS: &[Requirement] = &[
    Requirement::one::<DocumentStoreClient>("destination"),
    Requirement::one::<DocumentStoreClient>("source"),
];

#[lenso_plugin_sdk::plugin]
struct DocumentSyncPlugin {
    source: DocumentStoreClient,
    destination: DocumentStoreClient,
    running: AtomicBool,
}

impl Plugin for DocumentSyncPlugin {
    const CONFIGURATION_SCHEMA: Option<&'static str> =
        Some(include_str!("../../sync-config.schema.json"));

    fn requirements() -> &'static [Requirement] {
        REQUIREMENTS
    }

    fn create(context: CreateContext) -> Result<Self, String> {
        let ruleset = context
            .config()
            .get("ruleset")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "config requires ruleset".to_owned())?;
        if ruleset != "identity-v1" {
            return Err("unsupported ruleset".to_owned());
        }
        Ok(Self {
            source: context.dependencies().one("source")?.client()?,
            destination: context.dependencies().one("destination")?.client()?,
            running: AtomicBool::new(false),
        })
    }
}

impl DocumentSyncProvider for DocumentSyncPlugin {
    fn sync(&self, context: Ctx, request: SyncRequest) -> Result<SyncResponse, SyncError> {
        if self.running.swap(true, Ordering::AcqRel) {
            return Err(SyncError::AlreadyRunning);
        }
        let result = self.sync_inner(&context, request);
        self.running.store(false, Ordering::Release);
        result
    }
}

impl DocumentSyncPlugin {
    fn sync_inner(&self, context: &Ctx, request: SyncRequest) -> Result<SyncResponse, SyncError> {
        let text = self
            .source
            .read(
                context,
                ReadRequest {
                    document: request.document.clone(),
                },
            )
            .map_err(|error| match error {
                CallError::Domain(ReadError::NotFound) => SyncError::NotFound,
                _ => SyncError::WriteFailed,
            })?
            .text;
        self.destination
            .put(
                context,
                PutRequest {
                    document: request.document.clone(),
                    text: text.clone(),
                },
            )
            .map_err(|_| SyncError::WriteFailed)?;
        Ok(SyncResponse {
            document: request.document,
            text,
        })
    }
}

export_document_sync_plugin!(DocumentSyncPlugin);
