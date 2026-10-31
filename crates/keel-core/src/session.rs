use std::collections::BTreeSet;
use std::sync::Arc;

use crate::message::{Message, MessageRole};

/// Turns in one segment before the kernel seals it.
pub const SEGMENT_TURN_CAP: u32 = 12;
/// Input-to-segment term overlap below this is a topic change.
pub const TOPIC_OVERLAP_FLOOR: f32 = 0.2;

/// Words ignored when comparing a new input with a segment.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "this", "that", "from", "into", "your", "you", "are", "was",
    "has", "have", "still", "also", "please", "then", "them", "they",
];

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
    pub turns: u32,
    pub terms: BTreeSet<String>,
    pub artifacts: BTreeSet<String>,
}

/// What `begin_turn` did to the segment list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SegmentChange {
    pub sealed: Option<u32>,
    pub opened: Option<u32>,
    pub reason: Option<String>,
}

/// Everything one finished turn contributes to the session.
pub struct TurnRecord<'a> {
    pub input: &'a str,
    pub text: &'a str,
    pub tools: &'a [ToolSummary],
    pub artifacts: &'a [String],
    pub verified: bool,
}

/// Ordered conversation state for one host session.
///
/// The session stores user and assistant messages for completed turns. Tool
/// output summaries and compressed records are added by later M3 steps.
#[derive(Debug, Default)]
pub struct Session {
    segments: Vec<Segment>,
    next_segment_id: u32,
    last_verified: bool,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decides the segment boundary for an input before the turn runs.
    ///
    /// The kernel seals the open segment when the previous turn passed an
    /// acceptance plan, when the segment reached its turn cap, or when the
    /// input neither shares terms with the segment nor names one of its
    /// artifacts.
    pub fn begin_turn(&mut self, input: &str) -> SegmentChange {
        let Some((state, open_id)) = self
            .segments
            .last()
            .map(|segment| (segment.state, segment.id))
        else {
            let opened = self.open_segment(input);
            return SegmentChange {
                opened: Some(opened),
                ..SegmentChange::default()
            };
        };
        if state == SegmentState::Sealed {
            let opened = self.open_segment(input);
            return SegmentChange {
                opened: Some(opened),
                ..SegmentChange::default()
            };
        }

        let Some(reason) = self.seal_reason(input) else {
            return SegmentChange::default();
        };
        let sealed = open_id;
        if let Some(segment) = self.segments.last_mut() {
            segment.state = SegmentState::Sealed;
        }
        let opened = self.open_segment(input);
        SegmentChange {
            sealed: Some(sealed),
            opened: Some(opened),
            reason: Some(reason),
        }
    }

    /// Records a completed turn in the open segment.
    ///
    /// The input is stored even when the turn produced no text, so the next
    /// turn can see what was asked. Tool summaries are appended to the
    /// assistant entry as a compact block.
    pub fn record_turn(&mut self, turn: TurnRecord<'_>) {
        if !matches!(
            self.segments.last().map(|segment| segment.state),
            Some(SegmentState::Open)
        ) {
            self.open_segment(turn.input);
        }

        let Some(segment) = self.segments.last_mut() else {
            return;
        };
        segment.turns = segment.turns.saturating_add(1);
        segment.terms.extend(terms(turn.input));
        segment.artifacts.extend(turn.artifacts.iter().cloned());
        self.last_verified = turn.verified;

        let input = turn.input;
        let text = turn.text;
        let tools = turn.tools;
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

    fn open_segment(&mut self, input: &str) -> u32 {
        let id = self.next_segment_id;
        self.next_segment_id = self.next_segment_id.saturating_add(1);
        self.segments.push(Segment {
            id,
            topic: topic_of(input),
            state: SegmentState::Open,
            entries: Vec::new(),
            turns: 0,
            terms: terms(input),
            artifacts: BTreeSet::new(),
        });
        id
    }

    fn seal_reason(&mut self, input: &str) -> Option<String> {
        let verified = std::mem::take(&mut self.last_verified);
        if verified {
            return Some("verified".to_owned());
        }
        let segment = self.segments.last()?;
        if segment.turns >= SEGMENT_TURN_CAP {
            return Some("size".to_owned());
        }

        let input_terms = terms(input);
        let overlap = if input_terms.is_empty() {
            1.0
        } else {
            let hits = input_terms
                .iter()
                .filter(|term| segment.terms.contains(*term))
                .count();
            hits as f32 / input_terms.len() as f32
        };
        let references_artifact = segment.artifacts.iter().any(|path| references(input, path));
        if overlap < TOPIC_OVERLAP_FLOOR && !references_artifact {
            return Some("topic".to_owned());
        }
        None
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
    ///
    /// `begin_turn` may have opened an empty segment already, so this looks
    /// at the newest segment that has entries.
    pub fn last_turn_tokens(&self) -> u32 {
        let Some(segment) = self
            .segments
            .iter()
            .rev()
            .find(|segment| !segment.entries.is_empty())
        else {
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

/// Lowercased words of at least three characters, minus common stopwords.
fn terms(text: &str) -> BTreeSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| token.chars().count() >= 3)
        .map(|token| token.to_lowercase())
        .filter(|token| !STOPWORDS.contains(&token.as_str()))
        .collect()
}

/// Whether the input names a recorded artifact, by full path or file name.
fn references(input: &str, path: &str) -> bool {
    if input.contains(path) {
        return true;
    }
    match path.rsplit(['/', '\\']).next() {
        Some(name) if name.chars().count() >= 3 => input.contains(name),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(session: &mut Session, input: &str, text: &str) {
        session.record_turn(TurnRecord {
            input,
            text,
            tools: &[],
            artifacts: &[],
            verified: false,
        });
    }

    #[test]
    fn first_turn_opens_a_segment_and_keeps_order() {
        let mut session = Session::new();
        record(&mut session, "fix the parser", "done");

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
        record(&mut session, "first topic", "ok");
        assert!(session.seal_current());
        assert!(!session.seal_current());
        record(&mut session, "second topic", "ok");

        assert_eq!(session.segments().len(), 2);
        assert_eq!(session.segments()[0].state, SegmentState::Sealed);
        assert_eq!(session.segments()[1].topic, "second topic");
        assert_eq!(session.history().len(), 4);
    }

    #[test]
    fn a_turn_without_text_still_records_the_input() {
        let mut session = Session::new();
        record(&mut session, "unanswered", "");

        let history = session.history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, "unanswered");
    }

    #[test]
    fn topic_keeps_the_original_language_and_is_bounded() {
        let mut session = Session::new();
        let long = "修复解析器".repeat(40);
        record(&mut session, &long, "done");

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
        session.record_turn(TurnRecord {
            input: "read it",
            text: "it parses",
            tools: &[summary],
            artifacts: &[],
            verified: false,
        });

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

#[cfg(test)]
mod segmentation_tests {
    use super::*;

    fn record(session: &mut Session, input: &str) {
        session.record_turn(TurnRecord {
            input,
            text: "done",
            tools: &[],
            artifacts: &[],
            verified: false,
        });
    }

    #[test]
    fn a_new_topic_seals_the_segment() {
        let mut session = Session::new();
        record(&mut session, "fix the parser bug");

        let change = session.begin_turn("write a haiku about the sea");

        assert_eq!(change.sealed, Some(0));
        assert_eq!(change.reason.as_deref(), Some("topic"));
        assert_eq!(session.segments().len(), 2);
        assert_eq!(session.segments()[0].state, SegmentState::Sealed);
        assert_eq!(session.segments()[1].state, SegmentState::Open);
    }

    #[test]
    fn a_follow_up_keeps_the_segment() {
        let mut session = Session::new();
        record(&mut session, "fix the parser bug");

        let change = session.begin_turn("the parser bug is still there");

        assert_eq!(change, SegmentChange::default());
        assert_eq!(session.segments().len(), 1);
    }

    #[test]
    fn an_artifact_reference_keeps_the_segment() {
        let mut session = Session::new();
        session.record_turn(TurnRecord {
            input: "fix the parser",
            text: "done",
            tools: &[],
            artifacts: &["src/widget.rs".to_owned()],
            verified: false,
        });

        let change = session.begin_turn("open src/widget.rs");

        assert!(change.sealed.is_none());
        assert_eq!(session.segments().len(), 1);
    }

    #[test]
    fn a_verified_turn_seals_before_the_next_turn() {
        let mut session = Session::new();
        session.record_turn(TurnRecord {
            input: "fix the parser",
            text: "done",
            tools: &[],
            artifacts: &[],
            verified: true,
        });

        let change = session.begin_turn("fix the parser again");

        assert_eq!(change.reason.as_deref(), Some("verified"));
        assert_eq!(change.sealed, Some(0));
    }

    #[test]
    fn the_turn_cap_seals_the_segment() {
        let mut session = Session::new();
        for _ in 0..SEGMENT_TURN_CAP {
            session.begin_turn("keep going");
            record(&mut session, "keep going");
        }

        let change = session.begin_turn("keep going again");

        assert_eq!(change.reason.as_deref(), Some("size"));
        assert_eq!(change.sealed, Some(0));
    }
}
