use std::sync::Arc;

use async_trait::async_trait;

use crate::ids::TurnId;
use crate::message::{Message, MessageRole};

/// Inputs available to context stages.
pub struct BuildCtx<'a> {
    pub turn_id: TurnId,
    pub input: &'a str,
    pub history: &'a [Arc<Message>],
    pub system_prompt: &'a str,
}

/// One piece of prompt content produced by a stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    pub role: MessageRole,
    pub text: String,
}

impl Fragment {
    /// Rough token estimate used for budget enforcement. It is intentionally
    /// cheap; exact tokenization belongs to the model adapter.
    pub fn estimated_tokens(&self) -> u32 {
        estimate_tokens(&self.text)
    }
}

/// Approximate token count: four characters per token, rounded up.
pub fn estimate_tokens(text: &str) -> u32 {
    let characters = text.chars().count();
    let tokens = characters.saturating_add(3) / 4;
    tokens.min(u32::MAX as usize) as u32
}

/// A context fragment producer. Stages are ordered by priority; equal
/// priorities keep registration order.
#[async_trait]
pub trait ContextStage: Send + Sync {
    fn name(&self) -> &'static str;
    fn priority(&self) -> i32;
    async fn build(&self, ctx: &BuildCtx<'_>) -> Option<Fragment>;
}
