//! Model-written segment summaries.
//!
//! Structured records stay authoritative; the model summary is an optional
//! extra layer for segments large enough to be worth one call.

use std::sync::Arc;

use async_trait::async_trait;
use keel_core::{
    redact_secrets, CancelToken, Message, MessageRole, ModelChunk, ModelError, ModelProvider,
    ModelRequest, ModelRoute, ReasoningLevel, TokenUsage,
};

/// Minimum estimated transcript size before a segment is summarized.
pub const SUMMARY_MIN_TOKENS: u32 = 800;
/// Maximum characters kept from one message in the summary transcript.
pub const TRANSCRIPT_MESSAGE_CHARS: usize = 1500;

const SUMMARY_PROMPT: &str = "You compress one finished work segment for future context. \
Do not add facts that are not in the transcript. Keep file paths, identifiers, commands and \
numbers exactly as written; keep entity names in their original language. Reply in the language \
of the transcript with: goal, result, key decisions, errors and how they were resolved, changed \
artifacts, open items. No preamble. At most 180 words.";

/// A summary and the tokens it cost.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summarized {
    pub text: String,
    pub usage: TokenUsage,
}

/// Compresses a finished segment into a short report.
#[async_trait]
pub trait Summarizer: Send + Sync {
    async fn summarize(
        &self,
        transcript: &str,
        cancel: &CancelToken,
    ) -> Result<Summarized, ModelError>;
}

/// Summarizer backed by the same chat provider as the main turn.
pub struct ProviderSummarizer<M> {
    model: M,
    max_output_tokens: u32,
}

impl<M> ProviderSummarizer<M> {
    pub fn new(model: M, max_output_tokens: u32) -> Self {
        Self {
            model,
            max_output_tokens,
        }
    }
}

#[async_trait]
impl<M: ModelProvider> Summarizer for ProviderSummarizer<M> {
    async fn summarize(
        &self,
        transcript: &str,
        cancel: &CancelToken,
    ) -> Result<Summarized, ModelError> {
        let request = ModelRequest {
            messages: vec![
                Arc::new(Message::system(SUMMARY_PROMPT)),
                Arc::new(Message::user(transcript)),
            ],
            tools: Vec::new(),
            thinking_budget: 0,
            max_output_tokens: self.max_output_tokens,
            route: ModelRoute {
                provider: "summary".to_owned(),
                model: String::new(),
            },
            reasoning: ReasoningLevel::Low,
        };
        let mut stream = self.model.stream(request, cancel).await?;
        let mut text = String::new();
        let mut usage = TokenUsage::default();
        while let Some(chunk) = stream.recv().await {
            match chunk {
                Ok(ModelChunk::TextDelta(delta)) => {
                    text.push_str(&delta);
                    if text.chars().count() > 2000 {
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
        let text = text.trim().to_owned();
        if text.is_empty() {
            return Err(ModelError::internal("summarizer returned no text"));
        }
        Ok(Summarized { text, usage })
    }
}

/// Renders a segment transcript for the summarizer, with secrets redacted.
pub fn transcript_text(messages: &[Arc<Message>]) -> String {
    let mut text = String::new();
    for message in messages {
        let role = match message.role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            _ => continue,
        };
        let content: String = message
            .content
            .chars()
            .take(TRANSCRIPT_MESSAGE_CHARS)
            .collect();
        text.push_str(role);
        text.push_str(": ");
        text.push_str(&redact_secrets(&content));
        text.push_str("\n\n");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MockModel, MockStep};

    #[tokio::test]
    async fn a_provider_summary_returns_its_text() {
        let summarizer = ProviderSummarizer::new(
            MockModel::new(vec![MockStep::Text("summary".to_owned())]),
            64,
        );
        let summary = summarizer
            .summarize("user: hello", &CancelToken::new())
            .await
            .unwrap();
        assert_eq!(summary.text, "summary");
    }

    #[tokio::test]
    async fn an_empty_summary_is_an_error() {
        let summarizer =
            ProviderSummarizer::new(MockModel::new(vec![MockStep::Text(String::new())]), 64);
        assert!(summarizer
            .summarize("user: hello", &CancelToken::new())
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
}
