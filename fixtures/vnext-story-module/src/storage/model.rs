//! Private Story storage document, migration, and error model.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use lenso_capability_story_events::RecordRequest;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct StoredStoryEntry {
    pub(crate) event_id: String,
    pub(crate) event_version: i64,
    pub(crate) occurred_at: String,
    pub(crate) subject_id: String,
    pub(crate) event_type: String,
    pub(crate) facts: BTreeMap<String, serde_json::Value>,
    pub(crate) source_instance: String,
    pub(crate) revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct EventIdentity {
    pub(super) event_version: i64,
    pub(super) occurred_at: String,
    pub(super) subject_id: String,
    pub(super) event_type: String,
    pub(super) facts: BTreeMap<String, serde_json::Value>,
}

impl EventIdentity {
    pub(super) fn from_event(event: &RecordRequest, occurred_at: String) -> Self {
        Self {
            event_version: event.event_version,
            occurred_at,
            subject_id: event.subject_id.clone(),
            event_type: event.event_type.clone(),
            facts: event.facts.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct StoryDocument {
    pub(super) schema_version: u32,
    pub(super) revision: u64,
    pub(super) entries: Vec<StoredStoryEntry>,
    #[serde(default)]
    pub(super) event_ids: BTreeMap<String, EventIdentity>,
    #[serde(default)]
    pub(super) event_id_order: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OwnedMigration {
    pub(super) version: u32,
    pub(super) initial_document: StoryDocument,
}

#[derive(Debug, Deserialize)]
pub(super) struct LegacyStoryDocument {
    pub(super) schema_version: u32,
    #[serde(default)]
    pub(super) entries: Vec<StoredStoryEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IngestOutcome {
    Inserted,
    Duplicate,
}

/// Reviewable result from the explicit Story setup workflow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorySetupOutcome {
    /// The initial owned migration created storage.
    Created { schema_version: u32 },
    /// Storage already has the current owned schema.
    AlreadyCurrent { schema_version: u32 },
}

/// Reviewable result from the explicit Story upgrade workflow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoryUpgradeOutcome {
    /// No migration was required.
    AlreadyCurrent { schema_version: u32 },
    /// One owned migration was applied.
    Applied { from: u32, to: u32 },
}

/// Reviewable result from the explicit Story recovery workflow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoryRecoveryOutcome {
    /// No interrupted transaction existed.
    NoAction,
    /// The stable document won and the temporary document was discarded.
    DiscardedUncommitted,
    /// A complete temporary document was restored.
    Restored { schema_version: u32 },
}

/// Failure from the private Story persistence Adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoryStorageError {
    /// Required durable storage does not exist.
    Missing { path: PathBuf },
    /// Setup or upgrade has not created the transaction lock.
    MissingLock { path: PathBuf },
    /// The configured path cannot be used as storage.
    InvalidPath { path: PathBuf },
    /// Storage is malformed or cannot be decoded.
    InvalidDocument { path: PathBuf, detail: String },
    /// The compiled migration artifact is malformed.
    InvalidMigration { detail: String },
    /// Storage has a version requiring an explicit upgrade.
    IncompatibleSchema {
        path: PathBuf,
        expected: u32,
        actual: u32,
    },
    /// An interrupted transaction requires explicit recovery.
    RecoveryRequired { path: PathBuf },
    /// One business Event is not valid for Story durability.
    InvalidEvent { detail: String },
    /// One Event ID was reused for different business facts.
    ConflictingEventId { event_id: String },
    /// The host filesystem rejected an owned operation.
    Io {
        path: PathBuf,
        operation: String,
        detail: String,
    },
}

impl StoryStorageError {
    pub(super) fn io(path: &Path, operation: &str, error: &std::io::Error) -> Self {
        Self::Io {
            path: path.to_owned(),
            operation: operation.to_owned(),
            detail: error.to_string(),
        }
    }
}

impl std::fmt::Display for StoryStorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { path } => write!(
                formatter,
                "required durable Story storage `{}` is missing; run setup",
                path.display()
            ),
            Self::MissingLock { path } => write!(
                formatter,
                "Story transaction lock `{}` is missing; run the explicit setup or upgrade workflow",
                path.display()
            ),
            Self::InvalidPath { path } => {
                write!(
                    formatter,
                    "Story storage path `{}` is invalid",
                    path.display()
                )
            }
            Self::InvalidDocument { path, detail } => write!(
                formatter,
                "Story storage document `{}` is invalid: {detail}",
                path.display()
            ),
            Self::InvalidMigration { detail } => {
                write!(
                    formatter,
                    "owned Story migration artifact is invalid: {detail}"
                )
            }
            Self::IncompatibleSchema {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "Story storage `{}` uses schema {actual}; expected {expected}, run the explicit upgrade workflow",
                path.display()
            ),
            Self::RecoveryRequired { path } => write!(
                formatter,
                "interrupted Story transaction `{}` requires the explicit recovery workflow",
                path.display()
            ),
            Self::InvalidEvent { detail } => write!(formatter, "invalid business Event: {detail}"),
            Self::ConflictingEventId { event_id } => write!(
                formatter,
                "business Event ID `{event_id}` was reused with different facts"
            ),
            Self::Io {
                path,
                operation,
                detail,
            } => write!(
                formatter,
                "cannot {operation} Story storage `{}`: {detail}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for StoryStorageError {}
