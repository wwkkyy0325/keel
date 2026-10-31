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
    /// turn can see what was asked.
    pub fn record_turn(&mut self, input: &str, text: &str) {
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
        if !text.trim().is_empty() {
            segment.entries.push(Entry {
                message: Arc::new(Message::assistant(text)),
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
        session.record_turn("fix the parser", "done");

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
        session.record_turn("first topic", "ok");
        assert!(session.seal_current());
        assert!(!session.seal_current());
        session.record_turn("second topic", "ok");

        assert_eq!(session.segments().len(), 2);
        assert_eq!(session.segments()[0].state, SegmentState::Sealed);
        assert_eq!(session.segments()[1].topic, "second topic");
        assert_eq!(session.history().len(), 4);
    }

    #[test]
    fn a_turn_without_text_still_records_the_input() {
        let mut session = Session::new();
        session.record_turn("unanswered", "");

        let history = session.history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, "unanswered");
    }

    #[test]
    fn topic_keeps_the_original_language_and_is_bounded() {
        let mut session = Session::new();
        let long = "修复解析器".repeat(40);
        session.record_turn(&long, "done");

        let topic = &session.segments()[0].topic;
        assert!(topic.starts_with("修复解析器"));
        assert_eq!(topic.chars().count(), 80);
        assert!(!topic.contains("fix"));
    }
}
