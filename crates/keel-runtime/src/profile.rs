use keel_core::{KernelError, VerificationMode};

use crate::context::ContextBuilder;
use crate::registry::ToolRegistry;
use crate::RuntimeConfig;

pub use keel_policy::{from_profile_str, load_profile};

/// Checks that a resolved profile only references capabilities the host
/// installed.
pub fn validate_config(
    config: &RuntimeConfig,
    tools: &ToolRegistry,
    context: &ContextBuilder,
) -> Result<(), KernelError> {
    let registered_tools = tools.names();
    for name in &config.visible_tool_names {
        if !registered_tools.iter().any(|candidate| candidate == name) {
            return Err(KernelError::InvalidConfig {
                message: format!("visible tool `{name}` is not registered"),
            });
        }
    }

    let registered_stages = context.stage_names();
    for stage in &config.context.stages {
        if !registered_stages.contains(&stage.as_str()) {
            return Err(KernelError::InvalidConfig {
                message: format!("context stage `{stage}` is not registered"),
            });
        }
    }

    if config.verification == VerificationMode::Tests
        && !registered_tools.iter().any(|name| name == "test")
    {
        return Err(KernelError::InvalidConfig {
            message: "verification mode `tests` requires a `test` tool".to_owned(),
        });
    }

    Ok(())
}
