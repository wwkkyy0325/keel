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
    let mut ascii = 0usize;
    let mut cjk = 0usize;
    let mut other = 0usize;
    for character in text.chars() {
        if character.is_ascii() {
            ascii += 1;
        } else if is_cjk(character) {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    // ASCII is about four characters per token; CJK is about one token per
    // character, which the old four-characters rule underestimated by 3-4x.
    let tokens = ascii.saturating_add(3) / 4 + cjk + other.saturating_add(1) / 2;
    tokens.min(u32::MAX as usize) as u32
}

/// Whether a character belongs to a CJK block.
pub fn is_cjk(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{3040}'..='\u{30FF}'
            | '\u{AC00}'..='\u{D7AF}'
    )
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

#[cfg(test)]
mod estimate_tests {
    use super::estimate_tokens;

    #[test]
    fn ascii_stays_at_four_characters_per_token() {
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }

    #[test]
    fn cjk_counts_about_one_token_per_character() {
        assert_eq!(estimate_tokens("你好世界"), 4);
        assert_eq!(estimate_tokens("我们刚才说了什么"), 8);
    }

    #[test]
    fn mixed_text_adds_both_script_costs() {
        assert_eq!(estimate_tokens("hi 你好"), 3);
    }
}
