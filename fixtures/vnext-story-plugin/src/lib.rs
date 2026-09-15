//! A durable Story Plugin fixture sourced from explicit business Events.

use std::{collections::BTreeSet, path::PathBuf, rc::Rc};

use lenso_capability_story_events::{
    Events, EventsEndpoint, EventsProvider, RecordError, RecordRequest,
};
use lenso_capability_story_query::{
    Query, QueryEndpoint, QueryProvider, TimelineError, TimelineRequest, TimelineResponse,
    TimelineResponseEntriesItem,
};
use lenso_kernel::{
    InvocationContext, NativeRequestEndpoint, NativeRequestFuture, PluginFuture, PluginLifecycle,
    PrepareContext, RuntimeFailure,
};
use lenso_native_adapter::{NativePluginFactory, NativePluginFactoryContext, NativePluginInstance};
use serde::Deserialize;

mod storage;

use storage::FileStoryAdapter;

/// Package identity for the optional Story Plugin fixture.
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

impl PluginLifecycle for StoryLifecycle {
    fn prepare(&self, _context: PrepareContext) -> PluginFuture {
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
    ) -> NativeRequestFuture<Events> {
        let runtime = Rc::clone(&self.runtime);
        let source_instance = context.caller_instance().unwrap_or("unknown").to_owned();
        Box::pin(async move {
            match runtime.storage.ingest(
                &event,
                &source_instance,
                runtime.retention_limit,
                runtime.idempotency_limit,
            ) {
                Ok(_) => Ok(Ok(())),
                Err(StoryStorageError::InvalidEvent { .. }) => Ok(Err(RecordError::InvalidEvent)),
                Err(StoryStorageError::ConflictingEventId { .. }) => {
                    Ok(Err(RecordError::ConflictingEventId))
                }
                Err(error) => Err(RuntimeFailure::Internal {
                    detail: error.to_string(),
                }),
            }
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
    ) -> NativeRequestFuture<Query> {
        let runtime = Rc::clone(&self.runtime);
        Box::pin(async move {
            let caller = context.caller_instance().unwrap_or_default();
            if !runtime.authorized_callers.contains(caller) {
                return Ok(Err(TimelineError::Unauthorized));
            }
            if request.subject_id.is_empty() || !(1..=100).contains(&request.limit) {
                return Ok(Err(TimelineError::InvalidQuery));
            }
            let limit =
                usize::try_from(request.limit).map_err(|_| RuntimeFailure::PluginFailure {
                    detail: "validated Story query limit cannot fit this platform".to_owned(),
                })?;
            let entries = runtime
                .storage
                .timeline(&request.subject_id, limit)
                .map_err(|error| RuntimeFailure::Internal {
                    detail: error.to_string(),
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
            Ok(Ok(TimelineResponse { entries }))
        })
    }
}

/// Statically linked factory for the optional durable Story Plugin.
#[derive(Debug)]
pub struct StoryFactory;

impl NativePluginFactory for StoryFactory {
    fn package_id(&self) -> &'static str {
        STORY_PACKAGE_ID
    }

    fn instantiate(
        &self,
        context: NativePluginFactoryContext<'_>,
    ) -> Result<NativePluginInstance, RuntimeFailure> {
        let configuration: StoryConfiguration = serde_json::from_str(context.configuration())
            .map_err(|error| RuntimeFailure::InvalidResolvedPlan {
                detail: format!("Story Plugin configuration is invalid: {error}"),
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
                detail: "Story Plugin requires storage_path, positive retention_limit, idempotency_limit >= retention_limit, and non-empty authorized_callers"
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
        Ok(NativePluginInstance::with_lifecycle(
            vec![query_endpoint, event_endpoint],
            StoryLifecycle { runtime },
        ))
    }
}
