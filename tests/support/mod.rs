use std::rc::Rc;

use lenso_capability_story_events::{Events, RECORD_OPERATION, RecordRequest};
use lenso_kernel::{ActivateContext, ModuleFuture, ModuleLifecycle, RuntimeFailure};
use lenso_native_adapter::{NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance};
use serde::Deserialize;

pub const PRODUCER_PACKAGE_ID: &str = "fixture.story-producer";
pub const READER_PACKAGE_ID: &str = "fixture.story-reader";
pub const DENIED_PACKAGE_ID: &str = "fixture.story-denied";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProducerConfiguration {
    events: Vec<RecordRequest>,
}

#[derive(Debug)]
struct ProducerLifecycle {
    events: Vec<RecordRequest>,
}

impl ModuleLifecycle for ProducerLifecycle {
    fn activate(&self, context: ActivateContext) -> ModuleFuture {
        let events = self.events.clone();
        let dependencies = context.dependencies().clone();
        Box::pin(async move {
            let handles = dependencies.many::<Events>()?;
            for event in events {
                for handle in &handles {
                    handle
                        .invoke(RECORD_OPERATION, event.clone())
                        .await?
                        .map_err(|error| RuntimeFailure::ModuleFailure {
                            detail: format!("Story rejected producer Event: {error:?}"),
                        })?;
                }
            }
            Ok(())
        })
    }
}

#[derive(Debug)]
pub struct ProducerFactory;

impl NativeModuleFactory for ProducerFactory {
    fn package_id(&self) -> &'static str {
        PRODUCER_PACKAGE_ID
    }

    fn instantiate(
        &self,
        context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        let configuration: ProducerConfiguration = serde_json::from_str(context.configuration())
            .map_err(|error| RuntimeFailure::InvalidResolvedPlan {
                detail: format!("Story producer configuration is invalid: {error}"),
            })?;
        Ok(NativeModuleInstance::with_lifecycle(
            Vec::<Rc<dyn lenso_kernel::NativeRequestEndpoint>>::new(),
            ProducerLifecycle {
                events: configuration.events,
            },
        ))
    }
}

#[derive(Debug)]
pub struct NoopFactory {
    package_id: &'static str,
}

impl NoopFactory {
    pub fn new(package_id: &'static str) -> Self {
        Self { package_id }
    }
}

impl NativeModuleFactory for NoopFactory {
    fn package_id(&self) -> &'static str {
        self.package_id
    }

    fn instantiate(
        &self,
        _context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        Ok(NativeModuleInstance::new(Vec::<
            Rc<dyn lenso_kernel::NativeRequestEndpoint>,
        >::new()))
    }
}
