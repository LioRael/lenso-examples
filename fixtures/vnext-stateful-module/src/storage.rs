//! Private file-backed persistence and owned migration workflows.

use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

use super::{CURRENT_SCHEMA_VERSION, INITIAL_MIGRATION};

static NEXT_PROBE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct CounterDocument {
    schema_version: u32,
    revision: u64,
    entries: BTreeMap<String, i64>,
}

#[derive(Debug, Deserialize)]
struct OwnedMigration {
    version: u32,
    initial_document: CounterDocument,
}

#[derive(Debug, Deserialize)]
struct LegacyCounterDocument {
    schema_version: u32,
    entries: BTreeMap<String, i64>,
}

/// A private persistence Adapter owned by the counter Module.
#[derive(Clone, Debug)]
pub(super) struct FileStateAdapter {
    path: PathBuf,
}

impl FileStateAdapter {
    /// Selects one required durable document path. No file is created here.
    pub(super) fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Applies the owned initial migration, or reports that it is already applied.
    pub(super) fn setup(&self) -> Result<SetupOutcome, StateStorageError> {
        let parent = self.ensure_parent()?;
        let _lock = self.acquire_lock(true)?;
        self.require_recovered()?;
        let migration = Self::owned_migration()?;
        if self.path.exists() {
            self.read_current_document()?;
            return Ok(SetupOutcome::AlreadyCurrent {
                schema_version: migration.version,
            });
        }
        self.write_document(&migration.initial_document, parent)?;
        Ok(SetupOutcome::Created {
            schema_version: migration.version,
        })
    }

    /// Applies an explicit owned upgrade and never runs from Module preparation.
    pub(super) fn upgrade(&self) -> Result<UpgradeOutcome, StateStorageError> {
        let parent = self.parent()?;
        let _lock = self.acquire_lock(true)?;
        self.require_recovered()?;
        let migration = Self::owned_migration()?;
        let value = self.read_json_value(&self.path)?;
        let version = document_version(&self.path, &value)?;
        match version {
            version if version == migration.version => Ok(UpgradeOutcome::AlreadyCurrent {
                schema_version: migration.version,
            }),
            0 => {
                let legacy: LegacyCounterDocument =
                    serde_json::from_value(value).map_err(|error| {
                        StateStorageError::InvalidDocument {
                            path: self.path.clone(),
                            detail: error.to_string(),
                        }
                    })?;
                self.write_document(
                    &CounterDocument {
                        schema_version: migration.version,
                        revision: migration.initial_document.revision,
                        entries: legacy.entries,
                    },
                    parent,
                )?;
                Ok(UpgradeOutcome::Applied {
                    from: legacy.schema_version,
                    to: migration.version,
                })
            }
            actual => Err(StateStorageError::IncompatibleSchema {
                path: self.path.clone(),
                expected: migration.version,
                actual,
            }),
        }
    }

    /// Reconciles an interrupted durable commit as an explicit operator action.
    pub(super) fn recover(&self) -> Result<RecoveryOutcome, StateStorageError> {
        let parent = self.parent()?;
        let _lock = self.acquire_lock(true)?;
        let temporary_path = self.temporary_path();
        if !temporary_path.exists() {
            return Ok(RecoveryOutcome::NoAction);
        }
        if self.path.exists() {
            let value = self.read_json_value(&self.path)?;
            document_version(&self.path, &value)?;
            std::fs::remove_file(&temporary_path).map_err(|error| {
                StateStorageError::io(&temporary_path, "discard interrupted transaction", &error)
            })?;
            sync_directory(parent)?;
            return Ok(RecoveryOutcome::DiscardedUncommitted);
        }

        let document = self.read_current_document_at(&temporary_path)?;
        std::fs::rename(&temporary_path, &self.path)
            .map_err(|error| StateStorageError::io(&self.path, "restore transaction", &error))?;
        sync_directory(parent)?;
        Ok(RecoveryOutcome::Restored {
            schema_version: document.schema_version,
        })
    }

    /// Verifies storage, schema, and write capability without changing owned state.
    pub(super) fn verify_ready(&self) -> Result<(), StateStorageError> {
        if !self.path.exists() && !self.temporary_path().exists() {
            return Err(StateStorageError::Missing {
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
                StateStorageError::io(&self.path, "open storage read-write", &error)
            })?;
        self.read_current_document_from(&mut file, &self.path)?;
        self.probe_parent_writable(parent)
    }

    pub(crate) fn read_counter(&self, key: &str) -> Result<Option<(i64, u64)>, StateStorageError> {
        let _lock = self.acquire_lock(false)?;
        self.require_recovered()?;
        let document = self.read_current_document()?;
        Ok(document
            .entries
            .get(key)
            .copied()
            .map(|value| (value, document.revision)))
    }

    pub(crate) fn increment_counter(
        &self,
        key: &str,
        amount: i64,
    ) -> Result<(i64, u64), StateStorageError> {
        let parent = self.parent()?;
        let _lock = self.acquire_lock(false)?;
        self.require_recovered()?;
        let mut document = self.read_current_document()?;
        let value = document.entries.entry(key.to_owned()).or_default();
        *value = value
            .checked_add(amount)
            .ok_or_else(|| StateStorageError::InvalidDocument {
                path: self.path.clone(),
                detail: "counter value overflow".to_owned(),
            })?;
        document.revision =
            document
                .revision
                .checked_add(1)
                .ok_or_else(|| StateStorageError::InvalidDocument {
                    path: self.path.clone(),
                    detail: "revision overflow".to_owned(),
                })?;
        let result = (*value, document.revision);
        self.write_document(&document, parent)?;
        Ok(result)
    }

    fn parent(&self) -> Result<&Path, StateStorageError> {
        self.path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or_else(|| StateStorageError::InvalidPath {
                path: self.path.clone(),
            })
    }

    fn ensure_parent(&self) -> Result<&Path, StateStorageError> {
        let parent = self.parent()?;
        std::fs::create_dir_all(parent).map_err(|error| {
            StateStorageError::io(&self.path, "create storage directory", &error)
        })?;
        Ok(parent)
    }

    fn acquire_lock(&self, create: bool) -> Result<File, StateStorageError> {
        let lock_path = self.lock_path();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .open(&lock_path)
            .map_err(|error| {
                if !create && error.kind() == std::io::ErrorKind::NotFound {
                    StateStorageError::MissingLock {
                        path: lock_path.clone(),
                    }
                } else {
                    StateStorageError::io(&lock_path, "open transaction lock", &error)
                }
            })?;
        File::lock(&file).map_err(|error| {
            StateStorageError::io(&lock_path, "acquire transaction lock", &error)
        })?;
        Ok(file)
    }

    fn read_current_document(&self) -> Result<CounterDocument, StateStorageError> {
        let mut file = File::open(&self.path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                StateStorageError::Missing {
                    path: self.path.clone(),
                }
            } else {
                StateStorageError::io(&self.path, "read storage", &error)
            }
        })?;
        self.read_current_document_from(&mut file, &self.path)
    }

    fn read_current_document_at(&self, path: &Path) -> Result<CounterDocument, StateStorageError> {
        let mut file = File::open(path)
            .map_err(|error| StateStorageError::io(path, "read recovery document", &error))?;
        self.read_current_document_from(&mut file, path)
    }

    fn read_current_document_from(
        &self,
        reader: &mut File,
        path: &Path,
    ) -> Result<CounterDocument, StateStorageError> {
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|error| StateStorageError::io(path, "read storage", &error))?;
        let value =
            serde_json::from_slice(&bytes).map_err(|error| StateStorageError::InvalidDocument {
                path: path.to_owned(),
                detail: error.to_string(),
            })?;
        let actual = document_version(path, &value)?;
        if actual != CURRENT_SCHEMA_VERSION {
            return Err(StateStorageError::IncompatibleSchema {
                path: path.to_owned(),
                expected: CURRENT_SCHEMA_VERSION,
                actual,
            });
        }
        serde_json::from_value(value).map_err(|error| StateStorageError::InvalidDocument {
            path: path.to_owned(),
            detail: error.to_string(),
        })
    }

    fn owned_migration() -> Result<OwnedMigration, StateStorageError> {
        let migration: OwnedMigration =
            serde_json::from_str(INITIAL_MIGRATION).map_err(|error| {
                StateStorageError::InvalidMigration {
                    detail: error.to_string(),
                }
            })?;
        if migration.version != CURRENT_SCHEMA_VERSION
            || migration.initial_document.schema_version != migration.version
        {
            return Err(StateStorageError::InvalidMigration {
                detail: format!(
                    "artifact version {} does not match its initial document or Module schema {}",
                    migration.version, CURRENT_SCHEMA_VERSION
                ),
            });
        }
        Ok(migration)
    }

    fn read_json_value(&self, path: &Path) -> Result<serde_json::Value, StateStorageError> {
        let bytes = std::fs::read(path)
            .map_err(|error| StateStorageError::io(path, "read storage", &error))?;
        serde_json::from_slice(&bytes).map_err(|error| StateStorageError::InvalidDocument {
            path: path.to_owned(),
            detail: error.to_string(),
        })
    }

    fn write_document(
        &self,
        document: &CounterDocument,
        parent: &Path,
    ) -> Result<(), StateStorageError> {
        let temporary_path = self.temporary_path();
        let bytes = serde_json::to_vec_pretty(document).map_err(|error| {
            StateStorageError::InvalidDocument {
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
                    StateStorageError::RecoveryRequired {
                        path: temporary_path.clone(),
                    }
                } else {
                    StateStorageError::io(&temporary_path, "create transaction", &error)
                }
            })?;
        temporary
            .write_all(&bytes)
            .map_err(|error| StateStorageError::io(&temporary_path, "write transaction", &error))?;
        temporary
            .sync_all()
            .map_err(|error| StateStorageError::io(&temporary_path, "sync transaction", &error))?;
        std::fs::rename(&temporary_path, &self.path)
            .map_err(|error| StateStorageError::io(&self.path, "commit transaction", &error))?;
        sync_directory(parent)
    }

    fn require_recovered(&self) -> Result<(), StateStorageError> {
        let temporary_path = self.temporary_path();
        if temporary_path.exists() {
            Err(StateStorageError::RecoveryRequired {
                path: temporary_path,
            })
        } else {
            Ok(())
        }
    }

    fn probe_parent_writable(&self, parent: &Path) -> Result<(), StateStorageError> {
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
                StateStorageError::io(&probe_path, "probe storage directory", &error)
            })?;
        probe
            .sync_all()
            .map_err(|error| StateStorageError::io(&probe_path, "sync storage probe", &error))?;
        std::fs::remove_file(&probe_path)
            .map_err(|error| StateStorageError::io(&probe_path, "remove storage probe", &error))?;
        sync_directory(parent)
    }

    fn lock_path(&self) -> PathBuf {
        sibling_path(&self.path, ".lock")
    }

    fn temporary_path(&self) -> PathBuf {
        sibling_path(&self.path, ".tmp")
    }
}

pub(super) fn setup_owned_state(
    path: impl Into<PathBuf>,
) -> Result<SetupOutcome, StateStorageError> {
    FileStateAdapter::new(path).setup()
}

pub(super) fn upgrade_owned_state(
    path: impl Into<PathBuf>,
) -> Result<UpgradeOutcome, StateStorageError> {
    FileStateAdapter::new(path).upgrade()
}

pub(super) fn recover_owned_state(
    path: impl Into<PathBuf>,
) -> Result<RecoveryOutcome, StateStorageError> {
    FileStateAdapter::new(path).recover()
}

fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn document_version(path: &Path, value: &serde_json::Value) -> Result<u32, StateStorageError> {
    value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .ok_or_else(|| StateStorageError::InvalidDocument {
            path: path.to_owned(),
            detail: "schema_version is missing or too large".to_owned(),
        })
}

fn sync_directory(parent: &Path) -> Result<(), StateStorageError> {
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| StateStorageError::io(parent, "sync storage directory", &error))
}

/// Reviewable result from the explicit setup workflow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SetupOutcome {
    /// The initial owned migration created the storage document.
    Created { schema_version: u32 },
    /// The document already had the current owned schema.
    AlreadyCurrent { schema_version: u32 },
}

/// Reviewable result from the explicit upgrade workflow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpgradeOutcome {
    /// No migration was required.
    AlreadyCurrent { schema_version: u32 },
    /// One owned migration was applied.
    Applied { from: u32, to: u32 },
}

/// Reviewable result from the explicit recovery workflow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoveryOutcome {
    /// No interrupted transaction existed.
    NoAction,
    /// The committed document was authoritative, so the temporary document was discarded.
    DiscardedUncommitted,
    /// A fully written temporary document was restored after an interrupted rename.
    Restored { schema_version: u32 },
}

/// Failure from the private state Adapter, kept outside the Kernel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateStorageError {
    /// The required durable document does not exist.
    Missing { path: PathBuf },
    /// The stable transaction lock is missing because setup or upgrade has not run.
    MissingLock { path: PathBuf },
    /// The configured path cannot be used as a storage document.
    InvalidPath { path: PathBuf },
    /// The document is malformed or cannot be decoded.
    InvalidDocument { path: PathBuf, detail: String },
    /// The Module's compiled migration artifact is malformed or inconsistent.
    InvalidMigration { detail: String },
    /// The document requires an explicit setup or upgrade decision.
    IncompatibleSchema {
        path: PathBuf,
        expected: u32,
        actual: u32,
    },
    /// An interrupted transaction must be reconciled explicitly.
    RecoveryRequired { path: PathBuf },
    /// The host filesystem rejected one owned operation.
    Io {
        path: PathBuf,
        operation: String,
        detail: String,
    },
}

impl StateStorageError {
    fn io(path: &Path, operation: &str, error: &std::io::Error) -> Self {
        Self::Io {
            path: path.to_owned(),
            operation: operation.to_owned(),
            detail: error.to_string(),
        }
    }
}

impl std::fmt::Display for StateStorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { path } => write!(
                formatter,
                "required durable storage `{}` is missing; run setup",
                path.display()
            ),
            Self::MissingLock { path } => write!(
                formatter,
                "transaction lock `{}` is missing; run the explicit setup or upgrade workflow",
                path.display()
            ),
            Self::InvalidPath { path } => {
                write!(formatter, "storage path `{}` is invalid", path.display())
            }
            Self::InvalidDocument { path, detail } => write!(
                formatter,
                "storage document `{}` is invalid: {detail}",
                path.display()
            ),
            Self::InvalidMigration { detail } => {
                write!(formatter, "owned migration artifact is invalid: {detail}")
            }
            Self::IncompatibleSchema {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "storage document `{}` uses schema {actual}; expected {expected}, run the explicit upgrade workflow",
                path.display()
            ),
            Self::RecoveryRequired { path } => write!(
                formatter,
                "interrupted transaction `{}` requires the explicit recovery workflow",
                path.display()
            ),
            Self::Io {
                path,
                operation,
                detail,
            } => write!(
                formatter,
                "cannot {operation} `{}`: {detail}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for StateStorageError {}

#[cfg(test)]
mod tests;
