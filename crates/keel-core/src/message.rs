use crate::ids::ToolCallId;
use crate::model::ToolCallRequest;

/// One image attached to a message.
///
/// The bytes travel with the in-memory conversation only: the session
/// snapshot stores no image data, so a screenshot is evidence for the turn
/// that captured it and a later turn must ask for a new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePart {
    /// MIME type such as `image/png`.
    pub media_type: String,
    pub data: Vec<u8>,
}

/// What one image costs the context budget. Providers resize an image to a
/// fixed budget, so the byte length is not a token count.
pub const ESTIMATED_IMAGE_TOKENS: u32 = 1024;

impl ImagePart {
    pub fn new(media_type: impl Into<String>, data: Vec<u8>) -> Self {
        Self {
            media_type: media_type.into(),
            data,
        }
    }

    pub fn png(data: Vec<u8>) -> Self {
        Self::new("image/png", data)
    }
}

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
    /// Signature an Anthropic-style provider attaches to a thinking block.
    /// The block must be sent back verbatim on a tool follow-up, and the
    /// provider rejects one without its signature.
    pub thinking_signature: Option<String>,
    /// Images this message carries. A tool that captures a screenshot attaches
    /// it to its result message so the next model call can see it.
    pub images: Vec<ImagePart>,
}

impl Message {
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning_content: None,
            thinking_signature: None,
            images: Vec::new(),
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
            thinking_signature: None,
            images: Vec::new(),
        }
    }

    /// Attaches provider reasoning text to this message.
    pub fn with_reasoning(mut self, reasoning: impl Into<String>) -> Self {
        self.reasoning_content = Some(reasoning.into());
        self
    }

    /// Attaches the signature that must accompany the reasoning when the
    /// provider echoes a thinking block back.
    pub fn with_thinking_signature(mut self, signature: impl Into<String>) -> Self {
        self.thinking_signature = Some(signature.into());
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
            thinking_signature: None,
            images: Vec::new(),
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
            thinking_signature: None,
            images: Vec::new(),
        }
    }

    /// Attaches images the model should see with this message.
    pub fn with_images(mut self, images: Vec<ImagePart>) -> Self {
        self.images = images;
        self
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
        // Providers resize an image to a fixed budget; the byte length is not
        // a token count, so every image is charged the same estimate.
        for _ in &self.images {
            total = total.saturating_add(ESTIMATED_IMAGE_TOKENS);
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
