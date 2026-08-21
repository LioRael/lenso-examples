use std::{cell::RefCell, rc::Rc};

use lenso_capability_agent_progress::{ProgressEndpoint, ProgressProvider, UpdateRequest};
use lenso_kernel::{InvocationContext, NoopModuleLifecycle, RuntimeFailure};
use lenso_native_adapter::{NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance};

use crate::PROGRESS_PACKAGE_ID;

#[derive(Debug)]
struct ProgressRecorder {
    events: Rc<RefCell<Vec<UpdateRequest>>>,
}

impl ProgressProvider for ProgressRecorder {
    fn update(&self, _context: InvocationContext, event: UpdateRequest) {
        self.events.borrow_mut().push(event);
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

impl NativeModuleFactory for ProgressFactory {
    fn package_id(&self) -> &'static str {
        PROGRESS_PACKAGE_ID
    }

    fn instantiate(
        &self,
        _context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        Ok(NativeModuleInstance::with_event_endpoints(
            vec![Rc::new(ProgressEndpoint::new(ProgressRecorder {
                events: self.events.clone(),
            }))],
            NoopModuleLifecycle,
        ))
    }
}
