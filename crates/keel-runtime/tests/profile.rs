mod common;

use std::path::{Path, PathBuf};

use keel_core::{KernelError, PermissionLevel, Policy, RiskLevel, VerificationMode};
use keel_runtime::{from_profile_str, load_profile, validate_config};

const MINIMAL: &str = r#"
[profile]
name = "test"

[model]
provider = "mock"
model = "mock-test"
thinking_budget = 0
max_output_tokens = 64

[context]
stages = ["system", "latest"]
budget_tokens = 128

[tools]
visible = []
budget = 0

[verification]
mode = "none"

[permission]
level = "workspace_auto"

[persona]
system = "system.md"
"#;

fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("system.md"), "system prompt").unwrap();
    dir
}

fn parse(raw: &str) -> Result<keel_runtime::RuntimeConfig, KernelError> {
    let dir = workspace();
    from_profile_str(raw, dir.path())
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn coding_profile_loads() {
    let config = load_profile(&repo_root().join("variants/coding/profile.toml")).unwrap();
    assert_eq!(config.profile_name, "coding");
    assert!(config.visible_tool_names.iter().any(|name| name == "echo"));
    assert_eq!(config.permission_level, PermissionLevel::WorkspaceAuto);
    assert_eq!(
        config.context.stages,
        vec![
            "system".to_owned(),
            "memory".to_owned(),
            "history".to_owned(),
            "latest".to_owned()
        ]
    );
    assert_eq!(config.verification, VerificationMode::None);
}

#[test]
fn openai_profile_loads_and_validates() {
    let config = load_profile(&repo_root().join("variants/coding/profile.openai.toml")).unwrap();
    assert_eq!(config.profile_name, "coding-openai");
    assert_eq!(config.model_route.provider, "openai-compatible");
    assert_eq!(config.model_route.model, "gpt-4o-mini");
    assert_eq!(
        config.model_base_url.as_deref(),
        Some("https://api.openai.com/v1")
    );
    assert_eq!(config.api_key_env.as_deref(), Some("OPENAI_API_KEY"));
    assert!(!config.supports_reasoning);
    validate_config(
        &config,
        &common::standard_registry(&repo_root()),
        &common::standard_context(),
    )
    .unwrap();
}

#[test]
fn deepseek_profile_loads_and_validates() {
    let config = load_profile(&repo_root().join("variants/coding/profile.deepseek.toml")).unwrap();
    assert_eq!(config.profile_name, "coding-deepseek");
    assert_eq!(config.model_route.provider, "openai-compatible");
    assert_eq!(config.model_route.model, "deepseek-flash");
    assert_eq!(
        config.model_base_url.as_deref(),
        Some("https://api.deepseek.com")
    );
    assert_eq!(config.api_key_env.as_deref(), Some("DEEPSEEK_API_KEY"));
    assert!(config.supports_reasoning);
    validate_config(
        &config,
        &common::standard_registry(&repo_root()),
        &common::standard_context(),
    )
    .unwrap();
}

#[test]
fn anthropic_profile_loads_and_validates() {
    let config = load_profile(&repo_root().join("variants/coding/profile.anthropic.toml")).unwrap();
    assert_eq!(config.profile_name, "coding-anthropic");
    assert_eq!(config.model_route.provider, "anthropic-compatible");
    assert_eq!(config.model_route.model, "claude-sonnet-5-5");
    assert_eq!(
        config.model_base_url.as_deref(),
        Some("https://api.anthropic.com")
    );
    assert_eq!(config.api_key_env.as_deref(), Some("ANTHROPIC_API_KEY"));
    assert_eq!(config.api_key_keyring.as_deref(), Some("keel:anthropic"));
    validate_config(
        &config,
        &common::standard_registry(&repo_root()),
        &common::standard_context(),
    )
    .unwrap();
}

#[test]
fn companion_profile_loads() {
    let config = load_profile(&repo_root().join("variants/companion/profile.toml")).unwrap();
    assert_eq!(config.profile_name, "companion");
    assert!(config.visible_tool_names.is_empty());
    assert_eq!(config.permission_level, PermissionLevel::WorkspaceAuto);
    assert_eq!(config.tool_budget, 0);
}

#[test]
fn both_variants_validate_against_the_standard_plugins() {
    let registry = common::standard_registry(&repo_root());
    let context = common::standard_context();
    for name in ["coding", "companion"] {
        let config =
            load_profile(&repo_root().join(format!("variants/{name}/profile.toml"))).unwrap();
        validate_config(&config, &registry, &context).unwrap();
    }
}

#[test]
fn unknown_fields_are_rejected() {
    let raw = format!("{MINIMAL}\n[extra]\nvalue = 1\n");
    assert!(parse(&raw).is_err());
}

#[test]
fn profile_inheritance_is_rejected_in_m1() {
    let raw = MINIMAL.replace("name = \"test\"", "name = \"test\"\nextends = \"default\"");
    assert!(parse(&raw).is_err());
}

#[test]
fn unknown_permission_level_is_rejected() {
    let raw = MINIMAL.replace("level = \"workspace_auto\"", "level = \"root\"");
    assert!(parse(&raw).is_err());
}

#[test]
fn removed_permission_levels_are_rejected() {
    for legacy in ["read_only", "manual", "semi_auto"] {
        let raw = MINIMAL.replace(
            "level = \"workspace_auto\"",
            &format!("level = \"{legacy}\""),
        );
        assert!(parse(&raw).is_err(), "{legacy} must be rejected");
    }
}

#[test]
fn unknown_compaction_step_is_rejected() {
    let raw = MINIMAL.replace(
        "budget_tokens = 128",
        "budget_tokens = 128\ncompaction = [\"teleport\"]",
    );
    assert!(parse(&raw).is_err());
}

#[test]
fn visible_tools_must_be_registered() {
    let raw = MINIMAL.replace("visible = []", "visible = [\"missing\"]");
    let config = parse(&raw).unwrap();
    let error = validate_config(
        &config,
        &common::standard_registry(&repo_root()),
        &common::standard_context(),
    )
    .unwrap_err();
    assert!(matches!(error, KernelError::InvalidConfig { .. }));
}

#[test]
fn context_stages_must_be_registered() {
    let raw = MINIMAL.replace(
        "stages = [\"system\", \"latest\"]",
        "stages = [\"system\", \"missing\"]",
    );
    let config = parse(&raw).unwrap();
    let error = validate_config(
        &config,
        &common::standard_registry(&repo_root()),
        &common::standard_context(),
    )
    .unwrap_err();
    assert!(matches!(error, KernelError::InvalidConfig { .. }));
}

#[test]
fn tests_verification_requires_a_test_tool() {
    let raw = MINIMAL.replace("mode = \"none\"", "mode = \"tests\"");
    let config = parse(&raw).unwrap();
    let error = validate_config(
        &config,
        &common::standard_registry(&repo_root()),
        &common::standard_context(),
    )
    .unwrap_err();
    assert!(matches!(error, KernelError::InvalidConfig { .. }));
}

#[test]
fn plan_mode_hides_mutating_tools() {
    let registry = common::standard_registry(&repo_root());
    let mut config = common::runtime_config(
        &["echo", "read_file", "write_file"],
        4,
        PermissionLevel::WorkspaceAuto,
    );
    config.plan_mode = true;

    let visible = config.visible_tools(&registry.schemas());
    assert!(visible
        .iter()
        .all(|schema| schema.risk == RiskLevel::ReadOnly));
    assert!(visible.iter().any(|schema| schema.name == "echo"));
    assert!(!visible.iter().any(|schema| schema.name == "write_file"));
}

#[test]
fn plan_mode_profile_flag_parses() {
    let raw = MINIMAL.replace(
        "[tools]\nvisible = []\nbudget = 0",
        "[tools]\nvisible = []\nbudget = 0\nplan_mode = true",
    );
    let config = parse(&raw).unwrap();
    assert!(config.plan_mode);
}

#[test]
fn missing_persona_prompt_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let error = from_profile_str(MINIMAL, dir.path()).unwrap_err();
    assert!(matches!(error, KernelError::InvalidProfile { .. }));
}

#[test]
fn profile_relative_prompt_path_is_resolved() {
    let dir = workspace();
    std::fs::create_dir_all(dir.path().join("prompts")).unwrap();
    std::fs::write(dir.path().join("prompts/style.md"), "style prompt").unwrap();
    let raw = MINIMAL.replace("system = \"system.md\"", "system = \"prompts/style.md\"");
    let config = from_profile_str(&raw, dir.path()).unwrap();
    assert_eq!(config.system_prompt, "style prompt");
}

#[test]
fn absolute_prompt_path_is_resolved() {
    let dir = workspace();
    let prompt: &Path = &dir.path().join("system.md");
    let raw = MINIMAL.replace(
        "system = \"system.md\"",
        &format!("system = '{}'", prompt.display()),
    );
    let config = from_profile_str(&raw, Path::new("")).unwrap();
    assert_eq!(config.system_prompt, "system prompt");
}

#[test]
fn a_window_below_the_supported_floor_is_rejected() {
    let raw = MINIMAL.replace(
        "max_output_tokens = 64",
        "max_output_tokens = 64\ncontext_window = 4096",
    );
    assert!(parse(&raw).is_err());
}

#[test]
fn a_budget_that_does_not_fit_the_window_is_rejected() {
    let raw = MINIMAL
        .replace(
            "max_output_tokens = 64",
            "max_output_tokens = 64\ncontext_window = 16384",
        )
        .replace("budget_tokens = 128", "budget_tokens = 16384");
    assert!(parse(&raw).is_err());
}

#[test]
fn a_profile_with_a_valid_window_loads() {
    let raw = MINIMAL.replace(
        "max_output_tokens = 64",
        "max_output_tokens = 64\ncontext_window = 16384",
    );
    assert!(parse(&raw).is_ok());
}
