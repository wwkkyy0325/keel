//! Model-written rolling summaries of a session.
//!
//! One summary field is rewritten at every segment seal. Structured records
//! and the raw transcript stay in storage; the summary is the only compressed
//! view of older history that enters the context.

use std::sync::Arc;

use async_trait::async_trait;
use keel_core::{
    estimate_tokens, redact_secrets, CancelToken, Message, MessageRole, ModelChunk, ModelError,
    ModelProvider, ModelRequest, ModelRoute, ReasoningLevel, TokenUsage,
};

/// Lower bound for the summary budget.
pub const MEMO_BUDGET_FLOOR: u32 = 256;
/// Upper bound for the summary budget.
pub const MEMO_BUDGET_CEILING: u32 = 4096;
/// Room left for the summary prompt and provider framing.
pub const SUMMARY_INPUT_MARGIN: u32 = 1024;
/// Minimum room reserved for the newest segment transcript.
pub const MIN_TRANSCRIPT_TOKENS: u32 = 256;
/// Characters kept from the head of one transcript message.
pub const TRANSCRIPT_HEAD_CHARS: usize = 1000;
/// Characters kept from the tail of one transcript message.
pub const TRANSCRIPT_TAIL_CHARS: usize = 500;
/// A summary below a quarter of its budget cannot replace the records it
/// covers. The floor is clamped so small sessions stay usable.
const MIN_SUMMARY_FLOOR: u32 = 64;
const MAX_SUMMARY_FLOOR: u32 = 256;

const SUMMARY_PROMPT: &str = "You maintain one rolling summary of a long work session. \
You receive the previous summary, finished segments, and the newest segment transcript. \
Start with exactly one line of the form `SEGMENT_MEMO: <the newest segment in at most 120 words: \
what was asked, what was done, what was decided>`. Then a blank line. Then rewrite the whole \
summary. Keep the current goal and state, decisions and why, exact file paths, \
identifiers, commands and error messages, mistakes and fixes, open items, and anything the user \
explicitly required or rejected. Drop details that no longer matter. If the new segment contradicts \
the previous summary, replace the old statement. Do not add facts that are not in the input. Keep \
entity names in their original language. Use these sections: Goal and state; Decisions; Artifacts; \
Errors and fixes; Open items. No preamble.";

/// Splits the per-segment memo line from the rolling summary.
///
/// A model that ignores the instruction falls back to the old behaviour: the
/// whole text is the summary and no memo is stored.
pub fn split_segment_memo(text: &str) -> (Option<String>, String) {
    let trimmed = text.trim();
    let Some(line_end) = trimmed.find('\n') else {
        return (None, trimmed.to_owned());
    };
    let first = trimmed[..line_end]
        .trim()
        .trim_start_matches(['*', '#', ' ']);
    let lower = first.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("segment_memo:") else {
        return (None, trimmed.to_owned());
    };
    // The marker is ASCII, so its byte length is the same in both strings.
    let memo = first[first.len() - rest.len()..].trim();
    let summary = trimmed[line_end + 1..].trim();
    if memo.is_empty() || summary.is_empty() {
        return (None, trimmed.to_owned());
    }
    (Some(memo.to_owned()), summary.to_owned())
}

/// Summary budget for one session, scaled from the working set.
pub fn memo_budget(working_set: u32) -> u32 {
    (working_set / 10).clamp(MEMO_BUDGET_FLOOR, MEMO_BUDGET_CEILING)
}

/// Minimum text a summary must carry before it may absorb a segment.
fn summary_floor(max_tokens: u32) -> u32 {
    (max_tokens / 4).clamp(MIN_SUMMARY_FLOOR, MAX_SUMMARY_FLOOR)
}

/// A summary and the tokens it cost.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summarized {
    pub text: String,
    pub usage: TokenUsage,
}

/// Compresses the finished segments into the session summary.
#[async_trait]
pub trait Summarizer: Send + Sync {
    async fn summarize(
        &self,
        input: &str,
        max_tokens: u32,
        cancel: &CancelToken,
    ) -> Result<Summarized, ModelError>;
}

/// Summarizer backed by the same chat provider as the main turn.
pub struct ProviderSummarizer<M> {
    model: M,
}

impl<M> ProviderSummarizer<M> {
    pub fn new(model: M) -> Self {
        Self { model }
    }
}

#[async_trait]
impl<M: ModelProvider> Summarizer for ProviderSummarizer<M> {
    async fn summarize(
        &self,
        input: &str,
        max_tokens: u32,
        cancel: &CancelToken,
    ) -> Result<Summarized, ModelError> {
        let prompt = format!("{SUMMARY_PROMPT} Use at most {max_tokens} tokens.");
        let request = ModelRequest {
            messages: vec![
                Arc::new(Message::system(prompt)),
                Arc::new(Message::user(input)),
            ],
            tools: Vec::new(),
            control_tool: false,
            thinking_budget: 0,
            max_output_tokens: max_tokens,
            route: ModelRoute {
                provider: "summary".to_owned(),
                model: String::new(),
            },
            reasoning: ReasoningLevel::None,
        };
        let mut stream = self.model.stream(request, cancel).await?;
        let mut text = String::new();
        let mut usage = TokenUsage::default();
        // A provider that ignores the output limit must not stream forever.
        let guard = max_tokens.saturating_mul(8).max(2048) as usize;
        while let Some(chunk) = stream.recv().await {
            match chunk {
                Ok(ModelChunk::TextDelta(delta)) => {
                    text.push_str(&delta);
                    if text.chars().count() > guard {
                        break;
                    }
                }
                Ok(ModelChunk::Finished { usage: reported }) => {
                    usage = reported;
                    break;
                }
                Ok(_) => {}
                Err(error) => return Err(error),
            }
        }
        let text = trim_to_budget(text.trim(), max_tokens);
        if text.is_empty() {
            return Err(ModelError::internal("summarizer returned no text"));
        }
        // A summary this short cannot stand in for the records it covers; let
        // the driver keep the previous memo and write the structured fallback.
        if estimate_tokens(&text) < summary_floor(max_tokens) {
            return Err(ModelError::internal("summarizer returned too little text"));
        }
        Ok(Summarized { text, usage })
    }
}

/// Cuts text down to a token budget at a sentence boundary.
///
/// `max_output_tokens` is a request to the provider, not a guarantee. This is
/// the last guard, and it never cuts inside a sentence when a boundary exists.
pub fn trim_to_budget(text: &str, max_tokens: u32) -> String {
    if estimate_tokens(text) <= max_tokens {
        return text.to_owned();
    }
    let mut cutoff = text.len();
    while cutoff > 0 {
        while cutoff > 0 && !text.is_char_boundary(cutoff) {
            cutoff -= 1;
        }
        if estimate_tokens(&text[..cutoff]) <= max_tokens {
            break;
        }
        cutoff = cutoff.saturating_sub(16);
    }
    let head = &text[..cutoff];
    let boundary = head
        .rfind(['\n', '.', '。', ';', '；'])
        .unwrap_or(head.len());
    let mut out = head[..boundary].trim_end().to_owned();
    if !out.is_empty() {
        out.push_str(" ...");
    }
    out
}

/// Cuts text down to a token budget, keeping both ends.
///
/// Used for summarizer input, where the newest tool result or error is often
/// at the end of the transcript.
pub fn clip_to_tokens(text: &str, max_tokens: u32) -> String {
    let tokens = estimate_tokens(text);
    if tokens <= max_tokens || tokens == 0 {
        return text.to_owned();
    }
    let chars = text.chars().count();
    let keep = ((chars as u64 * u64::from(max_tokens)) / u64::from(tokens)) as usize;
    let keep = keep.max(1).min(chars);
    let head_chars = keep * 2 / 3;
    let tail_chars = keep.saturating_sub(head_chars);
    let head: String = text.chars().take(head_chars).collect();
    let mut tail: Vec<char> = text.chars().rev().take(tail_chars).collect();
    tail.reverse();
    format!("{head}\n...\n{}", tail.into_iter().collect::<String>())
}

/// Renders a segment transcript for the summarizer, with secrets redacted and
/// long messages kept at both ends.
pub fn transcript_text(messages: &[Arc<Message>]) -> String {
    let mut text = String::new();
    for message in messages {
        let role = match message.role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            _ => continue,
        };
        text.push_str(role);
        text.push_str(": ");
        text.push_str(&redact_secrets(&clip_message(&message.content)));
        text.push_str("\n\n");
    }
    text
}

fn clip_message(content: &str) -> String {
    let characters = content.chars().count();
    if characters <= TRANSCRIPT_HEAD_CHARS + TRANSCRIPT_TAIL_CHARS + 1 {
        return content.to_owned();
    }
    let head: String = content.chars().take(TRANSCRIPT_HEAD_CHARS).collect();
    let mut tail: Vec<char> = content.chars().rev().take(TRANSCRIPT_TAIL_CHARS).collect();
    tail.reverse();
    format!("{head}\n...\n{}", tail.into_iter().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MockModel, MockStep};

    struct CapturingModel {
        reasoning: Arc<std::sync::Mutex<Option<ReasoningLevel>>>,
    }

    #[async_trait]
    impl ModelProvider for CapturingModel {
        async fn stream(
            &self,
            request: ModelRequest,
            _cancel: &CancelToken,
        ) -> Result<keel_core::ModelStream, ModelError> {
            *self
                .reasoning
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(request.reasoning);
            let (tx, rx) = tokio::sync::mpsc::channel(4);
            let text = "summary ".repeat(200);
            let _ = tx.send(Ok(ModelChunk::TextDelta(text))).await;
            let _ = tx
                .send(Ok(ModelChunk::Finished {
                    usage: TokenUsage::default(),
                }))
                .await;
            Ok(keel_core::ModelStream::new(rx))
        }
    }

    #[tokio::test]
    async fn the_summarizer_disables_reasoning() {
        // Reasoning shares the output budget. At DeepSeek, `low` can consume
        // every token and leave an empty summary.
        let seen = Arc::new(std::sync::Mutex::new(None));
        let summarizer = ProviderSummarizer::new(CapturingModel {
            reasoning: seen.clone(),
        });
        summarizer
            .summarize("user: hello", 512, &CancelToken::new())
            .await
            .unwrap();
        let level = *seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(level, Some(ReasoningLevel::None));
    }

    #[tokio::test]
    async fn a_provider_summary_returns_its_text() {
        let text = "summary ".repeat(200);
        let summarizer =
            ProviderSummarizer::new(MockModel::new(vec![MockStep::Text(text.clone())]));
        let summary = summarizer
            .summarize("user: hello", 512, &CancelToken::new())
            .await
            .unwrap();
        assert_eq!(summary.text, text.trim_end());
    }

    #[tokio::test]
    async fn a_degenerate_summary_is_an_error() {
        let summarizer =
            ProviderSummarizer::new(MockModel::new(vec![MockStep::Text("摘要".repeat(63))]));
        assert!(summarizer
            .summarize("user: hello", 756, &CancelToken::new())
            .await
            .is_err());
    }

    #[test]
    fn the_summary_floor_scales_and_stays_bounded() {
        assert_eq!(summary_floor(128), 64);
        assert_eq!(summary_floor(756), 189);
        assert_eq!(summary_floor(4_096), 256);
    }

    #[test]
    fn a_segment_memo_is_split_from_the_rolling_summary() {
        let (memo, summary) = split_segment_memo(
            "SEGMENT_MEMO: asked for the die fix; edited commands.rs.\n\n## Goal and state\n- done",
        );
        assert_eq!(
            memo.as_deref(),
            Some("asked for the die fix; edited commands.rs.")
        );
        assert!(summary.starts_with("## Goal and state"));
    }

    #[test]
    fn a_summary_without_the_memo_marker_stays_whole() {
        let (memo, summary) = split_segment_memo("## Goal and state\n- done");
        assert!(memo.is_none());
        assert_eq!(summary, "## Goal and state\n- done");
    }

    #[test]
    fn a_memo_without_a_summary_is_rejected() {
        let (memo, summary) = split_segment_memo("SEGMENT_MEMO: only a memo");
        assert!(memo.is_none());
        assert_eq!(summary, "SEGMENT_MEMO: only a memo");
    }

    #[tokio::test]
    async fn an_empty_summary_is_an_error() {
        let summarizer =
            ProviderSummarizer::new(MockModel::new(vec![MockStep::Text(String::new())]));
        assert!(summarizer
            .summarize("user: hello", 64, &CancelToken::new())
            .await
            .is_err());
    }

    #[test]
    fn the_transcript_keeps_roles_and_redacts() {
        let messages = vec![
            Arc::new(Message::user("sk-abcdefghijklmnopqrstuvwxyz")),
            Arc::new(Message::assistant("done")),
        ];
        let text = transcript_text(&messages);
        assert!(text.starts_with("user: "));
        assert!(!text.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(text.contains("assistant: done"));
    }

    #[test]
    fn a_long_message_keeps_its_head_and_tail() {
        let mut content = "head ".repeat(400);
        content.push_str("ERROR at the end");
        let messages = vec![Arc::new(Message::assistant(content))];
        let text = transcript_text(&messages);
        assert!(text.starts_with("assistant: head"));
        assert!(text.contains("ERROR at the end"));
        assert!(text.contains("\n...\n"));
    }

    #[test]
    fn the_budget_scales_with_the_working_set_and_stays_bounded() {
        assert_eq!(memo_budget(1_000), MEMO_BUDGET_FLOOR);
        assert_eq!(memo_budget(7_168), 716);
        assert_eq!(memo_budget(200_000), MEMO_BUDGET_CEILING);
    }

    #[test]
    fn trimming_keeps_a_sentence_boundary() {
        let text = "one two three four five six seven eight nine ten.";
        let trimmed = trim_to_budget(text, 8);
        assert!(trimmed.len() <= text.len());
        assert!(trimmed.ends_with("..."));
        assert!(estimate_tokens(&trimmed) <= 8);
    }

    #[test]
    fn clipping_keeps_both_ends() {
        let text = format!("{}MIDDLE{}", "a".repeat(1000), "z".repeat(1000));
        let clipped = clip_to_tokens(&text, 100);
        assert!(clipped.starts_with('a'));
        assert!(clipped.ends_with('z'));
        assert!(clipped.contains("MIDDLE") || clipped.contains("..."));
    }
}
