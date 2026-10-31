use std::sync::Arc;

use crate::message::{Message, MessageRole};

/// Lifecycle of one topic segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentState {
    Open,
    Sealed,
}

/// One recorded item inside a segment.
#[derive(Debug, Clone)]
pub struct Entry {
    pub message: Arc<Message>,
}

/// Compact record of one tool call and its result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSummary {
    pub name: String,
    pub args: String,
    pub status: String,
    pub output: String,
}

impl ToolSummary {
    /// Renders the summary as history text.
    pub fn render(&self) -> String {
        let mut line = format!("- {}", self.name);
        if !self.args.is_empty() {
            line.push(' ');
            line.push_str(&self.args);
        }
        line.push_str(" -> ");
        line.push_str(&self.status);
        if !self.output.trim().is_empty() {
            for output_line in self.output.lines() {
                line.push('\n');
                line.push_str("  ");
                line.push_str(output_line);
            }
        }
        line
    }
}

/// A topic span of a session.
///
/// Segmentation is decided by the kernel; this type only holds the result. A
/// sealed segment is never reopened.
#[derive(Debug, Clone)]
pub struct Segment {
    pub id: u32,
    pub topic: String,
    pub state: SegmentState,
    pub entries: Vec<Entry>,
}

/// Ordered conversation state for one host session.
///
/// The session stores user and assistant messages for completed turns. Tool
/// output summaries and compressed records are added by later M3 steps.
#[derive(Debug, Default)]
pub struct Session {
    segments: Vec<Segment>,
    next_segment_id: u32,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a completed turn, opening a segment when none is open.
    ///
    /// The input is stored even when the turn produced no text, so the next
    /// turn can see what was asked. Tool summaries are appended to the
    /// assistant entry as a compact block.
    pub fn record_turn(&mut self, input: &str, text: &str, tools: &[ToolSummary]) {
        if !matches!(
            self.segments.last().map(|segment| segment.state),
            Some(SegmentState::Open)
        ) {
            let id = self.next_segment_id;
            self.next_segment_id = self.next_segment_id.saturating_add(1);
            self.segments.push(Segment {
                id,
                topic: topic_of(input),
                state: SegmentState::Open,
                entries: Vec::new(),
            });
        }

        let Some(segment) = self.segments.last_mut() else {
            return;
        };
        if !input.trim().is_empty() {
            segment.entries.push(Entry {
                message: Arc::new(Message::user(input)),
            });
        }
        let mut body = text.trim().to_owned();
        if !tools.is_empty() {
            if !body.is_empty() {
                body.push_str("\n\n");
            }
            body.push_str("[tools]\n");
            for tool in tools {
                body.push_str(&tool.render());
                body.push('\n');
            }
        }
        if !body.trim().is_empty() {
            segment.entries.push(Entry {
                message: Arc::new(Message::assistant(body)),
            });
        }
    }

    /// Seals the open segment. Returns whether one was sealed.
    pub fn seal_current(&mut self) -> bool {
        let Some(segment) = self.segments.last_mut() else {
            return false;
        };
        if segment.state == SegmentState::Sealed {
            return false;
        }
        segment.state = SegmentState::Sealed;
        true
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Tokens in the most recent turn, used as a growth estimate.
    pub fn last_turn_tokens(&self) -> u32 {
        let Some(segment) = self.segments.last() else {
            return 0;
        };
        let start = segment
            .entries
            .iter()
            .rposition(|entry| entry.message.role == MessageRole::User)
            .unwrap_or(0);
        segment.entries[start..].iter().fold(0u32, |total, entry| {
            total.saturating_add(entry.message.estimated_tokens())
        })
    }

    /// Flattens recorded entries in chronological order.
    pub fn history(&self) -> Vec<Arc<Message>> {
        self.segments
            .iter()
            .flat_map(|segment| segment.entries.iter())
            .filter(|entry| {
                matches!(
                    entry.message.role,
                    MessageRole::User | MessageRole::Assistant
                )
            })
            .map(|entry| entry.message.clone())
            .collect()
    }
}

fn topic_of(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return "untitled".to_owned();
    }
    trimmed.chars().take(80).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_turn_opens_a_segment_and_keeps_order() {
        let mut session = Session::new();
        session.record_turn("fix the parser", "done", &[]);

        assert_eq!(session.segments().len(), 1);
        let segment = &session.segments()[0];
        assert_eq!(segment.state, SegmentState::Open);
        assert_eq!(segment.topic, "fix the parser");
        assert_eq!(segment.entries.len(), 2);

        let history = session.history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, MessageRole::User);
        assert_eq!(history[0].content, "fix the parser");
        assert_eq!(history[1].role, MessageRole::Assistant);
        assert_eq!(history[1].content, "done");
    }

    #[test]
    fn sealing_starts_a_new_segment() {
        let mut session = Session::new();
        session.record_turn("first topic", "ok", &[]);
        assert!(session.seal_current());
        assert!(!session.seal_current());
        session.record_turn("second topic", "ok", &[]);

        assert_eq!(session.segments().len(), 2);
        assert_eq!(session.segments()[0].state, SegmentState::Sealed);
        assert_eq!(session.segments()[1].topic, "second topic");
        assert_eq!(session.history().len(), 4);
    }

    #[test]
    fn a_turn_without_text_still_records_the_input() {
        let mut session = Session::new();
        session.record_turn("unanswered", "", &[]);

        let history = session.history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, "unanswered");
    }

    #[test]
    fn topic_keeps_the_original_language_and_is_bounded() {
        let mut session = Session::new();
        let long = "修复解析器".repeat(40);
        session.record_turn(&long, "done", &[]);

        let topic = &session.segments()[0].topic;
        assert!(topic.starts_with("修复解析器"));
        assert_eq!(topic.chars().count(), 80);
        assert!(!topic.contains("fix"));
    }
}

#[cfg(test)]
mod tool_summary_tests {
    use super::*;

    #[test]
    fn tool_summaries_are_appended_to_the_assistant_entry() {
        let mut session = Session::new();
        let summary = ToolSummary {
            name: "read_file".to_owned(),
            args: "path=src/lib.rs".to_owned(),
            status: "ok".to_owned(),
            output: "line one\nline two".to_owned(),
        };
        session.record_turn("read it", "it parses", &[summary]);

        let history = session.history();
        assert_eq!(history.len(), 2);
        assert!(history[1].content.starts_with("it parses"));
        assert!(history[1].content.contains("[tools]"));
        assert!(history[1]
            .content
            .contains("- read_file path=src/lib.rs -> ok"));
        assert!(history[1].content.contains("  line one"));
    }
}
