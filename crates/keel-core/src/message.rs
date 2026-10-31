use crate::ids::ToolCallId;
use crate::model::ToolCallRequest;

/// Role of a message in the model conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// One message in the model conversation.
///
/// Tool calls and tool results are structured so a wire adapter can preserve
/// call ids. `content` remains the text projection for prompts and logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    pub tool_calls: Vec<ToolCallRequest>,
    pub tool_call_id: Option<ToolCallId>,
    /// Provider reasoning text that must be echoed back with tool calls for
    /// thinking-mode models such as DeepSeek.
    pub reasoning_content: Option<String>,
}

impl Message {
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::new(MessageRole::System, content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(MessageRole::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(MessageRole::Assistant, content)
    }

    /// Builds an assistant message that requested tool calls.
    pub fn assistant_tool_calls(tool_calls: Vec<ToolCallRequest>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: String::new(),
            tool_calls,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    /// Attaches provider reasoning text to this message.
    pub fn with_reasoning(mut self, reasoning: impl Into<String>) -> Self {
        self.reasoning_content = Some(reasoning.into());
        self
    }

    /// Builds a tool-result message linked to its originating call.
    pub fn tool_result(call_id: ToolCallId, tool: &str, content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: format!("{tool}: {}", content.into()),
            tool_calls: Vec::new(),
            tool_call_id: Some(call_id),
            reasoning_content: None,
        }
    }

    /// Builds a tool-result message without a call id. Prefer
    /// [`Message::tool_result`] when the call id is available.
    pub fn tool(tool: &str, content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: format!("{tool}: {}", content.into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    /// Cheap token estimate for budget checks, including tool-call payloads.
    pub fn estimated_tokens(&self) -> u32 {
        let mut total = crate::context::estimate_tokens(&self.content);
        if let Some(reasoning) = &self.reasoning_content {
            total = total.saturating_add(crate::context::estimate_tokens(reasoning));
        }
        for call in &self.tool_calls {
            total = total.saturating_add(crate::context::estimate_tokens(&call.name));
            for key in call.args.keys() {
                total = total.saturating_add(crate::context::estimate_tokens(key));
                if let Some(value) = call.args.get(key) {
                    total = total.saturating_add(value.estimated_tokens());
                }
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ToolArgs, ToolCallId};

    fn call() -> ToolCallRequest {
        ToolCallRequest {
            id: ToolCallId::new(7),
            name: "echo".to_owned(),
            args: ToolArgs::new(),
        }
    }

    #[test]
    fn assistant_tool_calls_are_structured() {
        let message = Message::assistant_tool_calls(vec![call()]);
        assert_eq!(message.role, MessageRole::Assistant);
        assert_eq!(message.tool_calls, vec![call()]);
        assert!(message.tool_call_id.is_none());
        assert!(message.reasoning_content.is_none());
    }

    #[test]
    fn reasoning_attaches_to_tool_call_messages() {
        let message = Message::assistant_tool_calls(vec![call()]).with_reasoning("plan");
        assert_eq!(message.reasoning_content.as_deref(), Some("plan"));
        assert!(message.estimated_tokens() > 0);
    }

    #[test]
    fn tool_result_carries_the_call_id() {
        let message = Message::tool_result(ToolCallId::new(7), "echo", "ok");
        assert_eq!(message.role, MessageRole::Tool);
        assert_eq!(message.tool_call_id, Some(ToolCallId::new(7)));
        assert_eq!(message.content, "echo: ok");
    }
}
