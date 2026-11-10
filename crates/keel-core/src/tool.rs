use std::collections::BTreeMap;
use std::path::PathBuf;

use async_trait::async_trait;
use serde::Serialize;

use crate::cancel::CancelToken;
use crate::context::estimate_tokens;
use crate::error::ToolError;
use crate::ids::{ToolCallId, TurnId};
use crate::model::TokenUsage;
use crate::permission::PermissionRequest;

/// How risky a tool call is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    ReadOnly,
    Mutating,
    Dangerous,
}

impl RiskLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Mutating => "mutating",
            Self::Dangerous => "dangerous",
        }
    }
}

/// Type of one tool parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    String,
    Integer,
    Boolean,
    StringList,
}

impl ParamKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Boolean => "boolean",
            Self::StringList => "string_list",
        }
    }
}

/// Declared parameter of a tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamSpec {
    pub name: String,
    pub kind: ParamKind,
    pub required: bool,
}

/// Typed tool schema announced to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub risk: RiskLevel,
    pub params: Vec<ParamSpec>,
}

impl ToolSchema {
    /// Validates arguments against this schema.
    ///
    /// Unknown arguments are rejected: a typo must not silently change tool
    /// behavior.
    pub fn validate(&self, args: &ToolArgs) -> Result<(), ToolError> {
        for param in &self.params {
            match args.values.get(&param.name) {
                Some(value) if value.kind() != param.kind => {
                    return Err(ToolError::invalid_arguments(format!(
                        "argument `{}` must be {}, got {}",
                        param.name,
                        param.kind.as_str(),
                        value.kind().as_str()
                    )));
                }
                None if param.required => {
                    return Err(ToolError::invalid_arguments(format!(
                        "missing required argument `{}`",
                        param.name
                    )));
                }
                _ => {}
            }
        }
        for name in args.values.keys() {
            if !self.params.iter().any(|param| &param.name == name) {
                // Naming the accepted arguments turns the model's next call
                // into a fix instead of a guess.
                let accepted: Vec<&str> = self
                    .params
                    .iter()
                    .map(|param| param.name.as_str())
                    .collect();
                return Err(ToolError::invalid_arguments(format!(
                    "unknown argument `{name}`; accepted: {}",
                    accepted.join(", ")
                )));
            }
        }
        Ok(())
    }
}

/// One typed tool argument value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgValue {
    String(String),
    Integer(i64),
    Boolean(bool),
    StringList(Vec<String>),
}

impl ArgValue {
    pub fn kind(&self) -> ParamKind {
        match self {
            Self::String(_) => ParamKind::String,
            Self::Integer(_) => ParamKind::Integer,
            Self::Boolean(_) => ParamKind::Boolean,
            Self::StringList(_) => ParamKind::StringList,
        }
    }

    pub fn estimated_tokens(&self) -> u32 {
        match self {
            Self::String(value) => estimate_tokens(value),
            Self::Integer(_) | Self::Boolean(_) => 1,
            Self::StringList(values) => values.iter().fold(0u32, |total, value| {
                total.saturating_add(estimate_tokens(value))
            }),
        }
    }
}

/// Typed arguments of one tool call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolArgs {
    values: BTreeMap<String, ArgValue>,
}

impl ToolArgs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: impl Into<String>, value: ArgValue) {
        self.values.insert(name.into(), value);
    }

    pub fn get(&self, name: &str) -> Option<&ArgValue> {
        self.values.get(name)
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.values.keys()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn require_str(&self, name: &str) -> Result<&str, ToolError> {
        match self.values.get(name) {
            Some(ArgValue::String(value)) => Ok(value),
            Some(other) => Err(ToolError::invalid_arguments(format!(
                "argument `{name}` must be string, got {}",
                other.kind().as_str()
            ))),
            None => Err(ToolError::invalid_arguments(format!(
                "missing required argument `{name}`"
            ))),
        }
    }
}

/// Outcome of a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolOutcome {
    Success { content: String },
    Failure { error: ToolError },
}

/// Typed result envelope for one tool call. A tool failure never aborts the
/// turn by itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub call_id: ToolCallId,
    pub tool: String,
    pub outcome: ToolOutcome,
    /// Tokens spent on behalf of this call outside the main model loop (a
    /// sub-agent). The driver adds them to the turn usage.
    pub usage: Option<TokenUsage>,
    /// Images the model should see with this result, such as a screenshot.
    /// The driver attaches them to the tool message for the next call.
    pub images: Vec<crate::message::ImagePart>,
}

impl ToolResult {
    pub fn success(
        call_id: ToolCallId,
        tool: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            call_id,
            tool: tool.into(),
            outcome: ToolOutcome::Success {
                content: content.into(),
            },
            usage: None,
            images: Vec::new(),
        }
    }

    pub fn failure(call_id: ToolCallId, tool: impl Into<String>, error: ToolError) -> Self {
        Self {
            call_id,
            tool: tool.into(),
            outcome: ToolOutcome::Failure { error },
            usage: None,
            images: Vec::new(),
        }
    }

    /// Records tokens spent by a delegated run so the turn budget sees them.
    pub fn with_usage(mut self, usage: TokenUsage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Attaches images to the result; the driver hands them to the model with
    /// the tool message.
    pub fn with_images(mut self, images: Vec<crate::message::ImagePart>) -> Self {
        self.images = images;
        self
    }

    pub fn is_success(&self) -> bool {
        matches!(self.outcome, ToolOutcome::Success { .. })
    }

    pub fn error(&self) -> Option<&ToolError> {
        match &self.outcome {
            ToolOutcome::Success { .. } => None,
            ToolOutcome::Failure { error } => Some(error),
        }
    }

    pub fn content(&self) -> Option<&str> {
        match &self.outcome {
            ToolOutcome::Success { content } => Some(content),
            ToolOutcome::Failure { .. } => None,
        }
    }
}

/// Everything a tool needs for one call.
#[derive(Clone)]
pub struct ToolCtx {
    pub turn_id: TurnId,
    pub call_id: ToolCallId,
    pub workspace_root: PathBuf,
    pub cancel: CancelToken,
}

/// Typed capability the model can call.
///
/// The tool declares what access it needs; the kernel's dispatcher asks the
/// permission gate before `call` runs. A tool implementation never decides
/// whether it is allowed to run.
#[async_trait]
pub trait Tool: Send + Sync {
    fn schema(&self) -> ToolSchema;

    fn permission_requests(&self, args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError>;

    /// May overlap with other calls in the same round even though it is not
    /// read-only. The dispatcher uses this for batching; risk stays as
    /// declared for the UI and the audit.
    fn concurrent(&self) -> bool {
        false
    }

    async fn call(&self, args: ToolArgs, ctx: &ToolCtx) -> ToolResult;
}
