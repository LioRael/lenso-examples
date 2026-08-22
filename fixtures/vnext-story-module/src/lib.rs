//! A durable Story Module fixture sourced from explicit business Events.

use std::{collections::BTreeSet, path::PathBuf, rc::Rc};

use futures::future::LocalBoxFuture;
use lenso_capability_story_events::{
    EventsEndpoint, EventsInvocationError, EventsProvider, RecordError, RecordRequest,
};
use lenso_capability_story_query::{
    QueryEndpoint, QueryInvocationError, QueryProvider, TimelineError, TimelineRequest,
    TimelineResponse, TimelineResponseEntriesItem,
};
use lenso_kernel::{
    InvocationContext, ModuleFuture, ModuleLifecycle, NativeRequestEndpoint, PrepareContext,
    RuntimeFailure,
};
use lenso_native_adapter::{NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance};
use serde::Deserialize;

mod storage;

use storage::FileStoryAdapter;

/// Package identity for the optional Story Module fixture.
pub const STORY_PACKAGE_ID: &str = "lenso.fixture-story";
/// Current private Story storage schema.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;
/// The owner-applied initial Story migration.
pub const INITIAL_MIGRATION: &str = include_str!("../migrations/001-story-v1.json");

pub use storage::{
    StoryRecoveryOutcome, StorySetupOutcome, StoryStorageError, StoryUpgradeOutcome,
};

/// Applies the Story owner's initial storage migration.
pub fn setup_owned_story(path: impl Into<PathBuf>) -> Result<StorySetupOutcome, StoryStorageError> {
    FileStoryAdapter::new(path).setup()
}

/// Applies an explicit Story storage upgrade.
pub fn upgrade_owned_story(
    path: impl Into<PathBuf>,
) -> Result<StoryUpgradeOutcome, StoryStorageError> {
    FileStoryAdapter::new(path).upgrade()
}

/// Recovers an interrupted Story storage transaction.
pub fn recover_owned_story(
    path: impl Into<PathBuf>,
) -> Result<StoryRecoveryOutcome, StoryStorageError> {
    FileStoryAdapter::new(path).recover()
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoryConfiguration {
    storage_path: PathBuf,
    authorized_callers: Vec<String>,
    retention_limit: usize,
    idempotency_limit: usize,
}

#[derive(Debug)]
struct StoryRuntime {
    storage: FileStoryAdapter,
    authorized_callers: BTreeSet<String>,
    retention_limit: usize,
    idempotency_limit: usize,
}

#[derive(Debug)]
struct StoryLifecycle {
    runtime: Rc<StoryRuntime>,
}

impl ModuleLifecycle for StoryLifecycle {
    fn prepare(&self, _context: PrepareContext) -> ModuleFuture {
        let runtime = Rc::clone(&self.runtime);
        Box::pin(async move {
            runtime
                .storage
                .verify_ready()
                .map_err(|error| RuntimeFailure::Internal {
                    detail: error.to_string(),
                })
        })
    }
}

#[derive(Debug)]
struct StoryEventsProvider {
    runtime: Rc<StoryRuntime>,
}

impl EventsProvider for StoryEventsProvider {
    fn record(
        &self,
        context: InvocationContext,
        event: RecordRequest,
    ) -> LocalBoxFuture<'static, Result<(), EventsInvocationError>> {
        let runtime = Rc::clone(&self.runtime);
        let source_instance = context.caller_instance().unwrap_or("unknown").to_owned();
        Box::pin(async move {
            runtime
                .storage
                .ingest(
                    &event,
                    &source_instance,
                    runtime.retention_limit,
                    runtime.idempotency_limit,
                )
                .map(|_| ())
                .map_err(|error| match error {
                    StoryStorageError::InvalidEvent { .. } => {
                        EventsInvocationError::Domain(RecordError::InvalidEvent)
                    }
                    StoryStorageError::ConflictingEventId { .. } => {
                        EventsInvocationError::Domain(RecordError::ConflictingEventId)
                    }
                    error => EventsInvocationError::Runtime(RuntimeFailure::Internal {
                        detail: error.to_string(),
                    }),
                })
        })
    }
}

#[derive(Debug)]
struct StoryQueryProvider {
    runtime: Rc<StoryRuntime>,
}

impl QueryProvider for StoryQueryProvider {
    fn timeline(
        &self,
        context: InvocationContext,
        request: TimelineRequest,
    ) -> LocalBoxFuture<'static, Result<TimelineResponse, QueryInvocationError>> {
        let runtime = Rc::clone(&self.runtime);
        Box::pin(async move {
            let caller = context.caller_instance().unwrap_or_default();
            if !runtime.authorized_callers.contains(caller) {
                return Err(QueryInvocationError::Domain(TimelineError::Unauthorized));
            }
            if request.subject_id.is_empty() || !(1..=100).contains(&request.limit) {
                return Err(QueryInvocationError::Domain(TimelineError::InvalidQuery));
            }
            let entries = runtime
                .storage
                .timeline(&request.subject_id, request.limit as usize)
                .map_err(|error| {
                    QueryInvocationError::Runtime(RuntimeFailure::Internal {
                        detail: error.to_string(),
                    })
                })?
                .into_iter()
                .map(|entry| TimelineResponseEntriesItem {
                    event_id: entry.event_id,
                    event_version: entry.event_version,
                    occurred_at: entry.occurred_at,
                    subject_id: entry.subject_id,
                    event_type: entry.event_type,
                    facts: entry.facts,
                    source_instance: entry.source_instance,
                    revision: entry.revision.to_string(),
                })
                .collect();
            Ok(TimelineResponse { entries })
        })
    }
}

/// Statically linked factory for the optional durable Story Module.
#[derive(Debug)]
pub struct StoryFactory;

impl NativeModuleFactory for StoryFactory {
    fn package_id(&self) -> &'static str {
        STORY_PACKAGE_ID
    }

    fn instantiate(
        &self,
        context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        let configuration: StoryConfiguration = serde_json::from_str(context.configuration())
            .map_err(|error| RuntimeFailure::InvalidResolvedPlan {
                detail: format!("Story Module configuration is invalid: {error}"),
            })?;
        if configuration.storage_path.as_os_str().is_empty()
            || configuration.retention_limit == 0
            || configuration.idempotency_limit < configuration.retention_limit
            || configuration
                .authorized_callers
                .iter()
                .any(String::is_empty)
        {
            return Err(RuntimeFailure::InvalidResolvedPlan {
                detail: "Story Module requires storage_path, positive retention_limit, idempotency_limit >= retention_limit, and non-empty authorized_callers"
                    .to_owned(),
            });
        }
        let runtime = Rc::new(StoryRuntime {
            storage: FileStoryAdapter::new(configuration.storage_path),
            authorized_callers: configuration.authorized_callers.into_iter().collect(),
            retention_limit: configuration.retention_limit,
            idempotency_limit: configuration.idempotency_limit,
        });
        let query_endpoint: Rc<dyn NativeRequestEndpoint> =
            Rc::new(QueryEndpoint::new(StoryQueryProvider {
                runtime: Rc::clone(&runtime),
            }));
        let event_endpoint: Rc<dyn NativeRequestEndpoint> =
            Rc::new(EventsEndpoint::new(StoryEventsProvider {
                runtime: Rc::clone(&runtime),
            }));
        Ok(NativeModuleInstance::with_lifecycle(
            vec![query_endpoint, event_endpoint],
            StoryLifecycle { runtime },
        ))
    }
}
