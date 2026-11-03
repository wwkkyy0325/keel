//! Sub-agent dispatch: a tool delegates a task to another turn with its own
//! context, and only the bounded report comes back.

use std::sync::Arc;

use async_trait::async_trait;
use keel_core::{
    ArgValue, CancelToken, ParamKind, ParamSpec, RiskLevel, TokenUsage, Tool, ToolArgs, ToolCtx,
    ToolError, ToolResult, ToolSchema, TurnState,
};

/// One delegation.
#[derive(Debug, Clone)]
pub struct SubAgentSpec {
    /// Free-form role label recorded in the tool result and audit.
    pub role: String,
    /// The task the child sees. The parent conversation is not forwarded.
    pub task: String,
    pub max_model_calls: u32,
    pub max_tool_calls: u32,
}

/// What the parent receives. The child transcript stays in the child.
#[derive(Debug, Clone)]
pub struct SubAgentOutcome {
    pub state: TurnState,
    pub answer: String,
    pub model_calls: u32,
    pub tool_calls: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Why the child turn failed, when it did.
    pub error: Option<String>,
}

/// Host-provided execution of a child turn.
#[async_trait]
pub trait SubAgentRunner: Send + Sync {
    async fn run(
        &self,
        spec: SubAgentSpec,
        cancel: &CancelToken,
    ) -> Result<SubAgentOutcome, ToolError>;
}

/// Answer text kept from the child report.
const MAX_REPORT_CHARS: usize = 8000;
const DEFAULT_MODEL_CALLS: u32 = 60;
const DEFAULT_TOOL_CALLS: u32 = 120;

/// `spawn_agent`: delegate one task, return one bounded report.
pub struct SpawnAgentTool {
    runner: Arc<dyn SubAgentRunner>,
}

impl SpawnAgentTool {
    pub fn new(runner: Arc<dyn SubAgentRunner>) -> Self {
        Self { runner }
    }
}

#[async_trait]
impl Tool for SpawnAgentTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "spawn_agent".to_owned(),
            description: "Delegate one self-contained task to a sub-agent with its own context. \
                          Only a bounded report and its usage come back; the parent conversation \
                          is not forwarded. Use it for a side investigation, not for work that \
                          needs this conversation's details."
                .to_owned(),
            risk: RiskLevel::Mutating,
            params: vec![
                ParamSpec {
                    name: "task".to_owned(),
                    kind: ParamKind::String,
                    required: true,
                },
                ParamSpec {
                    name: "role".to_owned(),
                    kind: ParamKind::String,
                    required: false,
                },
            ],
        }
    }

    fn permission_requests(
        &self,
        _args: &ToolArgs,
    ) -> Result<Vec<keel_core::PermissionRequest>, ToolError> {
        // The child's own tool calls pass the same gate; the spawn itself only
        // spends tokens, and the parent turn already carries a budget.
        Ok(Vec::new())
    }

    async fn call(&self, args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        let task = match args.require_str("task") {
            Ok(task) => task.to_owned(),
            Err(error) => return ToolResult::failure(ctx.call_id, "spawn_agent", error),
        };
        let role = match args.get("role") {
            Some(ArgValue::String(role)) if !role.trim().is_empty() => role.clone(),
            _ => "sub-agent".to_owned(),
        };
        let spec = SubAgentSpec {
            role: role.clone(),
            task,
            max_model_calls: DEFAULT_MODEL_CALLS,
            max_tool_calls: DEFAULT_TOOL_CALLS,
        };
        match self.runner.run(spec, &ctx.cancel).await {
            Ok(outcome) => {
                let report = clip(&outcome.answer);
                let text = format!(
                    "[sub-agent {role}] state: {}; model calls: {}; tool calls: {}; tokens: {} in / {} out\n{report}",
                    outcome.state,
                    outcome.model_calls,
                    outcome.tool_calls,
                    outcome.input_tokens,
                    outcome.output_tokens
                );
                ToolResult::success(ctx.call_id, "spawn_agent", text).with_usage(TokenUsage {
                    input_tokens: outcome.input_tokens,
                    output_tokens: outcome.output_tokens,
                    cached_input_tokens: 0,
                })
            }
            Err(error) => ToolResult::failure(ctx.call_id, "spawn_agent", error),
        }
    }
}

fn clip(text: &str) -> String {
    if text.chars().count() <= MAX_REPORT_CHARS {
        return text.to_owned();
    }
    let clipped: String = text.chars().take(MAX_REPORT_CHARS).collect();
    format!("{clipped}\n[sub-agent report truncated]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use keel_core::{ToolCallId, TurnId};

    struct FakeRunner {
        fail: bool,
    }

    #[async_trait]
    impl SubAgentRunner for FakeRunner {
        async fn run(
            &self,
            spec: SubAgentSpec,
            _cancel: &CancelToken,
        ) -> Result<SubAgentOutcome, ToolError> {
            if self.fail {
                return Err(ToolError::io("child failed"));
            }
            assert_eq!(spec.task, "do it");
            assert_eq!(spec.role, "writer");
            Ok(SubAgentOutcome {
                state: TurnState::Done,
                answer: "the child report".to_owned(),
                model_calls: 2,
                tool_calls: 3,
                input_tokens: 10,
                output_tokens: 5,
                error: None,
            })
        }
    }

    fn ctx() -> ToolCtx {
        ToolCtx {
            turn_id: TurnId::new(1),
            call_id: ToolCallId::new(1),
            workspace_root: std::env::temp_dir(),
            cancel: CancelToken::new(),
        }
    }

    fn args() -> ToolArgs {
        let mut args = ToolArgs::new();
        args.insert("task", ArgValue::String("do it".to_owned()));
        args.insert("role", ArgValue::String("writer".to_owned()));
        args
    }

    #[tokio::test]
    async fn a_spawn_returns_the_child_report_and_usage() {
        let tool = SpawnAgentTool::new(Arc::new(FakeRunner { fail: false }));
        let result = tool.call(args(), &ctx()).await;

        let text = result.content().unwrap_or_default();
        assert!(text.contains("[sub-agent writer]"), "{text}");
        assert!(text.contains("model calls: 2"), "{text}");
        assert!(text.contains("tool calls: 3"), "{text}");
        assert!(text.contains("the child report"), "{text}");
        assert!(matches!(
            result.usage,
            Some(TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn a_failed_child_is_a_tool_failure() {
        let tool = SpawnAgentTool::new(Arc::new(FakeRunner { fail: true }));
        let result = tool.call(args(), &ctx()).await;

        assert!(!result.is_success());
        assert!(result
            .error()
            .is_some_and(|error| error.message.contains("child failed")));
    }
}
