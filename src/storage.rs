//! Private Story persistence, idempotency, retention, and recovery workflows.

use std::{
    cmp::Ordering as CmpOrdering,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use lenso_capability_story_events::RecordRequest;

use super::{CURRENT_SCHEMA_VERSION, INITIAL_MIGRATION};

mod model;

use model::{
    EventIdentity, IngestOutcome, LegacyStoryDocument, OwnedMigration, StoredStoryEntry,
    StoryDocument,
};

pub use model::{StoryRecoveryOutcome, StorySetupOutcome, StoryStorageError, StoryUpgradeOutcome};

static NEXT_PROBE_ID: AtomicU64 = AtomicU64::new(0);

/// A private persistence Adapter owned by the Story Module.
#[derive(Clone, Debug)]
pub(super) struct FileStoryAdapter {
    path: PathBuf,
}

impl FileStoryAdapter {
    /// Selects one required durable document path. No file is created here.
    pub(super) fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Applies the owned initial migration, or reports that it is already applied.
    pub(super) fn setup(&self) -> Result<StorySetupOutcome, StoryStorageError> {
        let parent = self.ensure_parent()?;
        let _lock = self.acquire_lock(true)?;
        self.require_recovered()?;
        let migration = Self::owned_migration()?;
        if self.path.exists() {
            self.read_current_document()?;
            return Ok(StorySetupOutcome::AlreadyCurrent {
                schema_version: migration.version,
            });
        }
        self.write_document(&migration.initial_document, parent)?;
        Ok(StorySetupOutcome::Created {
            schema_version: migration.version,
        })
    }

    /// Applies an explicit owner migration and never runs from Module preparation.
    pub(super) fn upgrade(&self) -> Result<StoryUpgradeOutcome, StoryStorageError> {
        let parent = self.parent()?;
        let _lock = self.acquire_lock(true)?;
        self.require_recovered()?;
        let migration = Self::owned_migration()?;
        let value = self.read_json_value(&self.path)?;
        let version = document_version(&self.path, &value)?;
        match version {
            version if version == migration.version => {
                self.read_current_document()?;
                Ok(StoryUpgradeOutcome::AlreadyCurrent {
                    schema_version: migration.version,
                })
            }
            0 => {
                let legacy: LegacyStoryDocument =
                    serde_json::from_value(value).map_err(|error| {
                        StoryStorageError::InvalidDocument {
                            path: self.path.clone(),
                            detail: error.to_string(),
                        }
                    })?;
                if legacy.schema_version != 0 {
                    return Err(StoryStorageError::InvalidDocument {
                        path: self.path.clone(),
                        detail: format!(
                            "legacy Story document declared schema {} instead of 0",
                            legacy.schema_version
                        ),
                    });
                }
                let event_id_order = legacy
                    .entries
                    .iter()
                    .map(|entry| entry.event_id.clone())
                    .collect();
                let event_ids = legacy
                    .entries
                    .iter()
                    .map(|entry| {
                        (
                            entry.event_id.clone(),
                            EventIdentity {
                                event_version: entry.event_version,
                                occurred_at: entry.occurred_at.clone(),
                                subject_id: entry.subject_id.clone(),
                                event_type: entry.event_type.clone(),
                                facts: entry.facts.clone(),
                            },
                        )
                    })
                    .collect();
                let revision = legacy
                    .entries
                    .iter()
                    .map(|entry| entry.revision)
                    .max()
                    .unwrap_or(0);
                self.write_document(
                    &StoryDocument {
                        schema_version: migration.version,
                        revision,
                        entries: legacy.entries,
                        event_ids,
                        event_id_order,
                    },
                    parent,
                )?;
                Ok(StoryUpgradeOutcome::Applied {
                    from: 0,
                    to: migration.version,
                })
            }
            actual => Err(StoryStorageError::IncompatibleSchema {
                path: self.path.clone(),
                expected: migration.version,
                actual,
            }),
        }
    }

    /// Reconciles one interrupted commit as an explicit operator action.
    pub(super) fn recover(&self) -> Result<StoryRecoveryOutcome, StoryStorageError> {
        let parent = self.parent()?;
        let _lock = self.acquire_lock(true)?;
        let temporary_path = self.temporary_path();
        if !temporary_path.exists() {
            return Ok(StoryRecoveryOutcome::NoAction);
        }
        if self.path.exists() {
            self.read_current_document()?;
            std::fs::remove_file(&temporary_path).map_err(|error| {
                StoryStorageError::io(&temporary_path, "discard interrupted transaction", &error)
            })?;
            sync_directory(parent)?;
            return Ok(StoryRecoveryOutcome::DiscardedUncommitted);
        }

        let document = self.read_current_document_at(&temporary_path)?;
        std::fs::rename(&temporary_path, &self.path)
            .map_err(|error| StoryStorageError::io(&self.path, "restore transaction", &error))?;
        sync_directory(parent)?;
        Ok(StoryRecoveryOutcome::Restored {
            schema_version: document.schema_version,
        })
    }

    /// Verifies storage, schema, and write capability without changing state.
    pub(super) fn verify_ready(&self) -> Result<(), StoryStorageError> {
        if !self.path.exists() && !self.temporary_path().exists() {
            return Err(StoryStorageError::Missing {
                path: self.path.clone(),
            });
        }
        let parent = self.parent()?;
        let _lock = self.acquire_lock(false)?;
        self.require_recovered()?;
        Self::owned_migration()?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .map_err(|error| {
                StoryStorageError::io(&self.path, "open storage read-write", &error)
            })?;
        self.read_current_document_from(&mut file, &self.path)?;
        self.probe_parent_writable(parent)
    }

    /// Ingests one explicit Event with durable idempotency and owner retention.
    pub(super) fn ingest(
        &self,
        event: &RecordRequest,
        source_instance: &str,
        retention_limit: usize,
        idempotency_limit: usize,
    ) -> Result<IngestOutcome, StoryStorageError> {
        let occurred_at = canonical_occurred_at(event)?;
        let parent = self.parent()?;
        let _lock = self.acquire_lock(false)?;
        self.require_recovered()?;
        let mut document = self.read_current_document()?;
        let identity = EventIdentity::from_event(event, occurred_at.clone());
        if let Some(previous) = document.event_ids.get(&event.event_id) {
            if previous == &identity {
                return Ok(IngestOutcome::Duplicate);
            }
            return Err(StoryStorageError::ConflictingEventId {
                event_id: event.event_id.clone(),
            });
        }
        document.revision =
            document
                .revision
                .checked_add(1)
                .ok_or_else(|| StoryStorageError::InvalidDocument {
                    path: self.path.clone(),
                    detail: "Story revision overflow".to_owned(),
                })?;
        document.event_ids.insert(event.event_id.clone(), identity);
        document.event_id_order.push(event.event_id.clone());
        document.entries.push(StoredStoryEntry {
            event_id: event.event_id.clone(),
            event_version: event.event_version,
            occurred_at,
            subject_id: event.subject_id.clone(),
            event_type: event.event_type.clone(),
            facts: event.facts.clone(),
            source_instance: source_instance.to_owned(),
            revision: document.revision,
        });
        document.entries.sort_by(|left, right| {
            compare_occurred_at(&left.occurred_at, &right.occurred_at)
                .then_with(|| left.event_id.cmp(&right.event_id))
                .then_with(|| left.revision.cmp(&right.revision))
        });
        let evicted = document.entries.len().saturating_sub(retention_limit);
        if evicted > 0 {
            document.entries.drain(..evicted);
        }
        let expired_identities = document
            .event_id_order
            .len()
            .saturating_sub(idempotency_limit);
        let expired_event_ids: Vec<_> = document
            .event_id_order
            .drain(..expired_identities)
            .collect();
        for event_id in expired_event_ids {
            document.event_ids.remove(&event_id);
        }
        self.write_document(&document, parent)?;
        Ok(IngestOutcome::Inserted)
    }

    /// Reads the retained timeline entries for one subject in deterministic order.
    pub(super) fn timeline(
        &self,
        subject_id: &str,
        limit: usize,
    ) -> Result<Vec<StoredStoryEntry>, StoryStorageError> {
        let _lock = self.acquire_lock(false)?;
        self.require_recovered()?;
        let document = self.read_current_document()?;
        Ok(document
            .entries
            .into_iter()
            .filter(|entry| entry.subject_id == subject_id)
            .take(limit)
            .collect())
    }

    fn parent(&self) -> Result<&Path, StoryStorageError> {
        self.path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or_else(|| StoryStorageError::InvalidPath {
                path: self.path.clone(),
            })
    }

    fn ensure_parent(&self) -> Result<&Path, StoryStorageError> {
        let parent = self.parent()?;
        std::fs::create_dir_all(parent).map_err(|error| {
            StoryStorageError::io(&self.path, "create storage directory", &error)
        })?;
        Ok(parent)
    }

    fn acquire_lock(&self, create: bool) -> Result<File, StoryStorageError> {
        let lock_path = self.lock_path();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .open(&lock_path)
            .map_err(|error| {
                if !create && error.kind() == std::io::ErrorKind::NotFound {
                    StoryStorageError::MissingLock {
                        path: lock_path.clone(),
                    }
                } else {
                    StoryStorageError::io(&lock_path, "open transaction lock", &error)
                }
            })?;
        File::lock(&file).map_err(|error| {
            StoryStorageError::io(&lock_path, "acquire transaction lock", &error)
        })?;
        Ok(file)
    }

    fn read_current_document(&self) -> Result<StoryDocument, StoryStorageError> {
        let mut file = File::open(&self.path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                StoryStorageError::Missing {
                    path: self.path.clone(),
                }
            } else {
                StoryStorageError::io(&self.path, "read storage", &error)
            }
        })?;
        self.read_current_document_from(&mut file, &self.path)
    }

    fn read_current_document_at(&self, path: &Path) -> Result<StoryDocument, StoryStorageError> {
        let mut file = File::open(path)
            .map_err(|error| StoryStorageError::io(path, "read recovery document", &error))?;
        self.read_current_document_from(&mut file, path)
    }

    fn read_current_document_from(
        &self,
        reader: &mut File,
        path: &Path,
    ) -> Result<StoryDocument, StoryStorageError> {
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|error| StoryStorageError::io(path, "read storage", &error))?;
        let value =
            serde_json::from_slice(&bytes).map_err(|error| StoryStorageError::InvalidDocument {
                path: path.to_owned(),
                detail: error.to_string(),
            })?;
        let actual = document_version(path, &value)?;
        if actual != CURRENT_SCHEMA_VERSION {
            return Err(StoryStorageError::IncompatibleSchema {
                path: path.to_owned(),
                expected: CURRENT_SCHEMA_VERSION,
                actual,
            });
        }
        let document: StoryDocument =
            serde_json::from_value(value).map_err(|error| StoryStorageError::InvalidDocument {
                path: path.to_owned(),
                detail: error.to_string(),
            })?;
        validate_document(path, &document)?;
        Ok(document)
    }

    fn owned_migration() -> Result<OwnedMigration, StoryStorageError> {
        let migration: OwnedMigration =
            serde_json::from_str(INITIAL_MIGRATION).map_err(|error| {
                StoryStorageError::InvalidMigration {
                    detail: error.to_string(),
                }
            })?;
        if migration.version != CURRENT_SCHEMA_VERSION
            || migration.initial_document.schema_version != migration.version
        {
            return Err(StoryStorageError::InvalidMigration {
                detail: format!(
                    "artifact version {} does not match Story schema {}",
                    migration.version, CURRENT_SCHEMA_VERSION
                ),
            });
        }
        Ok(migration)
    }

    fn read_json_value(&self, path: &Path) -> Result<serde_json::Value, StoryStorageError> {
        let bytes = std::fs::read(path)
            .map_err(|error| StoryStorageError::io(path, "read storage", &error))?;
        serde_json::from_slice(&bytes).map_err(|error| StoryStorageError::InvalidDocument {
            path: path.to_owned(),
            detail: error.to_string(),
        })
    }

    fn write_document(
        &self,
        document: &StoryDocument,
        parent: &Path,
    ) -> Result<(), StoryStorageError> {
        let temporary_path = self.temporary_path();
        let bytes = serde_json::to_vec_pretty(document).map_err(|error| {
            StoryStorageError::InvalidDocument {
                path: self.path.clone(),
                detail: error.to_string(),
            }
        })?;
        let mut temporary = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary_path)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    StoryStorageError::RecoveryRequired {
                        path: temporary_path.clone(),
                    }
                } else {
                    StoryStorageError::io(&temporary_path, "create transaction", &error)
                }
            })?;
        temporary
            .write_all(&bytes)
            .map_err(|error| StoryStorageError::io(&temporary_path, "write transaction", &error))?;
        temporary
            .sync_all()
            .map_err(|error| StoryStorageError::io(&temporary_path, "sync transaction", &error))?;
        std::fs::rename(&temporary_path, &self.path)
            .map_err(|error| StoryStorageError::io(&self.path, "commit transaction", &error))?;
        sync_directory(parent)
    }

    fn require_recovered(&self) -> Result<(), StoryStorageError> {
        let temporary_path = self.temporary_path();
        if temporary_path.exists() {
            Err(StoryStorageError::RecoveryRequired {
                path: temporary_path,
            })
        } else {
            Ok(())
        }
    }

    fn probe_parent_writable(&self, parent: &Path) -> Result<(), StoryStorageError> {
        let probe_path = sibling_path(
            &self.path,
            &format!(
                ".probe.{}.{}",
                std::process::id(),
                NEXT_PROBE_ID.fetch_add(1, Ordering::Relaxed)
            ),
        );
        let probe = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&probe_path)
            .map_err(|error| {
                StoryStorageError::io(&probe_path, "probe storage directory", &error)
            })?;
        probe
            .sync_all()
            .map_err(|error| StoryStorageError::io(&probe_path, "sync storage probe", &error))?;
        std::fs::remove_file(&probe_path)
            .map_err(|error| StoryStorageError::io(&probe_path, "remove storage probe", &error))?;
        sync_directory(parent)
    }

    fn lock_path(&self) -> PathBuf {
        sibling_path(&self.path, ".lock")
    }

    fn temporary_path(&self) -> PathBuf {
        sibling_path(&self.path, ".tmp")
    }
}

fn canonical_occurred_at(event: &RecordRequest) -> Result<String, StoryStorageError> {
    if event.event_id.is_empty()
        || event.event_version < 1
        || event.occurred_at.is_empty()
        || event.subject_id.is_empty()
        || event.event_type.is_empty()
    {
        return Err(StoryStorageError::InvalidEvent {
            detail: "event_id, positive event_version, occurred_at, subject_id, and event_type are required"
                .to_owned(),
        });
    }
    let occurred_at = time::OffsetDateTime::parse(
        &event.occurred_at,
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|error| StoryStorageError::InvalidEvent {
        detail: format!("occurred_at is not RFC 3339: {error}"),
    })?;
    occurred_at
        .to_offset(time::UtcOffset::UTC)
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| StoryStorageError::InvalidEvent {
            detail: format!("occurred_at cannot be normalized: {error}"),
        })
}

fn compare_occurred_at(left: &str, right: &str) -> CmpOrdering {
    let format = &time::format_description::well_known::Rfc3339;
    let left = time::OffsetDateTime::parse(left, format)
        .expect("stored Story timestamps are validated before sorting");
    let right = time::OffsetDateTime::parse(right, format)
        .expect("stored Story timestamps are validated before sorting");
    left.cmp(&right)
}

fn validate_document(path: &Path, document: &StoryDocument) -> Result<(), StoryStorageError> {
    let format = &time::format_description::well_known::Rfc3339;
    if document.event_id_order.len() != document.event_ids.len()
        || document
            .event_id_order
            .iter()
            .any(|event_id| !document.event_ids.contains_key(event_id))
    {
        return Err(StoryStorageError::InvalidDocument {
            path: path.to_owned(),
            detail: "event idempotency order does not match its identity index".to_owned(),
        });
    }
    for entry in &document.entries {
        time::OffsetDateTime::parse(&entry.occurred_at, format).map_err(|error| {
            StoryStorageError::InvalidDocument {
                path: path.to_owned(),
                detail: format!(
                    "entry `{}` has an invalid occurred_at: {error}",
                    entry.event_id
                ),
            }
        })?;
    }
    Ok(())
}

fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn document_version(path: &Path, value: &serde_json::Value) -> Result<u32, StoryStorageError> {
    value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .ok_or_else(|| StoryStorageError::InvalidDocument {
            path: path.to_owned(),
            detail: "schema_version is missing or too large".to_owned(),
        })
}

fn sync_directory(parent: &Path) -> Result<(), StoryStorageError> {
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| StoryStorageError::io(parent, "sync storage directory", &error))
}
