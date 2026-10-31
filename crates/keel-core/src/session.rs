use std::collections::{BTreeMap, BTreeSet};
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

/// Where a fact lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactScope {
    Session,
    Global,
}

/// One extracted statement before its scope is decided.
struct FactCandidate {
    key: String,
    statement: String,
    durable: bool,
    confirmed: bool,
    segment_id: u32,
}

/// How often a durable constraint has been stated and how.
#[derive(Debug, Clone)]
struct Candidate {
    statement: String,
    segments: u32,
}

/// One durable statement extracted from a sealed segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub key: String,
    pub statement: String,
    pub scope: FactScope,
    pub confirmed: bool,
    pub segment_id: u32,
}

/// Compressed form of a sealed segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentRecord {
    pub goal: String,
    pub result: String,
    pub changes: Vec<String>,
    pub pending: Vec<String>,
    pub verified: bool,
    pub turns: u32,
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
    pub pending: Vec<String>,
    pub verified: bool,
    /// Present only when the sufficiency gate passed.
    pub record: Option<SegmentRecord>,
}

/// A segment that could not be compressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressionSkip {
    pub segment_id: u32,
    pub missing: Vec<String>,
}

/// What `begin_turn` did to the segment list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SegmentChange {
    pub sealed: Option<u32>,
    pub opened: Option<u32>,
    pub reason: Option<String>,
    pub skipped: Option<CompressionSkip>,
    /// Keys promoted to global scope at this seal.
    pub promoted: Vec<String>,
}

/// Everything one finished turn contributes to the session.
pub struct TurnRecord<'a> {
    pub input: &'a str,
    pub text: &'a str,
    pub tools: &'a [ToolSummary],
    pub artifacts: &'a [String],
    pub pending: &'a [String],
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
    facts: Vec<Fact>,
    global_facts: Vec<Fact>,
    candidates: BTreeMap<String, Candidate>,
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
        let (skipped, promoted) = self.seal_and_record();
        let opened = self.open_segment(input);
        SegmentChange {
            sealed: Some(sealed),
            opened: Some(opened),
            reason: Some(reason),
            skipped,
            promoted,
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
        segment.pending.extend(turn.pending.iter().cloned());
        segment.verified = turn.verified;
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
            pending: Vec::new(),
            verified: false,
            record: None,
        });
        id
    }

    /// Seals the open segment, builds its record when the gate passes, and
    /// sediments its facts.
    fn seal_and_record(&mut self) -> (Option<CompressionSkip>, Vec<String>) {
        let (skipped, facts) = {
            let Some(segment) = self.segments.last_mut() else {
                return (None, Vec::new());
            };
            segment.state = SegmentState::Sealed;
            let id = segment.id;
            let facts = extract_facts(segment);
            let outcome = build_record(segment);
            let skipped = match outcome {
                Ok(record) => {
                    segment.record = Some(record);
                    None
                }
                Err(missing) => Some(CompressionSkip {
                    segment_id: id,
                    missing,
                }),
            };
            (skipped, facts)
        };
        (skipped, self.apply_facts(facts))
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

    fn apply_facts(&mut self, candidates: Vec<FactCandidate>) -> Vec<String> {
        let mut promoted = Vec::new();
        for candidate in candidates {
            // A durable constraint is only a candidate: it must repeat in a
            // second segment before it becomes global. A different statement
            // for the same key resets the count and demotes it.
            let scope = if candidate.durable {
                let entry = self
                    .candidates
                    .entry(candidate.key.clone())
                    .or_insert_with(|| Candidate {
                        statement: candidate.statement.clone(),
                        segments: 0,
                    });
                if entry.statement == candidate.statement {
                    entry.segments = entry.segments.saturating_add(1);
                } else {
                    entry.statement = candidate.statement.clone();
                    entry.segments = 1;
                }
                if entry.segments >= 2 {
                    FactScope::Global
                } else {
                    FactScope::Session
                }
            } else {
                FactScope::Session
            };

            let fact = Fact {
                key: candidate.key,
                statement: candidate.statement,
                scope,
                confirmed: candidate.confirmed,
                segment_id: candidate.segment_id,
            };
            // A newer statement for the same key replaces the old one, even
            // when it moves scope.
            self.facts.retain(|existing| existing.key != fact.key);
            self.global_facts
                .retain(|existing| existing.key != fact.key);
            if scope == FactScope::Global {
                promoted.push(fact.key.clone());
                self.global_facts.push(fact);
            } else {
                self.facts.push(fact);
            }
        }
        promoted
    }

    /// Session-scoped facts.
    pub fn facts(&self) -> &[Fact] {
        &self.facts
    }

    /// Facts promoted to global scope in this process.
    pub fn global_facts(&self) -> &[Fact] {
        &self.global_facts
    }

    /// Global facts first, then session facts.
    pub fn all_facts(&self) -> Vec<Fact> {
        let mut all = self.global_facts.clone();
        all.extend(self.facts.iter().cloned());
        all
    }

    /// Seals the open segment. Returns whether one was sealed.
    pub fn seal_current(&mut self) -> bool {
        let Some(segment) = self.segments.last() else {
            return false;
        };
        if segment.state == SegmentState::Sealed {
            return false;
        }
        let _ = self.seal_and_record();
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
            .filter(|segment| segment.record.is_none())
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

    /// Records of sealed segments, oldest first.
    pub fn records(&self) -> Vec<SegmentRecord> {
        self.segments
            .iter()
            .filter_map(|segment| segment.record.clone())
            .collect()
    }
}

/// Cues that mark a user sentence as a constraint.
const CONSTRAINT_CUES: &[&str] = &[
    "always",
    "never",
    "from now on",
    "must",
    "do not",
    "don't",
    "不要",
    "必须",
    "禁止",
    "务必",
    "总是",
    "永远",
    "以后",
    "始终",
];

/// Cues that make a constraint durable, so it may become global.
const DURABLE_CUES: &[&str] = &[
    "always",
    "never",
    "from now on",
    "总是",
    "永远",
    "以后",
    "始终",
];

/// Markers that tie a sentence to this task, so it is never durable.
const SCOPING_MARKERS: &[&str] = &[
    "this task",
    "this time",
    "for now",
    "here",
    "本次",
    "这次",
    "这里",
    "暂时",
    "当前",
    "这个任务",
];

/// Extracts facts from structure the kernel already has: user constraints,
/// changed artifacts and verified segments.
fn extract_facts(segment: &Segment) -> Vec<FactCandidate> {
    let mut candidates: Vec<FactCandidate> = Vec::new();

    for entry in &segment.entries {
        if entry.message.role != MessageRole::User {
            continue;
        }
        for sentence in sentences(&entry.message.content) {
            if let Some((key, durable)) = parse_constraint(&sentence) {
                upsert_candidate(
                    &mut candidates,
                    FactCandidate {
                        key: format!("constraint:{key}"),
                        statement: clip_statement(&sentence),
                        durable,
                        confirmed: false,
                        segment_id: segment.id,
                    },
                );
            }
        }
    }

    for path in &segment.artifacts {
        upsert_candidate(
            &mut candidates,
            FactCandidate {
                key: format!("artifact:{path}"),
                statement: format!("modified {path}"),
                durable: false,
                confirmed: true,
                segment_id: segment.id,
            },
        );
    }

    if segment.verified {
        upsert_candidate(
            &mut candidates,
            FactCandidate {
                key: format!("verified:{}", segment.topic),
                statement: format!("verified: {}", segment.topic),
                durable: false,
                confirmed: true,
                segment_id: segment.id,
            },
        );
    }

    candidates
}

fn upsert_candidate(candidates: &mut Vec<FactCandidate>, candidate: FactCandidate) {
    candidates.retain(|existing| existing.key != candidate.key);
    candidates.push(candidate);
}

fn sentences(text: &str) -> Vec<String> {
    text.split(|character: char| {
        matches!(
            character,
            '.' | '!' | '?' | '。' | '！' | '？' | '\n' | ';' | '；'
        )
    })
    .map(str::trim)
    .filter(|sentence| !sentence.is_empty())
    .map(str::to_owned)
    .collect()
}

/// Returns a stable key and whether the constraint is durable.
///
/// The cue is removed, so "always use tabs" and "never use tabs" share a key
/// and the newer statement replaces the older one.
fn parse_constraint(sentence: &str) -> Option<(String, bool)> {
    let trimmed = sentence.trim();
    // Questions are never instructions.
    if trimmed.ends_with('?') || trimmed.ends_with('？') {
        return None;
    }
    let lower = trimmed.to_lowercase();
    // "In this task" / "for now" ties the sentence to the current work.
    if SCOPING_MARKERS.iter().any(|marker| lower.contains(marker)) {
        return None;
    }
    // First-person narration ("I always use tabs") is a description.
    if lower.starts_with("i ") || lower.starts_with("we ") || lower.starts_with('我') {
        return None;
    }
    let lower = lower.strip_prefix("please ").unwrap_or(&lower).trim_start();

    // The cue must open the sentence; a cue in the middle is commentary.
    if !CONSTRAINT_CUES.iter().any(|cue| lower.starts_with(*cue)) {
        return None;
    }
    let durable = DURABLE_CUES.iter().any(|cue| lower.starts_with(*cue));
    let mut rest = lower.to_owned();
    for cue in CONSTRAINT_CUES {
        rest = rest.replace(cue, " ");
    }
    let key = rest
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if key.is_empty() {
        return None;
    }
    Some((key, durable))
}

fn clip_statement(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= 200 {
        return flat;
    }
    let mut clipped: String = flat.chars().take(200).collect();
    clipped.push_str("...");
    clipped
}

/// Builds the compressed record, or reports the fields the gate requires.
fn build_record(segment: &Segment) -> Result<SegmentRecord, Vec<String>> {
    let mut missing = Vec::new();
    let goal = segment.topic.trim().to_owned();
    if goal.is_empty() {
        missing.push("goal".to_owned());
    }

    let result = segment
        .entries
        .iter()
        .rev()
        .find(|entry| entry.message.role == MessageRole::Assistant)
        .map(|entry| result_text(&entry.message.content))
        .unwrap_or_default();
    if result.is_empty() {
        missing.push("result".to_owned());
    }

    if !missing.is_empty() {
        return Err(missing);
    }
    Ok(SegmentRecord {
        goal,
        result,
        changes: segment.artifacts.iter().cloned().collect(),
        pending: segment.pending.clone(),
        verified: segment.verified,
        turns: segment.turns,
    })
}

/// The assistant text without the `[tools]` block, flattened and bounded.
fn result_text(content: &str) -> String {
    let body = content.split("\n\n[tools]").next().unwrap_or(content);
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= 400 {
        return flat;
    }
    let mut clipped: String = flat.chars().take(400).collect();
    clipped.push_str("...");
    clipped
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
            pending: &[],
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
        // The sealed segment is now a record, not raw entries.
        assert_eq!(session.history().len(), 2);
        assert_eq!(session.records().len(), 1);
        assert_eq!(session.records()[0].goal, "first topic");
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
            pending: &[],
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
            pending: &[],
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
            pending: &[],
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
            pending: &[],
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

#[cfg(test)]
mod record_tests {
    use super::*;

    fn turn<'a>(input: &'a str, text: &'a str, artifacts: &'a [String]) -> TurnRecord<'a> {
        TurnRecord {
            input,
            text,
            tools: &[],
            artifacts,
            pending: &[],
            verified: false,
        }
    }

    #[test]
    fn a_sealed_segment_becomes_a_record() {
        let mut session = Session::new();
        let artifacts = vec!["src/parser.rs".to_owned()];
        session.record_turn(turn("fix the parser bug", "fixed it", &artifacts));
        assert!(session.seal_current());

        let record = &session.records()[0];
        assert_eq!(record.goal, "fix the parser bug");
        assert_eq!(record.result, "fixed it");
        assert_eq!(record.changes, artifacts);
        assert!(session.history().is_empty());
    }

    #[test]
    fn a_segment_without_a_result_is_not_compressed() {
        let mut session = Session::new();
        session.record_turn(turn("unanswered question", "", &[]));

        let change = session.begin_turn("something else entirely");

        assert!(change.sealed.is_some());
        assert_eq!(
            change.skipped.as_ref().map(|skip| skip.missing.clone()),
            Some(vec!["result".to_owned()])
        );
        assert!(session.records().is_empty());
        assert_eq!(session.history().len(), 1);
    }

    #[test]
    fn the_record_result_drops_the_tool_block() {
        assert_eq!(
            result_text("fixed it\n\n[tools]\n- read_file path=a.rs -> ok"),
            "fixed it"
        );
    }
}

#[cfg(test)]
mod fact_tests {
    use super::*;

    fn record(session: &mut Session, input: &str, artifacts: &[String]) {
        session.record_turn(TurnRecord {
            input,
            text: "done",
            tools: &[],
            artifacts,
            pending: &[],
            verified: false,
        });
    }

    #[test]
    fn a_durable_constraint_needs_two_segments() {
        let mut session = Session::new();
        record(&mut session, "always use tabs", &[]);

        let first = session.begin_turn("write a haiku about the sea");
        assert!(first.promoted.is_empty());
        assert!(session.global_facts().is_empty());
        assert_eq!(session.facts().len(), 1);
        assert_eq!(session.facts()[0].scope, FactScope::Session);

        record(&mut session, "always use tabs", &[]);
        let second = session.begin_turn("another topic entirely");

        assert_eq!(second.promoted, vec!["constraint:use tabs".to_owned()]);
        assert_eq!(session.global_facts().len(), 1);
        assert_eq!(session.global_facts()[0].scope, FactScope::Global);
        assert!(session.global_facts()[0]
            .statement
            .contains("always use tabs"));
        assert!(!session
            .facts()
            .iter()
            .any(|fact| fact.key == "constraint:use tabs"));
    }

    #[test]
    fn a_question_is_not_a_constraint() {
        let mut session = Session::new();
        record(&mut session, "do you always use tabs?", &[]);
        session.begin_turn("write a haiku about the sea");

        assert!(session.facts().is_empty());
        assert!(session.global_facts().is_empty());
    }

    #[test]
    fn a_scoped_sentence_is_not_a_constraint() {
        let mut session = Session::new();
        record(&mut session, "always use tabs in this task", &[]);
        session.begin_turn("write a haiku about the sea");

        assert!(session.facts().is_empty());
    }

    #[test]
    fn a_mid_sentence_cue_is_not_a_constraint() {
        let mut session = Session::new();
        record(&mut session, "the linter always runs on save", &[]);
        session.begin_turn("write a haiku about the sea");

        assert!(session.facts().is_empty());
    }

    #[test]
    fn first_person_narration_is_not_a_constraint() {
        let mut session = Session::new();
        record(&mut session, "I always use tabs", &[]);
        session.begin_turn("write a haiku about the sea");

        assert!(session.facts().is_empty());
    }

    #[test]
    fn a_transient_artifact_stays_in_session() {
        let mut session = Session::new();
        let artifacts = vec!["src/widget.rs".to_owned()];
        record(&mut session, "fix the widget", &artifacts);

        let change = session.begin_turn("write a haiku about the sea");

        assert!(change.promoted.is_empty());
        assert!(session.global_facts().is_empty());
        assert_eq!(session.facts().len(), 1);
        assert_eq!(session.facts()[0].key, "artifact:src/widget.rs");
        assert_eq!(session.facts()[0].scope, FactScope::Session);
    }

    #[test]
    fn a_non_durable_constraint_stays_in_session() {
        let mut session = Session::new();
        record(&mut session, "must keep the tests untouched", &[]);

        session.begin_turn("write a haiku about the sea");

        assert!(session.global_facts().is_empty());
        assert_eq!(session.facts().len(), 1);
        assert!(session.facts()[0].statement.contains("must keep"));
    }

    #[test]
    fn a_contradiction_demotes_the_promoted_fact() {
        let mut session = Session::new();
        record(&mut session, "always use tabs", &[]);
        session.begin_turn("write a haiku about the sea");
        record(&mut session, "always use tabs", &[]);
        session.begin_turn("another topic entirely");
        assert_eq!(session.global_facts().len(), 1);

        record(&mut session, "never use tabs", &[]);
        session.begin_turn("what is the weather today");

        assert!(session.global_facts().is_empty());
        assert!(session
            .facts()
            .iter()
            .any(|fact| fact.statement.contains("never use tabs")));
    }
}
