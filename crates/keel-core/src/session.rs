use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::context::is_cjk;
use crate::error::KernelError;
use crate::secret::redact_secrets;

use crate::message::{Message, MessageRole};

/// Turns in one segment before the kernel seals it.
pub const SEGMENT_TURN_CAP: u32 = 12;
/// Text tokens in one segment before the kernel seals it, even while the
/// topic continues. Long answers and tool summaries otherwise stay verbatim in
/// the active segment until the turn cap, which is what dilutes attention.
pub const SEGMENT_TOKEN_CAP: u32 = 12_000;
/// Input-to-segment term overlap below this is a topic change.
pub const TOPIC_OVERLAP_FLOOR: f32 = 0.2;
/// Input-to-segment overlap needed to recall an absorbed segment.
pub const RECALL_MIN_SCORE: f32 = 0.34;
/// Lower bar when the input explicitly asks for earlier history.
pub const RECALL_INTENT_SCORE: f32 = 0.20;
/// Segments recalled per turn.
pub const RECALL_LIMIT: usize = 2;
/// A query this large is a task brief, not a memory question. It only recalls
/// when it also asks for history.
const RECALL_MAX_QUERY_TERMS: usize = 24;
/// Characters kept from each message in a recall excerpt.
pub const RECALL_EXCERPT_CHARS: usize = 300;

/// Words ignored when comparing a new input with a segment.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "this", "that", "from", "into", "your", "you", "are", "was",
    "has", "have", "still", "also", "please", "then", "them", "they",
];

/// Question and meta words removed from a recall query only. Segment terms
/// keep them so segmentation stays unchanged; IDF makes them cheap anyway.
const QUERY_STOPWORDS: &[&str] = &[
    "what",
    "which",
    "where",
    "when",
    "who",
    "whom",
    "whose",
    "why",
    "how",
    "is",
    "are",
    "do",
    "does",
    "did",
    "can",
    "could",
    "would",
    "should",
    "will",
    "shall",
    "may",
    "might",
    "me",
    "my",
    "mine",
    "our",
    "ours",
    "us",
    "it",
    "its",
    "these",
    "those",
    "there",
    "here",
    "about",
    "again",
    "earlier",
    "previous",
    "previously",
    "mentioned",
    "remember",
    "recall",
    "said",
    "told",
    "say",
    "know",
    "thing",
    "things",
    "something",
    "anything",
    "last",
    "time",
    "before",
    "after",
    "now",
    "just",
    "very",
    "really",
    "actually",
    "thanks",
];

/// Common Chinese meta bigrams removed from a recall query. A bigram that is
/// rare inside the session is kept; this list only covers words that would
/// otherwise match many topics.
const CJK_QUERY_STOP_BIGRAMS: &[&str] = &[
    "之前", "刚才", "上次", "前面", "那个", "这个", "什么", "怎么", "记得", "还记", "提到", "提过",
    "说过", "一下", "我们", "你们", "你还", "他们", "她们", "它们", "可以", "是不", "不是", "但是",
    "然后", "现在", "当时", "这里", "那里", "哪些", "哪个", "怎样", "如何", "为什", "请问", "告诉",
    "知道", "觉得", "时候", "一个", "没有", "就是", "还是", "已经", "如果", "因为", "所以", "而且",
    "的话", "说的",
];

/// Input markers that mean "look at earlier history". They lower the recall
/// bar; they do not create a match on their own.
const RECALL_MARKERS: &[&str] = &[
    "remember",
    "recalled",
    "recall",
    "earlier",
    "mentioned",
    "previous",
    "previously",
    "before",
    "again",
    "记得",
    "之前说",
    "之前提",
    "之前那",
    "之前我们",
    "之前你",
    "刚才",
    "上次",
    "前面说",
    "前面提",
    "提过",
    "说过",
    "回忆",
    "想起来",
];

/// Lifecycle of one topic segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FactScope {
    Session,
    Workspace,
    Global,
}

/// One extracted statement before its scope is decided.
struct FactCandidate {
    key: String,
    statement: String,
    /// Subject stated by an update cue, when the fact replaces an older value.
    subject: Option<String>,
    durable: bool,
    explicit: bool,
    workspace: bool,
    global: bool,
    occurrences: u32,
    confirmed: bool,
    segment_id: u32,
}

/// One parsed constraint and the scope hints it carries.
struct Constraint {
    key: String,
    subject: Option<String>,
    durable: bool,
    explicit: bool,
    workspace: bool,
    global: bool,
}

/// How often a durable constraint has been stated and how.
#[derive(Debug, Clone)]
struct Candidate {
    statement: String,
    occurrences: u32,
}

/// One durable statement extracted from a sealed segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    pub key: String,
    pub statement: String,
    pub scope: FactScope,
    pub confirmed: bool,
    pub segment_id: u32,
}

/// Compressed form of a sealed segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentRecord {
    pub goal: String,
    pub result: String,
    pub changes: Vec<String>,
    pub pending: Vec<String>,
    pub verified: bool,
    pub turns: u32,
    /// Excluded from the context projection because facts cover its changes.
    /// The record and its transcript stay in storage.
    pub folded: bool,
    /// Summary revision whose text includes this record.
    pub absorbed_in: Option<u32>,
    /// Model-written memo of this segment, kept for recall after the record
    /// is absorbed into the rolling summary. Absent in older snapshots.
    #[serde(default)]
    pub memo: Option<String>,
    /// A bounded exchange kept for recall when the summary covers this
    /// segment and its verbatim entries are dropped. Absent in older
    /// snapshots.
    #[serde(default)]
    pub excerpt: Option<String>,
}

/// Rolling model-written summary of the whole session.
///
/// One field, rewritten at every segment seal. The structured records and the
/// transcript stay in storage; this is the only compressed view of the older
/// history that enters the context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub text: String,
    pub revision: u32,
    /// Segment ids included in this revision or an earlier one.
    pub covers: Vec<u32>,
    /// Token budget the text was written against.
    pub budget_tokens: u32,
}

/// An absorbed segment pulled back by a lexical match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecalledSegment {
    pub segment_id: u32,
    pub record: SegmentRecord,
    /// Best matching user and assistant exchange, clipped and redacted.
    pub excerpt: Option<String>,
    /// IDF-weighted overlap with the query, after unknown terms are ignored,
    /// in basis points (0..=10_000). It is an integer so the event stays `Eq`.
    pub score_bp: u32,
    /// Query terms found in this segment.
    pub matched: u32,
    /// Query terms after stopword filtering.
    pub query_terms: u32,
    /// Query terms that occur somewhere in the session.
    pub known_terms: u32,
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

/// The snapshot format this kernel writes and can read.
///
/// A host may keep snapshots across kernel upgrades. `restore` refuses a
/// snapshot written by a newer format instead of silently dropping fields it
/// does not understand.
pub const SNAPSHOT_VERSION: u32 = 1;

fn snapshot_version() -> u32 {
    SNAPSHOT_VERSION
}

/// A serializable view of one session, for a host store.
///
/// The snapshot carries the planes the kernel needs to continue a session:
/// segments, entries, records, the memo and facts. Fact candidates are
/// transient extraction state and are not carried.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Format version; absent in snapshots written before the field existed.
    #[serde(default = "snapshot_version")]
    pub format_version: u32,
    pub segments: Vec<SegmentSnapshot>,
    pub next_segment_id: u32,
    pub last_verified: bool,
    pub facts: Vec<Fact>,
    pub workspace_facts: Vec<Fact>,
    pub global_facts: Vec<Fact>,
    pub summary: Option<SessionSummary>,
    pub next_summary_revision: u32,
    pub summary_missing: bool,
}

impl Default for SessionSnapshot {
    fn default() -> Self {
        Self {
            format_version: SNAPSHOT_VERSION,
            segments: Vec::new(),
            next_segment_id: 0,
            last_verified: false,
            facts: Vec::new(),
            workspace_facts: Vec::new(),
            global_facts: Vec::new(),
            summary: None,
            next_summary_revision: 0,
            summary_missing: false,
        }
    }
}

/// One segment inside a session snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentSnapshot {
    pub id: u32,
    pub topic: String,
    pub state: SegmentState,
    pub entries: Vec<EntrySnapshot>,
    pub turns: u32,
    pub terms: Vec<String>,
    pub artifacts: Vec<String>,
    pub pending: Vec<String>,
    pub verified: bool,
    pub record: Option<SegmentRecord>,
}

/// One message inside a segment snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntrySnapshot {
    pub role: String,
    pub content: String,
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
    /// Keys removed by an explicit "forget" at this seal.
    pub forgotten: Vec<String>,
    /// Keys promoted above session scope at this seal.
    pub promoted: Vec<String>,
    /// Records excluded from the context because facts cover them.
    pub folded: Vec<u32>,
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
    workspace_facts: Vec<Fact>,
    global_facts: Vec<Fact>,
    candidates: BTreeMap<String, Candidate>,
    summary: Option<SessionSummary>,
    next_summary_revision: u32,
    summary_missing: bool,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Captures the state a host needs to continue this session in another
    /// process.
    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            format_version: SNAPSHOT_VERSION,
            segments: self
                .segments
                .iter()
                .map(|segment| SegmentSnapshot {
                    id: segment.id,
                    topic: segment.topic.clone(),
                    state: segment.state,
                    entries: segment
                        .entries
                        .iter()
                        .map(|entry| EntrySnapshot {
                            role: match entry.message.role {
                                MessageRole::User => "user",
                                MessageRole::Assistant => "assistant",
                                MessageRole::System => "system",
                                MessageRole::Tool => "tool",
                            }
                            .to_owned(),
                            content: entry.message.content.clone(),
                        })
                        .collect(),
                    turns: segment.turns,
                    terms: segment.terms.iter().cloned().collect(),
                    artifacts: segment.artifacts.iter().cloned().collect(),
                    pending: segment.pending.clone(),
                    verified: segment.verified,
                    record: segment.record.clone(),
                })
                .collect(),
            next_segment_id: self.next_segment_id,
            last_verified: self.last_verified,
            facts: self.facts.clone(),
            workspace_facts: self.workspace_facts.clone(),
            global_facts: self.global_facts.clone(),
            summary: self.summary.clone(),
            next_summary_revision: self.next_summary_revision,
            summary_missing: self.summary_missing,
        }
    }

    /// Rebuilds a session from a host store snapshot.
    ///
    /// Fails when the snapshot was written by a newer format; a host that
    /// upgrades the kernel should migrate or keep the old kernel for that
    /// session rather than guess.
    pub fn restore(snapshot: SessionSnapshot) -> Result<Self, KernelError> {
        if snapshot.format_version > SNAPSHOT_VERSION {
            return Err(KernelError::Unsupported {
                feature: format!(
                    "session snapshot version {} (this kernel supports {SNAPSHOT_VERSION})",
                    snapshot.format_version
                ),
            });
        }
        let segments: Vec<Segment> = snapshot
            .segments
            .into_iter()
            .map(|segment| Segment {
                id: segment.id,
                topic: segment.topic,
                state: segment.state,
                entries: segment
                    .entries
                    .into_iter()
                    .map(|entry| Entry {
                        message: Arc::new(match entry.role.as_str() {
                            "user" => Message::user(entry.content),
                            "system" => Message::system(entry.content),
                            "tool" => Message::new(MessageRole::Tool, entry.content),
                            // A document written before the cap may hold a
                            // very long body; the history copy is bounded on
                            // load too.
                            _ => Message::assistant(cap_entry(entry.content)),
                        }),
                    })
                    .collect(),
                turns: segment.turns,
                terms: segment.terms.into_iter().collect(),
                artifacts: segment.artifacts.into_iter().collect(),
                pending: segment.pending,
                verified: segment.verified,
                record: segment.record,
            })
            .collect();
        let next_segment_id = snapshot
            .next_segment_id
            .max(segments.iter().map(|segment| segment.id).max().unwrap_or(0));
        let mut session = Self {
            segments,
            next_segment_id,
            last_verified: snapshot.last_verified,
            facts: snapshot.facts,
            workspace_facts: snapshot.workspace_facts,
            global_facts: snapshot.global_facts,
            candidates: BTreeMap::new(),
            summary: snapshot.summary,
            next_summary_revision: snapshot.next_summary_revision,
            summary_missing: snapshot.summary_missing,
        };
        // A document written before this rule kept the verbatim entries of
        // segments an earlier summary already covered. Drop them on load; the
        // next save writes the smaller document.
        session.drop_absorbed_transcripts();
        Ok(session)
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
        let (skipped, forgotten, promoted, folded) = self.seal_and_record();
        let opened = self.open_segment(input);
        SegmentChange {
            sealed: Some(sealed),
            opened: Some(opened),
            reason: Some(reason),
            skipped,
            forgotten,
            promoted,
            folded,
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
                message: Arc::new(Message::assistant(cap_entry(body))),
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
    fn seal_and_record(&mut self) -> (Option<CompressionSkip>, Vec<String>, Vec<String>, Vec<u32>) {
        let (skipped, facts, forgotten_terms) = {
            let Some(segment) = self.segments.last_mut() else {
                return (None, Vec::new(), Vec::new(), Vec::new());
            };
            segment.state = SegmentState::Sealed;
            let id = segment.id;
            let facts = extract_facts(segment);
            let forgotten_terms = extract_forgotten(segment);
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
            (skipped, facts, forgotten_terms)
        };
        let mut forgotten = Vec::new();
        for term in forgotten_terms {
            forgotten.extend(self.forget_matching(&term));
        }
        let promoted = self.apply_facts(facts);
        let folded = self.fold_covered_records();
        (skipped, forgotten, promoted, folded)
    }

    /// Folds records whose changes are all represented by artifact facts.
    ///
    /// The record and its transcript stay in storage; only the context
    /// projection drops it. The newest non-empty segment is never folded.
    fn fold_covered_records(&mut self) -> Vec<u32> {
        let newest = self
            .segments
            .iter()
            .rposition(|segment| !segment.entries.is_empty());
        let covered: BTreeSet<String> = self
            .facts
            .iter()
            .chain(self.workspace_facts.iter())
            .chain(self.global_facts.iter())
            .filter_map(|fact| fact.key.strip_prefix("artifact:").map(str::to_owned))
            .collect();

        let mut folded = Vec::new();
        for (index, segment) in self.segments.iter_mut().enumerate() {
            if Some(index) == newest {
                continue;
            }
            let Some(record) = segment.record.as_mut() else {
                continue;
            };
            if record.folded
                || record.absorbed_in.is_some()
                || !record.pending.is_empty()
                || record.changes.is_empty()
            {
                continue;
            }
            if record.changes.iter().all(|path| covered.contains(path)) {
                record.folded = true;
                folded.push(segment.id);
            }
        }
        folded
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
        let text_tokens = segment.entries.iter().fold(0u32, |total, entry| {
            total.saturating_add(entry.message.estimated_tokens())
        });
        if text_tokens >= SEGMENT_TOKEN_CAP {
            return Some("size".to_owned());
        }

        if FOLLOWUP_MARKERS
            .iter()
            .any(|marker| contains_marker(input, marker))
        {
            return None;
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
            // Repetition is the only implicit promotion path, and it stops at
            // workspace scope. Global needs an explicit, global-scoped request.
            let repeated = if candidate.durable || candidate.explicit {
                let entry = self
                    .candidates
                    .entry(candidate.key.clone())
                    .or_insert_with(|| Candidate {
                        statement: candidate.statement.clone(),
                        occurrences: 0,
                    });
                if entry.statement == candidate.statement {
                    entry.occurrences = entry.occurrences.saturating_add(candidate.occurrences);
                } else {
                    entry.statement = candidate.statement.clone();
                    entry.occurrences = candidate.occurrences;
                }
                entry.occurrences >= 2
            } else {
                false
            };

            let scope = if candidate.explicit && candidate.global {
                FactScope::Global
            } else if candidate.explicit || candidate.workspace || (candidate.durable && repeated) {
                FactScope::Workspace
            } else {
                FactScope::Session
            };

            // An explicit update replaces the fact for the same subject, which
            // may have a different key: "项目代号是蓝鲸" then
            // "项目代号改成海鸥". History keeps both statements in storage.
            if let Some(subject) = &candidate.subject {
                let prefix = format!("constraint:{subject}");
                for facts in [
                    &mut self.facts,
                    &mut self.workspace_facts,
                    &mut self.global_facts,
                ] {
                    facts.retain(|existing| !existing.key.starts_with(&prefix));
                }
            }

            let fact = Fact {
                key: candidate.key,
                statement: candidate.statement,
                scope,
                confirmed: candidate.confirmed,
                segment_id: candidate.segment_id,
            };
            // A newer statement for the same key replaces the old one, even
            // when it moves scope; that is the rollback path.
            self.facts.retain(|existing| existing.key != fact.key);
            self.workspace_facts
                .retain(|existing| existing.key != fact.key);
            self.global_facts
                .retain(|existing| existing.key != fact.key);
            match scope {
                FactScope::Global => {
                    promoted.push(fact.key.clone());
                    self.global_facts.push(fact);
                }
                FactScope::Workspace => {
                    promoted.push(fact.key.clone());
                    self.workspace_facts.push(fact);
                }
                FactScope::Session => self.facts.push(fact),
            }
        }
        promoted
    }

    /// Removes every fact whose key or statement mentions `subject`.
    pub fn forget_matching(&mut self, subject: &str) -> Vec<String> {
        let needle = subject.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        let mut removed = Vec::new();
        for store in [
            &mut self.facts,
            &mut self.workspace_facts,
            &mut self.global_facts,
        ] {
            store.retain(|fact| {
                let matches = fact.key.to_lowercase().contains(&needle)
                    || fact.statement.to_lowercase().contains(&needle);
                if matches {
                    removed.push(fact.key.clone());
                }
                !matches
            });
        }
        removed
    }

    /// Session-scoped facts.
    pub fn facts(&self) -> &[Fact] {
        &self.facts
    }

    /// Facts promoted to workspace scope.
    pub fn workspace_facts(&self) -> &[Fact] {
        &self.workspace_facts
    }

    /// Facts promoted to global scope in this process.
    pub fn global_facts(&self) -> &[Fact] {
        &self.global_facts
    }

    /// Global facts first, then workspace, then session.
    pub fn all_facts(&self) -> Vec<Fact> {
        let mut all = self.global_facts.clone();
        all.extend(self.workspace_facts.iter().cloned());
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
        // The newest non-empty segment stays verbatim even after it seals,
        // so the model always sees the latest exchange as it was written.
        let newest = self
            .segments
            .iter()
            .rposition(|segment| !segment.entries.is_empty());
        self.segments
            .iter()
            .enumerate()
            .filter(|(index, segment)| Some(*index) == newest || segment.record.is_none())
            .flat_map(|(_, segment)| segment.entries.iter())
            .filter(|entry| {
                matches!(
                    entry.message.role,
                    MessageRole::User | MessageRole::Assistant
                )
            })
            .map(|entry| entry.message.clone())
            .collect()
    }

    /// Verbatim messages of one segment, for a summarizer.
    pub fn sealed_transcript(&self, segment_id: u32) -> Option<Vec<Arc<Message>>> {
        let segment = self
            .segments
            .iter()
            .find(|segment| segment.id == segment_id)?;
        Some(
            segment
                .entries
                .iter()
                .map(|entry| entry.message.clone())
                .collect(),
        )
    }

    /// The current rolling summary, if one has been written.
    pub fn summary(&self) -> Option<&SessionSummary> {
        self.summary.as_ref()
    }

    /// True when the last summary update failed; records stay visible.
    pub fn summary_missing(&self) -> bool {
        self.summary_missing
    }

    /// Record of one segment, cloned for use outside the session lock.
    pub fn record(&self, segment_id: u32) -> Option<SegmentRecord> {
        self.segments
            .iter()
            .find(|segment| segment.id == segment_id)
            .and_then(|segment| segment.record.clone())
    }

    /// Records not yet represented in the summary and safe to absorb.
    ///
    /// The newest segment stays in detail, and records with open items stay
    /// structured: unfinished state is never handed to the model to rewrite.
    pub fn absorbable_records(&self, exclude: Option<u32>) -> Vec<(u32, SegmentRecord)> {
        let newest = self
            .segments
            .iter()
            .rposition(|segment| !segment.entries.is_empty());
        self.segments
            .iter()
            .enumerate()
            .filter(|(index, _)| Some(*index) != newest)
            .filter(|(_, segment)| Some(segment.id) != exclude)
            .filter_map(|(_, segment)| segment.record.as_ref().map(|record| (segment.id, record)))
            .filter(|(_, record)| record.absorbed_in.is_none() && record.pending.is_empty())
            .map(|(id, record)| (id, record.clone()))
            .collect()
    }

    /// Stores a rewritten summary and marks the records it absorbed.
    ///
    /// Coverage is cumulative: a later revision keeps every segment an earlier
    /// one covered. Returns the new revision, or `None` for empty text.
    pub fn set_summary(&mut self, text: &str, covers: &[u32], budget_tokens: u32) -> Option<u32> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let revision = self.next_summary_revision.saturating_add(1);
        self.next_summary_revision = revision;
        let mut merged = self
            .summary
            .as_ref()
            .map(|summary| summary.covers.clone())
            .unwrap_or_default();
        for id in covers {
            if !merged.contains(id) {
                merged.push(*id);
            }
        }
        self.summary = Some(SessionSummary {
            text: text.to_owned(),
            revision,
            covers: merged,
            budget_tokens,
        });
        self.summary_missing = false;
        for segment in &mut self.segments {
            if covers.contains(&segment.id) {
                if let Some(record) = segment.record.as_mut() {
                    record.absorbed_in = Some(revision);
                }
            }
        }
        self.drop_absorbed_transcripts();
        Some(revision)
    }

    /// Drops the verbatim transcript of segments a summary already covers.
    ///
    /// The record, its memo and the summary are what recall and the context
    /// projection read; the entries are only the summarizer's input and are
    /// no longer needed once the summary includes the segment. The newest
    /// non-empty segment keeps its entries, so the latest exchange stays
    /// verbatim.
    pub(crate) fn drop_absorbed_transcripts(&mut self) {
        let Some(summary) = self.summary.as_ref() else {
            return;
        };
        let covered: BTreeSet<u32> = summary.covers.iter().copied().collect();
        let newest = self
            .segments
            .iter()
            .rposition(|segment| !segment.entries.is_empty());
        for (index, segment) in self.segments.iter_mut().enumerate() {
            if Some(index) == newest {
                continue;
            }
            let absorbed = segment
                .record
                .as_ref()
                .is_some_and(|record| record.absorbed_in.is_some());
            if absorbed && covered.contains(&segment.id) {
                // Recall may still ask for a detail; keep one bounded
                // exchange before the transcript goes away.
                if segment
                    .record
                    .as_ref()
                    .is_some_and(|record| record.excerpt.is_none())
                {
                    if let Some(record) = segment.record.as_mut() {
                        let goal: String = record.goal.chars().take(RECALL_EXCERPT_CHARS).collect();
                        record.excerpt =
                            Some(format!("user: {goal}\nassistant: {}", record.result));
                    }
                }
                segment.entries.clear();
            }
        }
    }

    /// Stores the model-written memo of one segment.
    ///
    /// The record is the fallback when the summary drops the segment; the
    /// memo is what recall returns, so it is bounded and never empty.
    pub fn set_segment_memo(&mut self, segment_id: u32, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let bound = 1200usize;
        let memo: String = text.chars().take(bound).collect();
        for segment in &mut self.segments {
            if segment.id != segment_id {
                continue;
            }
            if let Some(record) = segment.record.as_mut() {
                record.memo = Some(memo);
            }
            break;
        }
    }

    /// Marks that the latest update failed; the previous summary stays valid.
    pub fn mark_summary_missing(&mut self) {
        self.summary_missing = true;
    }

    /// Records of sealed segments that are no longer the newest content.
    pub fn records(&self) -> Vec<SegmentRecord> {
        let newest = self
            .segments
            .iter()
            .rposition(|segment| !segment.entries.is_empty());
        self.segments
            .iter()
            .enumerate()
            .filter(|(index, _)| Some(*index) != newest)
            .filter_map(|(_, segment)| segment.record.clone())
            .filter(|record| !record.folded && record.absorbed_in.is_none())
            .collect()
    }

    /// Segments already folded into the rolling summary, oldest first.
    ///
    /// The summary is a rewritten view; this index keeps the one-line shape of
    /// every covered segment in context so a topic the rewrite dropped is
    /// still discoverable, at about two dozen tokens per segment.
    pub fn covered_index(&self) -> Vec<(u32, SegmentRecord)> {
        self.segments
            .iter()
            .filter_map(|segment| {
                segment
                    .record
                    .clone()
                    .filter(|record| record.absorbed_in.is_some())
                    .map(|record| (segment.id, record))
            })
            .collect()
    }

    /// Absorbed segments that match the current input lexically.
    ///
    /// This is deterministic recall, not a model or an index. It reuses the
    /// segment's term signature and artifact paths, and adds a short excerpt
    /// from the best matching turn so a detail that never reached the memo can
    /// still come back into view.
    pub fn recall(&self, input: &str) -> Vec<RecalledSegment> {
        let input_terms = query_terms(input);
        if input_terms.is_empty() {
            return Vec::new();
        }
        let asks_history = input.contains('?')
            || input.contains('？')
            || RECALL_MARKERS
                .iter()
                .any(|marker| contains_marker(input, marker));
        if !asks_history && input_terms.len() > RECALL_MAX_QUERY_TERMS {
            return Vec::new();
        }
        let document_count = self
            .segments
            .iter()
            .filter(|segment| !segment.terms.is_empty())
            .count()
            .max(1) as f32;
        let mut weights: BTreeMap<String, f32> = BTreeMap::new();
        let mut known_weight = 0.0f32;
        let mut known_terms = 0u32;
        for term in &input_terms {
            let df = self
                .segments
                .iter()
                .filter(|segment| segment.terms.contains(term))
                .count() as f32;
            let weight = ((document_count + 1.0) / (df + 1.0)).ln() + 1.0;
            if df > 0.0 {
                known_weight += weight;
                known_terms = known_terms.saturating_add(1);
            }
            weights.insert(term.clone(), weight);
        }
        if known_weight <= 0.0 {
            return Vec::new();
        }
        let threshold = if RECALL_MARKERS
            .iter()
            .any(|marker| contains_marker(input, marker))
        {
            RECALL_INTENT_SCORE
        } else {
            RECALL_MIN_SCORE
        };
        let query_term_count = input_terms.len() as u32;
        let newest = self
            .segments
            .iter()
            .rposition(|segment| !segment.entries.is_empty());
        let mut scored: Vec<(f32, usize, RecalledSegment)> = Vec::new();
        for (index, segment) in self.segments.iter().enumerate() {
            if Some(index) == newest {
                continue;
            }
            let Some(record) = segment.record.as_ref() else {
                continue;
            };
            // Only segments the projection has already dropped need recall.
            if record.absorbed_in.is_none() && !record.folded {
                continue;
            }
            // Open items stay visible through the record block.
            if !record.pending.is_empty() {
                continue;
            }
            let matched_terms: Vec<&String> = input_terms
                .iter()
                .filter(|term| segment.terms.contains(*term))
                .collect();
            let matched = matched_terms.len() as u32;
            let matched_weight: f32 = matched_terms.iter().map(|term| weights[*term]).sum();
            let mut score = matched_weight / known_weight;
            let artifact_hit = record
                .changes
                .iter()
                .any(|path| input.contains(path.as_str()));
            if artifact_hit {
                score = score.max(0.9);
            }
            // A hit on a term present in every segment says nothing; require
            // one term that is actually selective, unless an artifact names it.
            let selective = matched_terms.iter().any(|term| weights[*term] > 1.0);
            if score < threshold || (matched == 0 && !artifact_hit) || (!artifact_hit && !selective)
            {
                continue;
            }
            scored.push((
                score,
                index,
                RecalledSegment {
                    segment_id: segment.id,
                    record: record.clone(),
                    excerpt: recall_excerpt(segment, &weights).or_else(|| record.excerpt.clone()),
                    score_bp: (score * 10_000.0).round().clamp(0.0, 10_000.0) as u32,
                    matched,
                    query_terms: query_term_count,
                    known_terms,
                },
            ));
        }
        scored.sort_by(|left, right| {
            right
                .0
                .partial_cmp(&left.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right.1.cmp(&left.1))
        });
        scored
            .into_iter()
            .take(RECALL_LIMIT)
            .map(|(_, _, recalled)| recalled)
            .collect()
    }
}

/// Terms used for recall queries. Segment terms keep the full signature;
/// only the query drops question and meta words.
fn query_terms(text: &str) -> BTreeSet<String> {
    let mut terms = terms(text);
    terms.retain(|term| {
        if term.chars().count() == 2 && term.chars().all(is_cjk) {
            !CJK_QUERY_STOP_BIGRAMS.contains(&term.as_str())
        } else {
            !QUERY_STOPWORDS.contains(&term.as_str())
        }
    });
    terms
}

/// Best matching user turn and its answer, clipped for recall.
fn recall_excerpt(segment: &Segment, weights: &BTreeMap<String, f32>) -> Option<String> {
    let mut best: Option<(f32, usize)> = None;
    for (index, entry) in segment.entries.iter().enumerate() {
        if entry.message.role != MessageRole::User {
            continue;
        }
        let entry_terms = terms(&entry.message.content);
        let score: f32 = entry_terms
            .iter()
            .filter_map(|term| weights.get(term))
            .sum();
        if score <= 0.0 {
            continue;
        }
        let better = match best {
            Some((best_score, _)) => score > best_score,
            None => true,
        };
        if better {
            best = Some((score, index));
        }
    }
    let (_, index) = best?;
    let user = clip_chars(
        &redact_secrets(&segment.entries[index].message.content),
        RECALL_EXCERPT_CHARS,
    );
    let assistant = segment.entries[index + 1..]
        .iter()
        .find(|entry| entry.message.role == MessageRole::Assistant)
        .map(|entry| {
            clip_chars(
                &redact_secrets(&entry.message.content),
                RECALL_EXCERPT_CHARS,
            )
        });
    Some(match assistant {
        Some(text) => format!("user: {user}\nassistant: {text}"),
        None => format!("user: {user}"),
    })
}

fn clip_chars(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut clipped: String = flat.chars().take(max).collect();
    clipped.push_str("...");
    clipped
}

/// Matches a marker on word boundaries for ASCII text, by substring for CJK.
fn contains_marker(text: &str, marker: &str) -> bool {
    if !marker.is_ascii() {
        return text.contains(marker);
    }
    let words: Vec<&str> = text
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let parts: Vec<&str> = marker.split_whitespace().collect();
    if parts.is_empty() {
        return false;
    }
    words
        .windows(parts.len())
        .any(|window| window == parts.as_slice())
}

/// Cues that mark a user sentence as a constraint.
const CONSTRAINT_CUES: &[&str] = &[
    "always",
    "never",
    "from now on",
    "must",
    "do not",
    "don't",
    "remember",
    "记住",
    "请记住",
    "不要",
    "必须",
    "禁止",
    "务必",
    "总是",
    "永远",
    "以后",
    "始终",
];

/// Phrases that ask the kernel to keep something, so one statement is enough.
const EXPLICIT_MARKERS: &[&str] = &["remember", "记住", "请记住", "from now on", "以后都"];

/// Phrases that scope a constraint to this repository or project.
const WORKSPACE_CUES: &[&str] = &[
    "this repo",
    "this project",
    "this workspace",
    "这个项目",
    "这个仓库",
    "本项目",
    "本仓库",
];

/// Phrases that ask for a constraint across every project.
const GLOBAL_CUES: &[&str] = &[
    "everywhere",
    "all projects",
    "globally",
    "所有项目",
    "任何项目",
    "全局",
];

/// Phrases that retract a fact.
const FORGET_MARKERS: &[&str] = &["forget about", "forget", "忘记", "忘掉", "不要记住"];

/// Phrases that ask for an answer now rather than state a durable preference.
///
/// "不要查文件，直接回答：我的项目代号是什么" opens with a constraint cue but
/// is about this turn's reply. Storing it would record the user's question as
/// a fact, which the multi-session evaluation observed four times.
const REPLY_MARKERS: &[&str] = &[
    "直接回答",
    "只回答",
    "回答我",
    "告诉我",
    "just answer",
    "only answer",
    "answer me",
    "tell me",
];

/// Phrases that state a new value for an existing subject.
///
/// The text before the phrase is the subject. A fact whose key starts with the
/// same subject is replaced, which is what "项目代号改成海鸥" needs: without
/// it the old value and the new one both stay in the fact table.
const UPDATE_MARKERS: &[&str] = &[
    "改成",
    "改为",
    "换成",
    "现在叫",
    "更名为",
    "changed to",
    "renamed to",
    "now called",
    "is now",
    "now is",
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

/// Phrases that refer back to the conversation, so the segment continues.
const FOLLOWUP_MARKERS: &[&str] = &[
    "刚才",
    "刚刚",
    "之前",
    "上面",
    "前面",
    "继续",
    "earlier",
    "before",
    "continue",
    "go on",
    "what did we",
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
        // Count one occurrence per turn even when the sentence repeats.
        let mut seen_in_turn: BTreeSet<(String, String)> = BTreeSet::new();
        for sentence in sentences(&entry.message.content) {
            let Some(constraint) = parse_constraint(&sentence) else {
                continue;
            };
            let key = format!("constraint:{}", constraint.key);
            let statement = clip_statement(&sentence);
            if !seen_in_turn.insert((key.clone(), statement.clone())) {
                continue;
            }
            if let Some(existing) = candidates.iter_mut().find(|candidate| candidate.key == key) {
                if existing.statement == statement {
                    existing.occurrences = existing.occurrences.saturating_add(1);
                    existing.durable |= constraint.durable;
                    existing.explicit |= constraint.explicit;
                    existing.workspace |= constraint.workspace;
                    existing.global |= constraint.global;
                    existing.subject = existing.subject.take().or(constraint.subject);
                } else {
                    existing.statement = statement;
                    existing.occurrences = 1;
                    existing.durable = constraint.durable;
                    existing.explicit = constraint.explicit;
                    existing.workspace = constraint.workspace;
                    existing.global = constraint.global;
                    existing.subject = constraint.subject;
                }
            } else {
                candidates.push(FactCandidate {
                    key,
                    statement,
                    subject: constraint.subject,
                    durable: constraint.durable,
                    explicit: constraint.explicit,
                    workspace: constraint.workspace,
                    global: constraint.global,
                    occurrences: 1,
                    confirmed: false,
                    segment_id: segment.id,
                });
            }
        }
    }

    for path in &segment.artifacts {
        upsert_candidate(
            &mut candidates,
            FactCandidate {
                key: format!("artifact:{path}"),
                statement: format!("modified {path}"),
                subject: None,
                durable: false,
                explicit: false,
                workspace: false,
                global: false,
                occurrences: 1,
                confirmed: true,
                segment_id: segment.id,
            },
        );
    }

    // A verified conversation turn is not a fact about the world. Only a
    // verified segment that produced artifacts records a completed task; a
    // Q&A turn would otherwise store its own question as a "verified" fact and
    // crowd the memory block.
    if segment.verified && !segment.artifacts.is_empty() {
        upsert_candidate(
            &mut candidates,
            FactCandidate {
                key: format!("verified:{}", segment.topic),
                statement: format!("verified: {}", segment.topic),
                subject: None,
                durable: false,
                explicit: false,
                workspace: false,
                global: false,
                occurrences: 1,
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

/// Explicit retractions such as "forget about tabs".
fn extract_forgotten(segment: &Segment) -> Vec<String> {
    let mut terms = Vec::new();
    for entry in &segment.entries {
        if entry.message.role != MessageRole::User {
            continue;
        }
        for sentence in sentences(&entry.message.content) {
            if let Some(term) = parse_forget(&sentence) {
                terms.push(term);
            }
        }
    }
    terms
}

fn parse_forget(sentence: &str) -> Option<String> {
    let trimmed = sentence.trim();
    if trimmed.ends_with('?') || trimmed.ends_with('？') {
        return None;
    }
    let lower = trimmed.to_lowercase();
    let lower = lower.strip_prefix("please ").unwrap_or(&lower).trim_start();
    let marker = FORGET_MARKERS
        .iter()
        .find(|marker| lower.starts_with(*marker))?;
    let rest = lower[marker.len()..].trim_start_matches(|character: char| {
        character.is_whitespace() || matches!(character, ':' | ',' | '：' | '，')
    });
    let subject = rest
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| token.chars().count() >= 3)
        .collect::<Vec<_>>()
        .join(" ");
    if subject.is_empty() {
        return None;
    }
    Some(subject)
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
fn parse_constraint(sentence: &str) -> Option<Constraint> {
    let trimmed = sentence.trim();
    // Questions are never instructions.
    if trimmed.ends_with('?') || trimmed.ends_with('？') {
        return None;
    }
    let lower = trimmed.to_lowercase();
    // "In this task" / "for now" ties the sentence to the current work.
    if SCOPING_MARKERS
        .iter()
        .any(|marker| contains_marker(&lower, marker))
    {
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
    // "Do not read files, just answer: what is my code name" constrains this
    // reply, not the session.
    if REPLY_MARKERS
        .iter()
        .any(|marker| contains_marker(lower, marker))
    {
        return None;
    }
    let durable = DURABLE_CUES.iter().any(|cue| lower.starts_with(*cue));
    let explicit = EXPLICIT_MARKERS.iter().any(|cue| lower.starts_with(*cue));
    let workspace = WORKSPACE_CUES.iter().any(|cue| contains_marker(lower, cue));
    let global = GLOBAL_CUES.iter().any(|cue| contains_marker(lower, cue));
    let mut rest = lower.to_owned();
    for cue in CONSTRAINT_CUES {
        rest = rest.replace(cue, " ");
    }
    for cue in WORKSPACE_CUES.iter().chain(GLOBAL_CUES.iter()) {
        rest = rest.replace(cue, " ");
    }
    let subject = fact_subject(&rest);
    let key = tokens(&rest);
    if key.is_empty() {
        return None;
    }
    Some(Constraint {
        key,
        subject,
        durable,
        explicit,
        workspace,
        global,
    })
}

/// The subject of a statement that names a new value, normalized like a key.
fn fact_subject(rest: &str) -> Option<String> {
    let mut earliest: Option<usize> = None;
    for marker in UPDATE_MARKERS {
        let Some(position) = marker_position(rest, marker) else {
            continue;
        };
        earliest = Some(earliest.map_or(position, |current| current.min(position)));
    }
    let subject = tokens(&rest[..earliest?]);
    if subject.chars().count() < 2 {
        return None;
    }
    Some(subject)
}

/// Finds an ASCII marker on word boundaries, or a non-ASCII marker anywhere.
fn marker_position(text: &str, marker: &str) -> Option<usize> {
    if !marker.is_ascii() {
        return text.find(marker);
    }
    let mut from = 0;
    while let Some(index) = text[from..].find(marker) {
        let start = from + index;
        let end = start + marker.len();
        let before = text[..start]
            .chars()
            .next_back()
            .is_none_or(|character| !character.is_alphanumeric());
        let after = text[end..]
            .chars()
            .next()
            .is_none_or(|character| !character.is_alphanumeric());
        if before && after {
            return Some(start);
        }
        from = end;
    }
    None
}

/// Normalized word tokens, the shared form of a key and a subject.
fn tokens(text: &str) -> String {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| token.chars().count() >= 3)
        .collect::<Vec<_>>()
        .join(" ")
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
        folded: false,
        absorbed_in: None,
        memo: None,
        excerpt: None,
    })
}

/// Longest assistant body kept in a segment entry. The live turn saw the
/// whole text and the tool output; the history copy is bounded so one long
/// turn cannot grow the session document without limit.
pub const MAX_ENTRY_CHARS: usize = 12_000;

/// Suffix of the note `cap_entry` appends. An entry that already ends with it
/// is bounded, so a restore leaves it alone instead of capping it again and
/// replacing the dropped count with the length of the note itself.
const CAP_NOTE_SUFFIX: &str = " characters kept out of this turn's transcript)";

/// Keeps the head of an entry body and names what was dropped, so a reader
/// can tell a short turn from a clipped one.
pub(crate) fn cap_entry(text: String) -> String {
    if text.ends_with(CAP_NOTE_SUFFIX) {
        return text;
    }
    let count = text.chars().count();
    if count <= MAX_ENTRY_CHARS {
        return text;
    }
    let head: String = text.chars().take(MAX_ENTRY_CHARS).collect();
    format!(
        "{head}\n(+{} characters kept out of this turn's transcript)",
        count - MAX_ENTRY_CHARS
    )
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
    let mut terms = BTreeSet::new();
    for token in text.split(|character: char| !character.is_alphanumeric()) {
        if token.is_empty() {
            continue;
        }
        if token.chars().any(is_cjk) {
            // CJK has no spaces, so compare character bigrams.
            let characters: Vec<char> = token.chars().collect();
            if characters.len() == 1 {
                terms.insert(token.to_lowercase());
            }
            for pair in characters.windows(2) {
                terms.insert(pair.iter().collect());
            }
        } else if token.chars().count() >= 3 {
            let lower = token.to_lowercase();
            if !STOPWORDS.contains(&lower.as_str()) {
                terms.insert(lower);
            }
        }
    }
    terms
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
    fn a_snapshot_restores_the_derived_state() {
        let mut session = Session::new();
        record(&mut session, "fix the parser", "done");
        session.set_summary("memo text", &[1], 256);

        let restored = Session::restore(session.snapshot()).expect("snapshot restores");

        assert_eq!(restored.segments().len(), session.segments().len());
        assert_eq!(restored.history().len(), session.history().len());
        assert_eq!(restored.segments()[0].entries.len(), 2);
        assert_eq!(
            restored.summary().map(|summary| summary.text.as_str()),
            Some("memo text")
        );
    }

    #[test]
    fn a_long_turn_body_is_capped_in_the_transcript() {
        let mut session = Session::new();
        let body = "x".repeat(MAX_ENTRY_CHARS * 2);
        session.record_turn(TurnRecord {
            input: "write a lot",
            text: &body,
            tools: &[],
            artifacts: &[],
            pending: &[],
            verified: false,
        });

        let assistant = session.segments()[0]
            .entries
            .iter()
            .find(|entry| entry.message.role == MessageRole::Assistant)
            .expect("the assistant entry");
        assert!(
            assistant.message.content.chars().count() <= MAX_ENTRY_CHARS + 80,
            "the entry stays near the cap"
        );
        assert!(
            assistant
                .message
                .content
                .contains("kept out of this turn's transcript"),
            "the clipped body says so"
        );
    }

    #[test]
    fn restoring_caps_an_old_long_entry() {
        let mut session = Session::new();
        record(&mut session, "write a lot", "done");
        let mut snapshot = session.snapshot();
        snapshot.segments[0].entries[1].content = "y".repeat(MAX_ENTRY_CHARS * 2);

        let restored = Session::restore(snapshot).expect("the snapshot restores");

        let assistant = restored.segments()[0]
            .entries
            .iter()
            .find(|entry| entry.message.role == MessageRole::Assistant)
            .expect("the assistant entry");
        assert!(assistant.message.content.chars().count() <= MAX_ENTRY_CHARS + 80);
    }

    #[test]
    fn restoring_a_capped_entry_keeps_its_note() {
        let mut session = Session::new();
        record(&mut session, "write a lot", "done");
        let mut snapshot = session.snapshot();
        snapshot.segments[0].entries[1].content = "y".repeat(MAX_ENTRY_CHARS * 2);

        let first = Session::restore(snapshot).expect("the snapshot restores");
        let capped = first.snapshot();
        let second = Session::restore(capped.clone()).expect("the capped snapshot restores");

        assert_eq!(
            serde_json::to_value(second.snapshot()).expect("a snapshot encodes"),
            serde_json::to_value(capped).expect("a snapshot encodes"),
            "a second restore does not cap the note itself"
        );
    }

    #[test]
    fn a_covered_segment_drops_its_verbatim_transcript() {
        let mut session = Session::new();
        record(&mut session, "first task", "first answer");
        assert!(session.seal_current());
        record(&mut session, "second task", "second answer");
        let sealed_id = session.segments()[0].id;
        assert_eq!(session.segments()[0].entries.len(), 2);

        session.set_summary("memo", &[sealed_id], 256);

        assert!(
            session.segments()[0].entries.is_empty(),
            "the covered segment keeps its record, not its transcript"
        );
        assert_eq!(
            session.segments()[1].entries.len(),
            2,
            "the newest segment stays verbatim"
        );
        assert_eq!(session.history().len(), 2, "history is the newest segment");
    }

    #[test]
    fn restoring_prunes_a_segment_an_earlier_summary_covered() {
        let mut session = Session::new();
        record(&mut session, "first task", "first answer");
        assert!(session.seal_current());
        record(&mut session, "second task", "second answer");
        let sealed_id = session.segments()[0].id;
        let mut snapshot = session.snapshot();
        // A document written before the rule: the summary covers the segment
        // but its entries are still stored.
        snapshot.summary = Some(SessionSummary {
            text: "memo".to_owned(),
            revision: 1,
            covers: vec![sealed_id],
            budget_tokens: 256,
        });
        if let Some(record) = snapshot.segments[0].record.as_mut() {
            record.absorbed_in = Some(1);
        }
        assert_eq!(snapshot.segments[0].entries.len(), 2);

        let restored = Session::restore(snapshot).expect("the snapshot restores");

        assert!(restored.segments()[0].entries.is_empty());
        assert_eq!(restored.segments()[1].entries.len(), 2);
    }

    #[test]
    fn a_snapshot_from_a_newer_format_is_rejected() {
        let mut snapshot = Session::new().snapshot();
        snapshot.format_version = SNAPSHOT_VERSION + 1;
        let error = Session::restore(snapshot).expect_err("a newer format must fail");
        assert!(matches!(error, KernelError::Unsupported { .. }));
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
        // The newest segment is verbatim; the older sealed one is a record.
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

    #[test]
    fn a_large_segment_seals_before_the_turn_cap() {
        // One turn is capped, so the size seal is reached by several long
        // turns instead of one oversized one.
        let text = "x".repeat(MAX_ENTRY_CHARS);
        let mut session = Session::new();
        for _ in 0..4 {
            session.record_turn(TurnRecord {
                input: "keep going",
                text: &text,
                tools: &[],
                artifacts: &[],
                pending: &[],
                verified: false,
            });
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

        let record = session.segments()[0]
            .record
            .as_ref()
            .expect("sealed record");
        assert_eq!(record.goal, "fix the parser bug");
        assert_eq!(record.result, "fixed it");
        assert_eq!(record.changes, artifacts);
        // The newest segment still shows verbatim; its record is not used yet.
        assert_eq!(session.history().len(), 2);
        assert!(session.records().is_empty());
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
        assert_eq!(session.workspace_facts().len(), 1);
        assert!(session.global_facts().is_empty());
        assert_eq!(session.workspace_facts()[0].scope, FactScope::Workspace);
        assert!(session.workspace_facts()[0]
            .statement
            .contains("always use tabs"));
        assert!(!session
            .facts()
            .iter()
            .any(|fact| fact.key == "constraint:use tabs"));
    }

    #[test]
    fn two_mentions_in_one_segment_are_enough() {
        let mut session = Session::new();
        record(&mut session, "always use tabs", &[]);
        record(&mut session, "always use tabs", &[]);

        session.begin_turn("what is the weather today");

        assert_eq!(session.workspace_facts().len(), 1);
    }

    #[test]
    fn an_explicit_remember_promotes_immediately() {
        let mut session = Session::new();
        record(&mut session, "remember to use tabs", &[]);

        session.begin_turn("what is the weather today");

        assert_eq!(session.workspace_facts().len(), 1);
        assert!(session.workspace_facts()[0].statement.contains("remember"));
    }

    #[test]
    fn a_workspace_phrase_promotes_to_workspace() {
        let mut session = Session::new();
        record(&mut session, "always use tabs in this repo", &[]);

        session.begin_turn("what is the weather today");

        assert_eq!(session.workspace_facts().len(), 1);
        assert!(session.global_facts().is_empty());
    }

    #[test]
    fn an_explicit_global_request_promotes_to_global() {
        let mut session = Session::new();
        record(&mut session, "remember to use tabs everywhere", &[]);

        session.begin_turn("what is the weather today");

        assert_eq!(session.global_facts().len(), 1);
        assert!(session.workspace_facts().is_empty());
    }

    #[test]
    fn forget_removes_a_promoted_fact() {
        let mut session = Session::new();
        record(&mut session, "remember to use tabs everywhere", &[]);
        session.begin_turn("write a haiku about the sea");
        assert_eq!(session.global_facts().len(), 1);

        record(&mut session, "forget about tabs", &[]);
        let change = session.begin_turn("what is the weather today");

        assert_eq!(change.forgotten, vec!["constraint:use tabs".to_owned()]);
        assert!(session.global_facts().is_empty());
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
    fn a_reply_instruction_is_not_a_constraint() {
        let mut session = Session::new();
        record(
            &mut session,
            "不要查文件，直接回答：我的项目代号是什么",
            &[],
        );
        record(
            &mut session,
            "do not read files, just answer: what is my code name",
            &[],
        );
        session.begin_turn("write a haiku about the sea");

        assert!(session.facts().is_empty());
        assert!(session.workspace_facts().is_empty());
        assert!(session.global_facts().is_empty());
    }

    #[test]
    fn an_update_replaces_the_fact_for_the_same_subject() {
        let mut session = Session::new();
        record(&mut session, "请记住：项目代号是蓝鲸", &[]);
        session.begin_turn("write a haiku about the sea");
        assert_eq!(session.workspace_facts().len(), 1);

        record(&mut session, "请记住：项目代号改成海鸥", &[]);
        session.begin_turn("what is the weather today");

        assert_eq!(session.workspace_facts().len(), 1);
        assert!(session.workspace_facts()[0].statement.contains("海鸥"));
        assert_eq!(session.workspace_facts()[0].scope, FactScope::Workspace);
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
    fn a_verified_answer_without_artifacts_is_not_a_fact() {
        let mut session = Session::new();
        session.record_turn(TurnRecord {
            input: "what is the project name",
            text: "the project is blue whale",
            tools: &[],
            artifacts: &[],
            pending: &[],
            verified: true,
        });

        session.begin_turn("next question");

        assert!(session.facts().is_empty());
    }

    #[test]
    fn a_verified_task_with_artifacts_is_a_fact() {
        let mut session = Session::new();
        let artifacts = vec!["notes.md".to_owned()];
        session.record_turn(TurnRecord {
            input: "write the notes",
            text: "notes written",
            tools: &[],
            artifacts: &artifacts,
            pending: &[],
            verified: true,
        });

        session.begin_turn("next question");

        assert!(session
            .facts()
            .iter()
            .any(|fact| fact.key.starts_with("verified:")));
    }

    #[test]
    fn a_contradiction_demotes_the_promoted_fact() {
        let mut session = Session::new();
        record(&mut session, "remember to use tabs everywhere", &[]);
        session.begin_turn("write a haiku about the sea");
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

#[cfg(test)]
mod chinese_segmentation_tests {
    use super::*;

    fn record(session: &mut Session, input: &str) {
        session.record_turn(TurnRecord {
            input,
            text: "好",
            tools: &[],
            artifacts: &[],
            pending: &[],
            verified: false,
        });
    }

    #[test]
    fn a_chinese_follow_up_keeps_the_segment() {
        let mut session = Session::new();
        record(&mut session, "请介绍一下你自己");

        let change = session.begin_turn("我们刚才说了什么？");

        assert!(change.sealed.is_none());
        assert_eq!(session.segments().len(), 1);
    }

    #[test]
    fn a_chinese_topic_change_seals_the_segment() {
        let mut session = Session::new();
        record(&mut session, "请介绍一下你自己");

        let change = session.begin_turn("明天天气怎么样");

        assert_eq!(change.reason.as_deref(), Some("topic"));
        assert_eq!(session.segments().len(), 2);
    }
}

#[cfg(test)]
mod folding_tests {
    use super::*;

    fn record(session: &mut Session, input: &str, artifacts: &[String], pending: &[String]) {
        session.record_turn(TurnRecord {
            input,
            text: "done",
            tools: &[],
            artifacts,
            pending,
            verified: false,
        });
    }

    #[test]
    fn a_covered_record_is_folded_and_still_absorbable() {
        let mut session = Session::new();
        let artifacts = vec!["a.py".to_owned()];
        record(&mut session, "write a.py", &artifacts, &[]);
        session.begin_turn("alpha beta");

        record(&mut session, "gamma delta", &[], &[]);
        let change = session.begin_turn("epsilon zeta");

        assert_eq!(change.folded, vec![0]);
        assert!(session.records().is_empty(), "folded records leave context");
        let absorbable = session.absorbable_records(change.sealed);
        assert_eq!(absorbable.len(), 1);
        assert_eq!(absorbable[0].0, 0);

        let revision = session.set_summary("summary", &[0], 512);
        assert_eq!(revision, Some(1));
        assert!(session.absorbable_records(change.sealed).is_empty());
        assert_eq!(session.summary().unwrap().covers, vec![0]);
    }

    #[test]
    fn a_record_with_pending_is_not_absorbable() {
        let mut session = Session::new();
        let artifacts = vec!["a.py".to_owned()];
        let pending = vec!["blocked on review".to_owned()];
        record(&mut session, "write a.py", &artifacts, &pending);
        session.begin_turn("alpha beta");

        record(&mut session, "gamma delta", &[], &[]);
        session.begin_turn("epsilon zeta");

        assert!(session.absorbable_records(None).is_empty());
    }

    #[test]
    fn a_new_summary_keeps_earlier_coverage() {
        let mut session = Session::new();
        assert_eq!(session.set_summary("one", &[0], 256), Some(1));
        assert_eq!(session.set_summary("two", &[1], 256), Some(2));

        let summary = session.summary().expect("summary");
        assert_eq!(summary.revision, 2);
        assert_eq!(summary.covers, vec![0, 1]);
    }

    #[test]
    fn an_absorbed_record_leaves_the_context() {
        let mut session = Session::new();
        record(&mut session, "first", &[], &[]);
        session.begin_turn("alpha beta");
        record(&mut session, "second", &[], &[]);
        session.begin_turn("gamma delta");

        assert_eq!(session.records().len(), 1);
        session.set_summary("summary", &[0], 256);
        assert!(session.records().is_empty());
    }

    #[test]
    fn a_record_with_pending_is_not_folded() {
        let mut session = Session::new();
        let artifacts = vec!["a.py".to_owned()];
        let pending = vec!["blocked on review".to_owned()];
        record(&mut session, "write a.py", &artifacts, &pending);
        session.begin_turn("alpha beta");

        record(&mut session, "gamma delta", &[], &[]);
        let change = session.begin_turn("epsilon zeta");

        assert!(change.folded.is_empty());
        assert_eq!(session.records().len(), 1);
    }

    #[test]
    fn a_record_without_facts_is_not_folded() {
        let mut session = Session::new();
        record(&mut session, "first", &[], &[]);
        session.begin_turn("alpha beta");

        record(&mut session, "gamma delta", &[], &[]);
        let change = session.begin_turn("epsilon zeta");

        assert!(change.folded.is_empty());
    }
}

#[cfg(test)]
mod recall_tests {
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

    fn absorbed_session() -> Session {
        let mut session = Session::new();
        record(&mut session, "plan the marlin config");
        session.begin_turn("alpha beta");
        record(&mut session, "alpha beta");
        session.begin_turn("gamma delta");
        session.set_summary("summary", &[0], 256);
        session
    }

    #[test]
    fn an_absorbed_topic_is_recalled_by_its_terms() {
        let session = absorbed_session();
        let recalled = session.recall("where is the marlin config");

        assert_eq!(recalled.len(), 1);
        assert_eq!(recalled[0].segment_id, 0);
        assert!(recalled[0].record.goal.contains("marlin"));
        assert!(recalled[0]
            .excerpt
            .as_deref()
            .unwrap_or_default()
            .contains("plan the marlin config"));
    }

    #[test]
    fn unrelated_input_recalls_nothing() {
        let session = absorbed_session();
        assert!(session.recall("what is the weather").is_empty());
    }

    #[test]
    fn a_paraphrased_question_ignores_unknown_terms() {
        let mut session = Session::new();
        record(
            &mut session,
            "the pasta i made was aglio e olio with garlic and olive oil",
        );
        session.begin_turn("alpha beta");
        record(&mut session, "alpha beta");
        session.begin_turn("gamma delta");
        session.set_summary("summary", &[0], 256);

        let recalled = session.recall("what was the pasta recipe i mentioned earlier");

        assert_eq!(recalled.len(), 1);
        assert_eq!(recalled[0].segment_id, 0);
        assert_eq!(recalled[0].query_terms, 2);
        assert_eq!(recalled[0].known_terms, 1);
        assert_eq!(recalled[0].matched, 1);
        assert!(recalled[0].score_bp >= 3_400);
        assert!(recalled[0]
            .excerpt
            .as_deref()
            .unwrap_or_default()
            .contains("aglio"));
    }

    #[test]
    fn a_chinese_paraphrase_recalls_the_topic() {
        let mut session = Session::new();
        record(&mut session, "意面的做法是蒜香橄榄油，只需要大蒜和橄榄油");
        session.begin_turn("今天下雨");
        record(&mut session, "今天下雨了");
        session.begin_turn("另一个话题");
        session.set_summary("summary", &[0], 256);

        let recalled = session.recall("之前说的意面配方还记得吗");

        assert_eq!(recalled.len(), 1);
        assert_eq!(recalled[0].segment_id, 0);
        assert!(recalled[0].matched >= 1);
        assert!(recalled[0].score_bp >= 3_400);
    }

    #[test]
    fn a_long_task_brief_does_not_recall() {
        let mut session = Session::new();
        record(&mut session, "the pasta i made was aglio e olio");
        session.begin_turn("alpha beta");
        record(&mut session, "alpha beta");
        session.begin_turn("gamma delta");
        session.set_summary("summary", &[0], 256);

        let brief = "继续写第2章。写之前用 todo 标记 in_progress，先读状态，写章节，更新状态，不要提问，不要跳章。".repeat(8);

        assert!(session.recall(&brief).is_empty());
    }

    #[test]
    fn a_term_in_every_segment_is_not_enough() {
        let mut session = Session::new();
        record(&mut session, "alpha beta");
        session.begin_turn("alpha gamma");
        record(&mut session, "alpha gamma");
        session.begin_turn("delta");
        session.set_summary("summary", &[0], 256);

        assert!(session.recall("alpha").is_empty());
    }

    #[test]
    fn a_record_still_in_the_projection_is_not_recalled() {
        let mut session = Session::new();
        record(&mut session, "plan the marlin config");
        session.begin_turn("alpha beta");
        record(&mut session, "alpha beta");

        assert!(session.recall("where is the marlin config").is_empty());
    }
}
