use std::sync::Arc;

use async_trait::async_trait;

use crate::event::EventSink;
use crate::ids::TurnId;
use crate::message::{Message, MessageRole};
use crate::session::{Fact, SegmentRecord};

/// Inputs available to context stages.
pub struct BuildCtx<'a> {
    pub turn_id: TurnId,
    pub input: &'a str,
    pub history: &'a [Arc<Message>],
    /// Compressed records of sealed segments, oldest first.
    pub records: &'a [SegmentRecord],
    /// Global facts first, then session facts.
    pub facts: &'a [Fact],
    pub system_prompt: &'a str,
    /// Tokens left for this stage after higher-priority stages.
    pub budget_tokens: u32,
    /// Share of the history budget kept verbatim; chosen by the governor.
    pub detail_share: f32,
    /// Optional sink for stage-level observability.
    pub events: Option<&'a dyn EventSink>,
}

impl<'a> BuildCtx<'a> {
    /// Rebuilds the context with the budget remaining for one stage.
    pub fn with_budget(&self, budget_tokens: u32) -> BuildCtx<'a> {
        BuildCtx {
            turn_id: self.turn_id,
            input: self.input,
            history: self.history,
            records: self.records,
            facts: self.facts,
            system_prompt: self.system_prompt,
            budget_tokens,
            detail_share: self.detail_share,
            events: self.events,
        }
    }
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
///
/// A stage may emit several fragments, because history is a sequence of user
/// and assistant messages rather than one text block.
#[async_trait]
pub trait ContextStage: Send + Sync {
    fn name(&self) -> &'static str;
    fn priority(&self) -> i32;
    async fn build(&self, ctx: &BuildCtx<'_>) -> Option<Vec<Fragment>>;
}
