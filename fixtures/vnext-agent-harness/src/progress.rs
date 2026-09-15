use std::{cell::RefCell, rc::Rc};

use futures::future::LocalBoxFuture;
use lenso_capability_agent_progress::{ProgressEndpoint, ProgressProvider, UpdateRequest};
use lenso_kernel::{InvocationContext, NoopPluginLifecycle, RuntimeFailure};
use lenso_native_adapter::{NativePluginFactory, NativePluginFactoryContext, NativePluginInstance};

use crate::PROGRESS_PACKAGE_ID;

#[derive(Debug)]
struct ProgressRecorder {
    events: Rc<RefCell<Vec<UpdateRequest>>>,
}

impl ProgressProvider for ProgressRecorder {
    fn update(
        &self,
        _context: InvocationContext,
        event: UpdateRequest,
    ) -> LocalBoxFuture<'static, Result<(), RuntimeFailure>> {
        self.events.borrow_mut().push(event);
        Box::pin(async { Ok(()) })
    }
}

#[derive(Clone, Debug)]
pub struct ProgressFactory {
    events: Rc<RefCell<Vec<UpdateRequest>>>,
}

impl ProgressFactory {
    pub fn new(events: Rc<RefCell<Vec<UpdateRequest>>>) -> Self {
        Self { events }
    }
}

impl NativePluginFactory for ProgressFactory {
    fn package_id(&self) -> &'static str {
        PROGRESS_PACKAGE_ID
    }

    fn instantiate(
        &self,
        _context: NativePluginFactoryContext<'_>,
    ) -> Result<NativePluginInstance, RuntimeFailure> {
        Ok(NativePluginInstance::with_event_endpoints(
            vec![Rc::new(ProgressEndpoint::new(ProgressRecorder {
                events: self.events.clone(),
            }))],
            NoopPluginLifecycle,
        ))
    }
}
