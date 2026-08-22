//! A minimal AI agent harness composed only from ordinary Capabilities.

use std::{cell::RefCell, path::Path, rc::Rc};

use lenso_app_plan::{
    AppComposition, CapabilityBinding, CapabilityEndpointPlan, CapabilityRequirementPlan,
    ModuleInstancePlan, ResolvedAppPlan,
};
use lenso_capability_agent::RUN_OPERATION;
use lenso_capability_agent_progress::UPDATE_OPERATION;
use lenso_kernel::RuntimeFailure;
use lenso_native_adapter::{
    NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance, NativeModuleRegistry,
};

mod agent;
mod memory;
mod model;
mod progress;
mod storage;
mod tool;

pub use progress::ProgressFactory;
pub use storage::{MemorySetupOutcome, MemoryStorageError, setup_owned_memory};

/// Public package identity for the harness Module.
pub const AGENT_PACKAGE_ID: &str = "example.agent-harness";
/// Public package identity for the owned durable memory Module.
pub const MEMORY_PACKAGE_ID: &str = "example.agent-memory";
/// Public package identity for the progress subscriber Module.
pub const PROGRESS_PACKAGE_ID: &str = "example.agent-progress-recorder";
/// Package identity for the echo model provider.
pub const ECHO_MODEL_PACKAGE_ID: &str = "example.agent-model-echo";
/// Package identity for the friendly model provider.
pub const FRIENDLY_MODEL_PACKAGE_ID: &str = "example.agent-model-friendly";
/// Package identity for the echo tool provider.
pub const ECHO_TOOL_PACKAGE_ID: &str = "example.agent-tool-echo";
/// Package identity for the uppercase tool provider.
pub const UPPERCASE_TOOL_PACKAGE_ID: &str = "example.agent-tool-uppercase";
const CALLER_PACKAGE_ID: &str = "example.agent-caller";
const MEMORY_KEY: &str = "agent.conversation";
const TOOL_NAME: &str = "lookup";
const MAX_MODEL_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug)]
struct CallerFactory;

impl NativeModuleFactory for CallerFactory {
    fn package_id(&self) -> &'static str {
        CALLER_PACKAGE_ID
    }

    fn instantiate(
        &self,
        _context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        Ok(NativeModuleInstance::default())
    }
}

/// Builds the exact App Composition used by the harness fixture.
#[expect(
    clippy::too_many_lines,
    reason = "the declarative fixture keeps its complete resolved graph visible in one place"
)]
pub fn composition(
    memory_path: impl AsRef<Path>,
    model_package_id: &'static str,
    tool_package_id: &'static str,
    include_progress: bool,
) -> ResolvedAppPlan {
    let agent = ModuleInstancePlan::new("agent", AGENT_PACKAGE_ID)
        .with_configuration(
            serde_json::json!({
                "memory_key": MEMORY_KEY,
                "max_model_output_bytes": MAX_MODEL_OUTPUT_BYTES,
                "tool_name": TOOL_NAME,
            })
            .to_string(),
        )
        .with_capability(CapabilityEndpointPlan::new(
            lenso_capability_agent::CAPABILITY_ID,
            lenso_capability_agent::DESCRIPTOR_VERSION,
            [RUN_OPERATION],
        ))
        .with_requirement(CapabilityRequirementPlan::one(
            lenso_capability_agent_model::CAPABILITY_ID,
            lenso_capability_agent_model::DESCRIPTOR_VERSION,
        ))
        .with_requirement(CapabilityRequirementPlan::one(
            lenso_capability_agent_tool::CAPABILITY_ID,
            lenso_capability_agent_tool::DESCRIPTOR_VERSION,
        ))
        .with_requirement(CapabilityRequirementPlan::one(
            lenso_capability_agent_memory::CAPABILITY_ID,
            lenso_capability_agent_memory::DESCRIPTOR_VERSION,
        ))
        .with_requirement(CapabilityRequirementPlan::many(
            lenso_capability_agent_progress::CAPABILITY_ID,
            lenso_capability_agent_progress::DESCRIPTOR_VERSION,
        ));
    let model = ModuleInstancePlan::new("model", model_package_id).with_capability(
        CapabilityEndpointPlan::new(
            lenso_capability_agent_model::CAPABILITY_ID,
            lenso_capability_agent_model::DESCRIPTOR_VERSION,
            [lenso_capability_agent_model::COMPLETE_OPERATION],
        )
        .with_stream_operation(lenso_capability_agent_model::COMPLETE_OPERATION)
        .with_limits(8, 1),
    );
    let tool = ModuleInstancePlan::new("tool", tool_package_id).with_capability(
        CapabilityEndpointPlan::new(
            lenso_capability_agent_tool::CAPABILITY_ID,
            lenso_capability_agent_tool::DESCRIPTOR_VERSION,
            [lenso_capability_agent_tool::EXECUTE_OPERATION],
        )
        .with_limits(8, 1),
    );
    let memory = ModuleInstancePlan::new("memory", MEMORY_PACKAGE_ID)
        .with_configuration(serde_json::json!({ "storage_path": memory_path.as_ref() }).to_string())
        .with_capability(CapabilityEndpointPlan::new(
            lenso_capability_agent_memory::CAPABILITY_ID,
            lenso_capability_agent_memory::DESCRIPTOR_VERSION,
            [
                lenso_capability_agent_memory::APPEND_OPERATION,
                lenso_capability_agent_memory::READ_OPERATION,
            ],
        ));
    let caller = ModuleInstancePlan::new("caller", CALLER_PACKAGE_ID).with_requirement(
        CapabilityRequirementPlan::one(
            lenso_capability_agent::CAPABILITY_ID,
            lenso_capability_agent::DESCRIPTOR_VERSION,
        ),
    );
    let mut instances = vec![caller, agent, model, tool, memory];
    let mut bindings = vec![
        CapabilityBinding::new(
            "caller",
            lenso_capability_agent::CAPABILITY_ID,
            lenso_capability_agent::DESCRIPTOR_VERSION,
            "agent",
        ),
        CapabilityBinding::new(
            "agent",
            lenso_capability_agent_model::CAPABILITY_ID,
            lenso_capability_agent_model::DESCRIPTOR_VERSION,
            "model",
        ),
        CapabilityBinding::new(
            "agent",
            lenso_capability_agent_tool::CAPABILITY_ID,
            lenso_capability_agent_tool::DESCRIPTOR_VERSION,
            "tool",
        ),
        CapabilityBinding::new(
            "agent",
            lenso_capability_agent_memory::CAPABILITY_ID,
            lenso_capability_agent_memory::DESCRIPTOR_VERSION,
            "memory",
        ),
    ];
    if include_progress {
        instances.push(
            ModuleInstancePlan::new("progress", PROGRESS_PACKAGE_ID).with_capability(
                CapabilityEndpointPlan::new(
                    lenso_capability_agent_progress::CAPABILITY_ID,
                    lenso_capability_agent_progress::DESCRIPTOR_VERSION,
                    [UPDATE_OPERATION],
                )
                .with_event_operation(UPDATE_OPERATION)
                .with_event_capacity(32),
            ),
        );
        bindings.push(CapabilityBinding::new(
            "agent",
            lenso_capability_agent_progress::CAPABILITY_ID,
            lenso_capability_agent_progress::DESCRIPTOR_VERSION,
            "progress",
        ));
    }
    AppComposition::new(instances, bindings)
        .resolve()
        .expect("agent harness Composition should resolve")
}

/// Creates every statically linked provider so Composition alone selects the active ones.
pub fn registry(
    events: Rc<RefCell<Vec<lenso_capability_agent_progress::UpdateRequest>>>,
) -> NativeModuleRegistry {
    NativeModuleRegistry::new()
        .with_factory(CallerFactory)
        .with_factory(agent::AgentFactory)
        .with_factory(memory::MemoryFactory)
        .with_factory(ProgressFactory::new(events))
        .with_factory(model::model_factory(ECHO_MODEL_PACKAGE_ID))
        .with_factory(model::model_factory(FRIENDLY_MODEL_PACKAGE_ID))
        .with_factory(tool::tool_factory(ECHO_TOOL_PACKAGE_ID))
        .with_factory(tool::tool_factory(UPPERCASE_TOOL_PACKAGE_ID))
}
