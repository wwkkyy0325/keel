use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use keel_core::{
    CompactionStep, ContextRecipe, InteractionPolicy, KernelError, MemoryMode, MemoryPolicy,
    ModelRoute, OnTimeout, PermissionLevel, PromotionMode, VerificationMode,
};
use serde::Deserialize;

use crate::context::ContextBuilder;
use crate::policy::RuntimeConfig;
use crate::registry::ToolRegistry;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileFile {
    profile: ProfileSection,
    model: ModelSection,
    context: ContextSection,
    tools: ToolsSection,
    verification: VerificationSection,
    permission: PermissionSection,
    persona: PersonaSection,
    #[serde(default)]
    interaction: InteractionSection,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileSection {
    name: String,
    #[serde(default)]
    extends: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelSection {
    provider: String,
    model: String,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    api_key_env: Option<String>,
    thinking_budget: u32,
    #[serde(default = "default_max_output_tokens")]
    max_output_tokens: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextSection {
    stages: Vec<String>,
    budget_tokens: u32,
    #[serde(default)]
    compaction: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolsSection {
    visible: Vec<String>,
    budget: u32,
    #[serde(default)]
    plan_mode: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerificationSection {
    mode: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PermissionSection {
    level: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonaSection {
    system: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InteractionSection {
    #[serde(default = "default_interaction_timeout_ms")]
    timeout_ms: u64,
    #[serde(default = "default_on_timeout")]
    on_timeout: String,
    #[serde(default = "default_max_interactions")]
    max_per_turn: u32,
}

impl Default for InteractionSection {
    fn default() -> Self {
        Self {
            timeout_ms: default_interaction_timeout_ms(),
            on_timeout: default_on_timeout(),
            max_per_turn: default_max_interactions(),
        }
    }
}

fn default_max_output_tokens() -> u32 {
    4096
}

fn default_interaction_timeout_ms() -> u64 {
    300_000
}

fn default_on_timeout() -> String {
    "defer_and_continue".to_owned()
}

fn default_max_interactions() -> u32 {
    4
}

/// Loads a profile file and resolves its persona prompt relative to the
/// profile directory.
pub fn load_profile(path: &Path) -> Result<RuntimeConfig, KernelError> {
    let raw = fs::read_to_string(path).map_err(|error| KernelError::InvalidProfile {
        message: format!("cannot read profile `{}`: {error}", path.display()),
    })?;
    let base_dir = path.parent().unwrap_or_else(|| Path::new(""));
    from_profile_str(&raw, base_dir)
}

/// Resolves a minimal M1 profile. Unknown TOML fields are errors.
pub fn from_profile_str(raw: &str, base_dir: &Path) -> Result<RuntimeConfig, KernelError> {
    let file: ProfileFile = toml::from_str(raw).map_err(|error| KernelError::InvalidProfile {
        message: error.to_string(),
    })?;

    if file.profile.extends.is_some() {
        return Err(KernelError::InvalidProfile {
            message: "profile inheritance is not supported in M1".to_owned(),
        });
    }

    let mut seen_stages = BTreeSet::new();
    for stage in &file.context.stages {
        if !seen_stages.insert(stage.clone()) {
            return Err(KernelError::InvalidProfile {
                message: format!("duplicate context stage `{stage}`"),
            });
        }
    }
    if file.context.stages.is_empty() {
        return Err(KernelError::InvalidProfile {
            message: "context stages must not be empty".to_owned(),
        });
    }
    if file.context.budget_tokens == 0 {
        return Err(KernelError::InvalidProfile {
            message: "context budget must be positive".to_owned(),
        });
    }
    if file.model.provider.is_empty() || file.model.model.is_empty() {
        return Err(KernelError::InvalidProfile {
            message: "model provider and model must not be empty".to_owned(),
        });
    }

    let permission_level = parse_permission_level(&file.permission.level)?;
    let verification = parse_verification_mode(&file.verification.mode)?;
    let compaction = parse_compaction(&file.context.compaction)?;
    let interaction = parse_interaction(&file.interaction)?;

    let prompt_path = base_dir.join(&file.persona.system);
    let system_prompt =
        fs::read_to_string(&prompt_path).map_err(|error| KernelError::InvalidProfile {
            message: format!(
                "cannot read persona prompt `{}`: {error}",
                prompt_path.display()
            ),
        })?;

    Ok(RuntimeConfig {
        profile_name: file.profile.name,
        system_prompt,
        visible_tool_names: file.tools.visible,
        plan_mode: file.tools.plan_mode,
        tool_budget: file.tools.budget,
        context: ContextRecipe {
            stages: file.context.stages,
            budget_tokens: file.context.budget_tokens,
            compaction,
        },
        thinking_budget: file.model.thinking_budget,
        verification,
        memory: MemoryPolicy {
            mode: MemoryMode::Off,
            promotion: PromotionMode::Strict,
            recall_limit: 8,
        },
        permission_level,
        model_route: ModelRoute {
            provider: file.model.provider,
            model: file.model.model,
        },
        max_output_tokens: file.model.max_output_tokens,
        model_base_url: file.model.base_url,
        api_key_env: file.model.api_key_env,
        interaction,
    })
}

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

fn parse_permission_level(raw: &str) -> Result<PermissionLevel, KernelError> {
    match raw {
        "workspace_auto" => Ok(PermissionLevel::WorkspaceAuto),
        "full_auto" => Ok(PermissionLevel::FullAuto),
        other => Err(KernelError::InvalidProfile {
            message: format!("unknown permission level `{other}`"),
        }),
    }
}

fn parse_verification_mode(raw: &str) -> Result<VerificationMode, KernelError> {
    match raw {
        "none" => Ok(VerificationMode::None),
        "evidence" => Ok(VerificationMode::Evidence),
        "tests" => Ok(VerificationMode::Tests),
        "custom" => Ok(VerificationMode::Custom("custom".to_owned())),
        other => Err(KernelError::InvalidProfile {
            message: format!("unknown verification mode `{other}`"),
        }),
    }
}

fn parse_interaction(section: &InteractionSection) -> Result<InteractionPolicy, KernelError> {
    let on_timeout = match section.on_timeout.as_str() {
        "block" => OnTimeout::Block,
        "defer_and_continue" => OnTimeout::DeferAndContinue,
        "end_turn" => OnTimeout::EndTurn,
        "fail" => OnTimeout::Fail,
        other => {
            return Err(KernelError::InvalidProfile {
                message: format!("unknown interaction timeout policy `{other}`"),
            });
        }
    };
    Ok(InteractionPolicy {
        timeout: std::time::Duration::from_millis(section.timeout_ms),
        on_timeout,
        max_per_turn: section.max_per_turn,
    })
}

fn parse_compaction(raw: &[String]) -> Result<Vec<CompactionStep>, KernelError> {
    raw.iter()
        .map(|step| match step.as_str() {
            "trim" => Ok(CompactionStep::Trim),
            "rolling_summary" => Ok(CompactionStep::RollingSummary),
            "llm_compact" => Ok(CompactionStep::LlmCompact),
            "task_boundary" => Ok(CompactionStep::TaskBoundary),
            other => Err(KernelError::InvalidProfile {
                message: format!("unknown compaction step `{other}`"),
            }),
        })
        .collect()
}
