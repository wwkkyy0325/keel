use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keel_core::{
    detect_secret, estimate_tokens, redact_secrets, AcceptanceItem, Answer, ArgValue, BuildCtx,
    CancelToken, CommitCtx, CommitDecision, ContextMessage, ContextSnapshot, ErrorCode, Event,
    EventSink, InteractionId, InteractionRequest, InteractionResponse, KernelError, Message,
    MessageRole, ModelChunk, ModelError, ModelProvider, ModelRequest, ModelRole, OnTimeout, Policy,
    ReasoningLevel, SecretKind, SegmentRecord, Session, SessionSnapshot, TodoList, TodoState,
    TodoStatus, ToolArgs, ToolCallId, ToolOutcome, TurnEvent, TurnRecord, TurnState,
    VerificationMode, VerificationReport,
};

use crate::context::ContextBuilder;
use crate::dispatch::ToolDispatcher;
use crate::event_log::EventLog;
use crate::fsm::TurnFsm;
use crate::governor::{ContextGovernor, PressureLevel};
use crate::interaction::InteractionBroker;
use crate::interjection::{InterjectionPush, InterjectionQueue};
use crate::permission::PermissionEngine;
use crate::registry::ToolRegistry;
use crate::summarizer::{
    clip_to_tokens, memo_budget, transcript_text, trim_to_budget, Summarizer, MEMO_BUDGET_FLOOR,
    MIN_TRANSCRIPT_TOKENS, SUMMARY_INPUT_MARGIN,
};
use crate::summary::{compact_args, summarize_output};
use crate::verification::AcceptanceVerifier;

/// How long a model call may stay silent (connect, or between chunks).
const MODEL_CALL_TIMEOUT: Duration = Duration::from_secs(300);
const SUMMARY_TIMEOUT: Duration = Duration::from_secs(30);
/// Bounds how many times one turn restarts itself because the model stopped
/// while its task list still had unfinished items.
const MAX_CONTINUATIONS: u32 = 6;
use crate::TurnLimits;

/// Host-side mechanism switches used by ablations.
///
/// Defaults keep every mechanism on. These are evaluation controls, not
/// variant policy: they exist so one mechanism can be removed while the
/// prompt, model and workload stay fixed.
#[derive(Debug, Clone, Copy)]
pub struct KernelOptions {
    pub recall: bool,
    pub memo: bool,
    /// Bound live tool results with the same head/tail summary used for history.
    pub bounded_tools: bool,
    /// Keep the turn-start message list in the outcome. Off by default: the
    /// snapshot is large and only a host that persists it needs the copy.
    pub capture_context: bool,
}

impl Default for KernelOptions {
    fn default() -> Self {
        Self {
            recall: true,
            memo: true,
            bounded_tools: true,
            capture_context: false,
        }
    }
}

/// Host configuration that is not one of the seven policy questions.
pub struct TurnDriverConfig {
    pub events: Arc<EventLog>,
    pub permissions: Arc<PermissionEngine>,
    pub interactions: Arc<InteractionBroker>,
    pub system_prompt: String,
    pub max_output_tokens: u32,
    /// Model hard capacity; `context.budget_tokens` is the working set.
    pub context_window: u32,
    pub limits: TurnLimits,
    /// Kernel-owned task list. The host seeds it and the `todo` tool edits it.
    pub todos: Option<Arc<Mutex<TodoList>>>,
    pub options: KernelOptions,
}

const WRAP_UP: &str = "Context is nearly exhausted. Do not start new work. \
Finish the step in flight, then report: goal, done, pending, artifacts, decisions.";

const FINAL_CALL: &str = "This is the last model call for this turn and tools \
are disabled. Report what you completed, what is still pending, and the files \
or commands you changed. Do not claim a check ran unless you saw its output.";

#[derive(Default)]
struct TurnStats {
    model_calls: u32,
    tool_calls: u32,
    interactions: u32,
    deferred: Vec<InteractionId>,
    usage: keel_core::TokenUsage,
    reasoning: ReasoningLevel,
    consecutive_tool_failures: u32,
    empty_rounds: u32,
    previous_failed_call: Option<(String, ToolArgs)>,
    verification_reprompts: u32,
    continuations: u32,
    verification: Option<VerificationReport>,
    awaiting_wrap_up: bool,
    wrap_up_report: Option<String>,
    answer: Answer,
    tool_summaries: Vec<keel_core::ToolSummary>,
    context: Option<ContextSnapshot>,
}

struct AcceptancePlan {
    items: Vec<AcceptanceItem>,
    max_reprompts: u32,
}

/// Terminal result of one turn.
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    pub turn_id: keel_core::TurnId,
    pub state: TurnState,
    pub text: String,
    pub error: Option<KernelError>,
    pub deferred_interactions: Vec<InteractionId>,
    /// User messages that arrived too late for a safe point. The host shows
    /// them so they are not silently lost.
    pub late_interjections: Vec<String>,
    /// Secret-like input seen at turn start. The turn still runs; the host
    /// surfaces the notice with this turn's result.
    pub secret_warning: Option<SecretKind>,
    /// Model calls spent in this turn, including the final answer call and any
    /// summary call.
    pub model_calls: u32,
    /// Token usage summed over model calls in this turn.
    pub usage: keel_core::TokenUsage,
    /// Wall-clock duration of the turn.
    pub duration_ms: u64,
    /// Final kernel-owned reasoning level for this turn.
    pub reasoning_level: ReasoningLevel,
    /// Last acceptance report, when the host supplied a checklist.
    pub verification: Option<VerificationReport>,
    /// The turn-start message list, when `capture_context` is enabled.
    pub context: Option<ContextSnapshot>,
    /// Model text produced after the context wrap-up instruction, kept for
    /// the segment record. It is not a substitute for the final answer.
    pub wrap_up_report: Option<String>,
    /// User-visible result of the turn, separate from the transcript in
    /// `text`.
    pub answer: Answer,
}

impl TurnOutcome {
    pub fn is_done(&self) -> bool {
        self.state == TurnState::Done
    }

    pub fn is_cancelled(&self) -> bool {
        self.state == TurnState::Cancelled
    }
}

/// Drives one turn through Observe -> Solve -> Act -> Verify -> Converge.
pub struct TurnDriver<M> {
    model: M,
    tools: Arc<ToolRegistry>,
    context: ContextBuilder,
    policy: Arc<dyn Policy>,
    events: Arc<EventLog>,
    interactions: Arc<InteractionBroker>,
    dispatcher: ToolDispatcher,
    system_prompt: String,
    max_output_tokens: u32,
    limits: TurnLimits,
    workspace_root: PathBuf,
    acceptance: Option<AcceptancePlan>,
    session: Mutex<Session>,
    context_window: u32,
    output_reserve: u32,
    working_set: u32,
    /// Measured difference between reported input tokens and the kernel
    /// estimate: tool schemas and provider template overhead.
    overhead: AtomicU64,
    summarizer: Option<Arc<dyn Summarizer>>,
    /// Where long tool outputs are spilled. `None` uses
    /// `<workspace>/.keel/outputs`.
    outputs_dir: Option<PathBuf>,
    todos: Option<Arc<Mutex<TodoList>>>,
    options: KernelOptions,
    /// Pressure read on the previous turn; the summary update happens before
    /// this turn's context is assembled, so this is the freshest signal.
    last_level: Mutex<PressureLevel>,
    next_turn: AtomicU64,
    /// User messages that arrived mid-turn, delivered at safe points.
    interjections: InterjectionQueue,
    /// The running turn id, or 0 when idle. Used to stamp queued events.
    active_turn: AtomicU64,
}

impl<M: ModelProvider> TurnDriver<M> {
    pub fn new(
        model: M,
        tools: Arc<ToolRegistry>,
        context: ContextBuilder,
        policy: Arc<dyn Policy>,
        config: TurnDriverConfig,
    ) -> Self {
        let dispatcher = ToolDispatcher::new(
            tools.clone(),
            config.events.clone(),
            config.permissions.clone(),
            config.limits.tool_timeout,
        )
        .with_interactions(config.interactions.clone());
        let working_set = policy.context_recipe().budget_tokens;
        Self {
            model,
            tools,
            context,
            policy,
            events: config.events,
            interactions: config.interactions,
            dispatcher,
            system_prompt: config.system_prompt,
            max_output_tokens: config.max_output_tokens,
            limits: config.limits,
            workspace_root: config.permissions.root().to_path_buf(),
            acceptance: None,
            session: Mutex::new(Session::new()),
            context_window: config.context_window,
            output_reserve: config.max_output_tokens,
            working_set,
            overhead: AtomicU64::new(0),
            summarizer: None,
            outputs_dir: None,
            todos: config.todos,
            options: config.options,
            last_level: Mutex::new(PressureLevel::Green),
            next_turn: AtomicU64::new(1),
            interjections: InterjectionQueue::default(),
            active_turn: AtomicU64::new(0),
        }
    }

    /// Enables model-written summaries for large sealed segments.
    pub fn with_summarizer(mut self, summarizer: Arc<dyn Summarizer>) -> Self {
        self.summarizer = Some(summarizer);
        self
    }

    /// Sets where long tool outputs spill. The host should allow reads on this
    /// directory through the permission engine so the model can follow the
    /// marker.
    pub fn with_outputs_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.outputs_dir = Some(dir.into());
        self
    }

    /// Enables host-provided acceptance checks at the `Verify` phase.
    pub fn with_acceptance(mut self, items: Vec<AcceptanceItem>, max_reprompts: u32) -> Self {
        self.acceptance = Some(AcceptancePlan {
            items,
            max_reprompts,
        });
        self
    }

    /// Captures the session for a host store. The kernel stays unaware of the
    /// backend; the host decides where the snapshot lives.
    pub fn session_snapshot(&self) -> SessionSnapshot {
        self.session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .snapshot()
    }

    /// Host-facing permission mode switch. A UI can offer `full_auto` without
    /// waiting for a tool to ask; the grant still expires with `ttl`.
    pub fn escalate_full_auto(&self, ttl: Duration) {
        self.dispatcher.escalate_full_auto(ttl);
    }

    /// Drops session grants and returns to the profile's base level.
    pub fn revoke_permission_grants(&self) {
        self.dispatcher.revoke_grants();
    }

    /// Continues a stored session instead of starting an empty one.
    pub fn with_session(mut self, session: Session) -> Self {
        self.session = Mutex::new(session);
        self
    }

    /// Queues a user message for the running turn. It is delivered at the next
    /// safe point; the host reports `Full` or `Duplicate` to the user.
    pub fn interject(&self, text: &str) -> InterjectionPush {
        let outcome = self.interjections.push(text);
        let active = self.active_turn.load(Ordering::Relaxed);
        if let InterjectionPush::Queued(id) = outcome {
            if active != 0 {
                self.events.append(Event::InterjectionQueued {
                    turn_id: keel_core::TurnId::new(active),
                    id,
                    text: text.trim().to_owned(),
                });
            }
        }
        outcome
    }

    /// Runs one turn. The returned state is always terminal.
    pub async fn run(&self, input: &str, cancel: CancelToken) -> TurnOutcome {
        let started = std::time::Instant::now();
        let turn_id = keel_core::TurnId::new(self.next_turn.fetch_add(1, Ordering::Relaxed));
        self.active_turn.store(turn_id.get(), Ordering::Relaxed);
        let mut fsm = TurnFsm::new();
        let mut text = String::new();
        self.events.append(Event::TurnStarted {
            turn_id,
            input: input.to_owned(),
        });
        let secret_warning = detect_secret(input);
        if let Some(kind) = secret_warning {
            self.events.append(Event::SecretWarning {
                turn_id,
                kind: kind.as_str().to_owned(),
            });
        }

        let deadline = tokio::time::Instant::now() + self.limits.turn_timeout;
        let mut stats = TurnStats::default();
        let result = self
            .run_inner(
                turn_id, input, &cancel, deadline, &mut fsm, &mut text, &mut stats,
            )
            .await;

        let mut error = None;
        if let Err(failure) = result {
            if matches!(failure, KernelError::Cancelled { .. }) {
                let _ = self.step(&mut fsm, turn_id, TurnEvent::Cancel);
            } else {
                error = Some(failure);
                let _ = self.step(&mut fsm, turn_id, TurnEvent::Fail);
            }
        }

        if !fsm.state().is_terminal() {
            error = Some(KernelError::Internal {
                message: format!("turn ended in non-terminal state {}", fsm.state()),
            });
            let _ = self.step(&mut fsm, turn_id, TurnEvent::Fail);
        }

        // Anything still queued missed the last safe point; report it rather
        // than silently dropping it.
        let mut late_interjections = Vec::new();
        for item in self.interjections.drain() {
            self.events.append(Event::InterjectionDropped {
                turn_id,
                id: item.id,
                text: item.text.clone(),
                reason: "turn ended before a safe point".to_owned(),
            });
            late_interjections.push(item.text);
        }
        self.active_turn.store(0, Ordering::Relaxed);

        let state = fsm.state();
        if state == TurnState::Cancelled {
            self.events.append(Event::TurnCancelled { turn_id, state });
        } else {
            self.events.append(Event::TurnFinished {
                turn_id,
                state,
                text: text.clone(),
            });
        }

        self.events.append(Event::TurnReport {
            turn_id,
            state,
            tool_calls: stats.tool_calls,
            interactions: stats.interactions,
            deferred: stats.deferred.len(),
        });

        if let Some(report) = &stats.wrap_up_report {
            stats.answer.notes = Some(report.clone());
        }
        for interaction in &stats.deferred {
            stats
                .answer
                .record_pending(format!("interaction {} was deferred", interaction.get()));
        }
        stats.answer.verification = stats.verification.clone();
        if stats.answer.body.trim().is_empty() {
            if let Some(report) = &stats.wrap_up_report {
                stats.answer.set_body(report);
            }
        }
        stats.answer.missing = stats.answer.body.trim().is_empty();

        if state != TurnState::Cancelled {
            let artifacts: Vec<String> = stats
                .answer
                .changes
                .iter()
                .map(|change| change.path.clone())
                .collect();
            let pending: Vec<String> = stats
                .answer
                .pending
                .iter()
                .map(|item| item.description.clone())
                .collect();
            let verified = stats
                .verification
                .as_ref()
                .map(|report| report.passed())
                .unwrap_or(false);
            self.session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .record_turn(TurnRecord {
                    input,
                    text: &stats.answer.body,
                    tools: &stats.tool_summaries,
                    artifacts: &artifacts,
                    pending: &pending,
                    verified,
                });
        }

        TurnOutcome {
            turn_id,
            state,
            text,
            error,
            deferred_interactions: stats.deferred,
            late_interjections,
            secret_warning,
            model_calls: stats.model_calls,
            usage: stats.usage,
            duration_ms: started.elapsed().as_millis() as u64,
            reasoning_level: stats.reasoning,
            verification: stats.verification,
            context: stats.context,
            wrap_up_report: stats.wrap_up_report,
            answer: stats.answer,
        }
    }

    fn deliver_interjections(&self, turn_id: keel_core::TurnId, messages: &mut Vec<Arc<Message>>) {
        for item in self.interjections.drain() {
            self.events.append(Event::InterjectionDelivered {
                turn_id,
                id: item.id,
                text: item.text.clone(),
            });
            messages.push(Arc::new(Message::user(format!(
                "[user interjection] {}",
                item.text
            ))));
        }
    }

    fn escalate_reasoning(&self, turn_id: keel_core::TurnId, stats: &mut TurnStats, reason: &str) {
        let from = stats.reasoning;
        let to = from.next();
        if to == from {
            return;
        }
        stats.reasoning = to;
        stats.consecutive_tool_failures = 0;
        stats.empty_rounds = 0;
        self.events.append(Event::ReasoningEscalated {
            turn_id,
            from: from.as_str().to_owned(),
            to: to.as_str().to_owned(),
            reason: reason.to_owned(),
        });
    }

    // The FSM, text buffer and stats are separate borrows by design; bundling
    // them into one struct would hide which part of the turn is being mutated.
    #[allow(clippy::too_many_arguments)]
    async fn run_inner(
        &self,
        turn_id: keel_core::TurnId,
        input: &str,
        cancel: &CancelToken,
        deadline: tokio::time::Instant,
        fsm: &mut TurnFsm,
        text: &mut String,
        stats: &mut TurnStats,
    ) -> Result<(), KernelError> {
        self.step(fsm, turn_id, TurnEvent::Ready)?;
        if cancel.is_cancelled() {
            return Err(KernelError::Cancelled { stage: "observe" });
        }

        // Tool schemas and provider framing are invisible to the estimate;
        // after the first call their measured cost is reserved up front.
        let overhead = self
            .overhead
            .load(Ordering::Relaxed)
            .min(u64::from(u32::MAX)) as u32;
        let mut recipe = self.policy.context_recipe();
        recipe.budget_tokens = recipe.budget_tokens.saturating_sub(overhead);
        let governor = ContextGovernor::new(
            self.context_window,
            self.working_set.saturating_sub(overhead),
            self.output_reserve,
        );
        let (change, transcript, opened_topic, sealed_record, absorbable, previous_summary) = {
            let mut session = self
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let change = session.begin_turn(input);
            let transcript = change.sealed.and_then(|id| session.sealed_transcript(id));
            let opened_topic = change.opened.and_then(|_| {
                session
                    .segments()
                    .last()
                    .map(|segment| segment.topic.clone())
            });
            let sealed_record = change.sealed.and_then(|id| session.record(id));
            let absorbable = session.absorbable_records(change.sealed);
            let previous_summary = session.summary().map(|summary| summary.text.clone());
            (
                change,
                transcript,
                opened_topic,
                sealed_record,
                absorbable,
                previous_summary,
            )
        };
        if let Some(segment_id) = change.sealed {
            self.events.append(Event::SegmentSealed {
                turn_id,
                segment_id,
                reason: change.reason.clone().unwrap_or_default(),
            });
        }
        if let Some(skip) = &change.skipped {
            self.events.append(Event::CompressionSkipped {
                turn_id,
                segment_id: skip.segment_id,
                missing: skip.missing.clone(),
            });
        }
        for key in &change.promoted {
            self.events.append(Event::FactPromoted {
                turn_id,
                key: key.clone(),
            });
        }
        for key in &change.forgotten {
            self.events.append(Event::FactForgotten {
                turn_id,
                key: key.clone(),
            });
        }
        for segment_id in &change.folded {
            self.events.append(Event::SegmentFolded {
                turn_id,
                segment_id: *segment_id,
            });
        }
        if let (Some(segment_id), Some(topic)) = (change.opened, opened_topic) {
            self.events.append(Event::SegmentOpened {
                turn_id,
                segment_id,
                topic,
            });
        }

        // One rewritten summary per seal. The structured records stay the
        // fallback when the call fails, and open items are never absorbed.
        if self.options.memo {
            if let (Some(summarizer), Some(segment_id), Some(record), Some(messages)) = (
                self.summarizer.as_ref(),
                change.sealed,
                sealed_record.as_ref(),
                transcript.as_ref(),
            ) {
                let compact = {
                    let level = *self
                        .last_level
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    level >= PressureLevel::Orange
                };
                let budget = if compact {
                    (memo_budget(recipe.budget_tokens) / 2).max(MEMO_BUDGET_FLOOR)
                } else {
                    memo_budget(recipe.budget_tokens)
                };
                let max_input = self
                    .context_window
                    .saturating_sub(budget)
                    .saturating_sub(SUMMARY_INPUT_MARGIN);

                let mut input_text = String::from("[previous summary]\n");
                match &previous_summary {
                    Some(text) => input_text.push_str(text),
                    None => input_text.push_str("(none)"),
                }
                input_text.push_str("\n\n[newest segment record]\n");
                input_text.push_str(&render_summary_record(segment_id, record));

                let mut lines: Vec<String> = Vec::new();
                let mut included: Vec<u32> = Vec::new();
                let mut fixed_tokens = estimate_tokens(&input_text);
                for (id, older) in absorbable.iter().rev() {
                    let line = render_summary_record(*id, older);
                    let tokens = estimate_tokens(&line);
                    if fixed_tokens
                        .saturating_add(tokens)
                        .saturating_add(MIN_TRANSCRIPT_TOKENS)
                        > max_input
                    {
                        break;
                    }
                    fixed_tokens = fixed_tokens.saturating_add(tokens);
                    lines.push(line);
                    included.push(*id);
                }
                lines.reverse();
                included.reverse();
                input_text.push_str("\n[older finished segments]\n");
                for line in &lines {
                    input_text.push_str(line);
                }

                let transcript_budget = max_input
                    .saturating_sub(fixed_tokens)
                    .max(MIN_TRANSCRIPT_TOKENS);
                let transcript_body = transcript_text(messages);
                let transcript_body = clip_to_tokens(&transcript_body, transcript_budget);
                input_text.push_str("\n[newest segment transcript]\n");
                input_text.push_str(&transcript_body);
                input_text.push('\n');

                let mut covers = Vec::new();
                if record.pending.is_empty() {
                    covers.push(segment_id);
                }
                covers.extend(included.iter().copied());

                if !covers.is_empty() {
                    let result = tokio::select! {
                        _ = cancel.cancelled() => None,
                        result = tokio::time::timeout(
                            SUMMARY_TIMEOUT,
                            summarizer.summarize(&input_text, budget, cancel),
                        ) => Some(result),
                    };
                    match result {
                        Some(Ok(Ok(summary))) => {
                            let mut session = self
                                .session
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            let revision = session.set_summary(&summary.text, &covers, budget);
                            drop(session);
                            if let Some(revision) = revision {
                                stats.usage.input_tokens = stats
                                    .usage
                                    .input_tokens
                                    .saturating_add(summary.usage.input_tokens);
                                stats.usage.output_tokens = stats
                                    .usage
                                    .output_tokens
                                    .saturating_add(summary.usage.output_tokens);
                                stats.usage.cached_input_tokens = stats
                                    .usage
                                    .cached_input_tokens
                                    .saturating_add(summary.usage.cached_input_tokens);
                                stats.model_calls = stats.model_calls.saturating_add(1);
                                self.events.append(Event::SummaryUpdated {
                                    turn_id,
                                    revision,
                                    covers,
                                    chars: summary.text.chars().count() as u32,
                                    budget,
                                    fallback: false,
                                });
                            }
                        }
                        Some(Ok(Err(error))) => {
                            self.apply_fallback_summary(
                                turn_id,
                                segment_id,
                                record,
                                &absorbable,
                                &covers,
                                previous_summary.as_deref(),
                                budget,
                            );
                            let mut session = self
                                .session
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            session.mark_summary_missing();
                            drop(session);
                            self.events.append(Event::SummaryFailed {
                                turn_id,
                                reason: format!("{error:?}"),
                            });
                        }
                        Some(Err(_)) => {
                            self.apply_fallback_summary(
                                turn_id,
                                segment_id,
                                record,
                                &absorbable,
                                &covers,
                                previous_summary.as_deref(),
                                budget,
                            );
                            let mut session = self
                                .session
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            session.mark_summary_missing();
                            drop(session);
                            self.events.append(Event::SummaryFailed {
                                turn_id,
                                reason: "timeout".to_owned(),
                            });
                        }
                        None => {}
                    }
                }
            }
        }

        let (history, records, facts, summary, recalled, session_growth) = {
            let session = self
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                session.history(),
                session.records(),
                session.all_facts(),
                session.summary().cloned(),
                if self.options.recall {
                    session.recall(input)
                } else {
                    Vec::new()
                },
                session.last_turn_tokens(),
            )
        };
        let used = estimate_tokens(&self.system_prompt)
            .saturating_add(history.iter().fold(0u32, |total, message| {
                total.saturating_add(message.estimated_tokens())
            }))
            .saturating_add(estimate_tokens(input));
        // The first turn has no measured growth; a large first input is not
        // evidence that the session grows that fast per call.
        let growth = session_growth;
        let pressure = governor.pressure(used, growth);
        self.events.append(Event::ContextPressure {
            turn_id,
            used,
            ceiling: pressure.ceiling,
            growth,
            headroom_calls: pressure.headroom_calls,
            level: pressure.level.as_str().to_owned(),
        });
        let todos = self.todo_snapshot();
        let build_ctx = BuildCtx {
            turn_id,
            input,
            history: &history,
            records: &records,
            summary: if self.options.memo {
                summary.as_ref()
            } else {
                None
            },
            recalled: &recalled,
            todos: todos.as_ref(),
            facts: &facts,
            system_prompt: &self.system_prompt,
            budget_tokens: recipe.budget_tokens,
            detail_share: pressure.level.detail_share(),
            events: Some(self.events.as_ref() as &dyn EventSink),
        };
        let built = self.context.build(&build_ctx, &recipe, cancel).await?;
        let context_stages = built.stages.clone();
        let context_estimate = built.estimated_tokens;
        let context_budget = built.budget_tokens;
        self.events.append(Event::ContextBuilt {
            turn_id,
            stages: context_stages.clone(),
            tokens: context_estimate,
            budget: context_budget,
        });
        self.step(fsm, turn_id, TurnEvent::Ready)?;

        let visible = self.policy.visible_tools(&self.tools.schemas());
        let visible_names: BTreeSet<String> =
            visible.iter().map(|schema| schema.name.clone()).collect();
        let mut messages: Vec<Arc<Message>> = built
            .fragments
            .into_iter()
            .map(|fragment| Arc::new(Message::new(fragment.role, fragment.text)))
            .collect();
        if self.options.capture_context {
            stats.context = Some(ContextSnapshot {
                estimate: context_estimate,
                budget: context_budget,
                stages: context_stages,
                messages: messages
                    .iter()
                    .map(|message| ContextMessage {
                        role: match message.role {
                            MessageRole::System => "system",
                            MessageRole::User => "user",
                            MessageRole::Assistant => "assistant",
                            MessageRole::Tool => "tool",
                        }
                        .to_owned(),
                        content: redact_secrets(&message.content),
                    })
                    .collect(),
            });
        }

        let mut model_calls = 0u32;
        let mut previous_used: Option<u32> = None;
        let mut wrapped_up = false;
        let mut final_call_announced = false;
        let mut budget_exhausted = false;

        'turn: loop {
            if cancel.is_cancelled() {
                return Err(KernelError::Cancelled { stage: "turn" });
            }
            if model_calls >= self.limits.max_model_calls {
                self.step(fsm, turn_id, TurnEvent::TurnsExhausted)?;
                break 'turn;
            }
            if tokio::time::Instant::now() >= deadline {
                self.step(fsm, turn_id, TurnEvent::WallClockLow)?;
                break 'turn;
            }

            // Safe point: before the next model call is built. This is also
            // before the reserved final call, so a late constraint can still
            // shape the answer. Never inside a stream or a tool round.
            self.deliver_interjections(turn_id, &mut messages);

            let mut estimated_tokens = messages.iter().fold(0u32, |total, message| {
                total.saturating_add(message.estimated_tokens())
            });

            // The turn's cost budget is spent: the next call is the answer.
            // The model is not told a number; it sees the same final-call
            // instruction a call-count limit would produce.
            if !budget_exhausted
                && u64::from(stats.usage.input_tokens) >= self.limits.max_turn_input_tokens
            {
                budget_exhausted = true;
                self.events.append(Event::TurnBudgetExhausted {
                    turn_id,
                    input_tokens: stats.usage.input_tokens,
                });
            }

            // The last call is reserved for the answer, never for new work.
            let final_call = budget_exhausted || model_calls + 1 == self.limits.max_model_calls;
            if final_call && !final_call_announced {
                final_call_announced = true;
                self.events.append(Event::FinalCallForced { turn_id });
                if estimated_tokens.saturating_add(estimate_tokens(FINAL_CALL))
                    <= recipe.budget_tokens
                {
                    messages.push(Arc::new(Message::system(FINAL_CALL)));
                    estimated_tokens = estimated_tokens.saturating_add(estimate_tokens(FINAL_CALL));
                }
            }

            let call_growth = match previous_used {
                Some(previous) => estimated_tokens.saturating_sub(previous).max(1),
                None => growth.max(1),
            };
            previous_used = Some(estimated_tokens);
            let pressure = governor.pressure(estimated_tokens, call_growth);
            *self
                .last_level
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = pressure.level;
            self.events.append(Event::ContextPressure {
                turn_id,
                used: estimated_tokens,
                ceiling: pressure.ceiling,
                growth: call_growth,
                headroom_calls: pressure.headroom_calls,
                level: pressure.level.as_str().to_owned(),
            });
            let wrap_up_fits =
                estimated_tokens.saturating_add(estimate_tokens(WRAP_UP)) <= recipe.budget_tokens;
            if pressure.level == PressureLevel::Red && !wrapped_up && wrap_up_fits {
                wrapped_up = true;
                stats.awaiting_wrap_up = true;
                messages.push(Arc::new(Message::system(WRAP_UP)));
                self.events.append(Event::WrapUpInjected {
                    turn_id,
                    used: estimated_tokens,
                    ceiling: pressure.ceiling,
                });
                continue 'turn;
            }
            if estimated_tokens > recipe.budget_tokens {
                let before = estimated_tokens;
                let elided = elide_old_tool_results(&mut messages);
                if elided > 0 {
                    estimated_tokens = messages.iter().fold(0u32, |total, message| {
                        total.saturating_add(message.estimated_tokens())
                    });
                    self.events.append(Event::ContextElided {
                        turn_id,
                        messages: elided,
                        before,
                        after: estimated_tokens,
                    });
                }
            }
            if estimated_tokens > recipe.budget_tokens {
                self.step(fsm, turn_id, TurnEvent::Overflow)?;
                return Err(KernelError::ContextOverflow {
                    used: estimated_tokens,
                    budget: recipe.budget_tokens,
                });
            }
            model_calls += 1;
            stats.model_calls = stats.model_calls.saturating_add(1);

            let route = self.policy.model_route(ModelRole::Main);
            let request = ModelRequest {
                messages: messages.clone(),
                tools: if final_call {
                    Vec::new()
                } else {
                    visible.clone()
                },
                control_tool: !final_call,
                thinking_budget: self.policy.thinking_budget(),
                max_output_tokens: self.max_output_tokens,
                route: route.clone(),
                reasoning: stats.reasoning,
            };
            self.events.append(Event::ModelStarted {
                turn_id,
                provider: route.provider.clone(),
                model: route.model.clone(),
            });

            let call_deadline = deadline.min(tokio::time::Instant::now() + MODEL_CALL_TIMEOUT);
            let stream_result = tokio::select! {
                _ = cancel.cancelled() => {
                    return Err(KernelError::Cancelled { stage: "model" });
                }
                result = tokio::time::timeout_at(call_deadline, self.model.stream(request, cancel)) => result,
            };

            let mut stream = match stream_result {
                Ok(Ok(stream)) => stream,
                Ok(Err(failure)) => {
                    let event = if failure.code == ErrorCode::ContextOverflow {
                        TurnEvent::Overflow
                    } else {
                        TurnEvent::StreamFailure
                    };
                    self.step(fsm, turn_id, event)?;
                    return Err(KernelError::Model(failure));
                }
                Err(_) => {
                    if text.is_empty() {
                        return Err(KernelError::Timeout {
                            stage: "model",
                            limit_ms: self.timeout_ms(),
                        });
                    }
                    self.step(fsm, turn_id, TurnEvent::WallClockLow)?;
                    break 'turn;
                }
            };

            let mut round_text = String::new();
            let mut round_reasoning = String::new();
            let mut calls = Vec::new();
            let mut pending_interactions: Vec<InteractionRequest> = Vec::new();

            // A stream that keeps delivering chunks is alive, however long the
            // answer takes; only a silent stream hits the window.
            let mut chunk_deadline = call_deadline;
            let usage = loop {
                let chunk = tokio::select! {
                    _ = cancel.cancelled() => {
                        return Err(KernelError::Cancelled { stage: "model stream" });
                    }
                    result = tokio::time::timeout_at(chunk_deadline, stream.recv()) => result,
                };
                chunk_deadline = deadline.min(tokio::time::Instant::now() + MODEL_CALL_TIMEOUT);

                match chunk {
                    Err(_) => {
                        if text.is_empty() {
                            return Err(KernelError::Timeout {
                                stage: "model stream",
                                limit_ms: self.timeout_ms(),
                            });
                        }
                        self.step(fsm, turn_id, TurnEvent::WallClockLow)?;
                        break 'turn;
                    }
                    Ok(None) => {
                        self.step(fsm, turn_id, TurnEvent::StreamFailure)?;
                        return Err(KernelError::Model(ModelError::stream_cut(
                            "model stream ended without a finish chunk",
                        )));
                    }
                    Ok(Some(Err(failure))) => {
                        let event = if failure.code == ErrorCode::ContextOverflow {
                            TurnEvent::Overflow
                        } else {
                            TurnEvent::StreamFailure
                        };
                        self.step(fsm, turn_id, event)?;
                        return Err(KernelError::Model(failure));
                    }
                    Ok(Some(Ok(ModelChunk::TextDelta(delta)))) => {
                        text.push_str(&delta);
                        round_text.push_str(&delta);
                    }
                    Ok(Some(Ok(ModelChunk::ThinkingDelta(delta)))) => {
                        round_reasoning.push_str(&delta);
                    }
                    Ok(Some(Ok(ModelChunk::ToolCall(call)))) => calls.push(call),
                    Ok(Some(Ok(ModelChunk::Interaction(request)))) => {
                        pending_interactions.push(request);
                    }
                    Ok(Some(Ok(ModelChunk::Finished { usage: reported }))) => {
                        break reported;
                    }
                }
            };

            let observed_overhead = usage.input_tokens.saturating_sub(estimated_tokens);
            self.overhead
                .fetch_max(u64::from(observed_overhead), Ordering::Relaxed);
            stats.usage.input_tokens = stats.usage.input_tokens.saturating_add(usage.input_tokens);
            stats.usage.output_tokens = stats
                .usage
                .output_tokens
                .saturating_add(usage.output_tokens);
            stats.usage.cached_input_tokens = stats
                .usage
                .cached_input_tokens
                .saturating_add(usage.cached_input_tokens);
            self.events.append(Event::ModelFinished {
                turn_id,
                tool_calls: calls.len(),
                usage,
            });

            // The first text after the wrap-up instruction is the report. It
            // stays ordinary assistant text, and is also kept separately so
            // the session can use it when the segment is sealed.
            if stats.awaiting_wrap_up && !round_text.trim().is_empty() {
                stats.wrap_up_report = Some(round_text.clone());
                stats.awaiting_wrap_up = false;
            }

            // Tools were disabled for the final call. If the model asked for
            // one anyway, ignore it and keep any text it produced.
            if final_call && (!calls.is_empty() || !pending_interactions.is_empty()) {
                calls.clear();
                pending_interactions.clear();
                if round_text.trim().is_empty() {
                    self.step(fsm, turn_id, TurnEvent::TurnsExhausted)?;
                    return Err(KernelError::Model(ModelError::internal(
                        "model requested tools on the final call and produced no answer",
                    )));
                }
            }

            if !pending_interactions.is_empty() {
                if !calls.is_empty() {
                    self.step(fsm, turn_id, TurnEvent::StreamFailure)?;
                    return Err(KernelError::Model(ModelError::internal(
                        "model returned tool calls and an interaction in one response",
                    )));
                }
                if pending_interactions.len() > 1 {
                    self.step(fsm, turn_id, TurnEvent::StreamFailure)?;
                    return Err(KernelError::Model(ModelError::internal(
                        "model returned more than one interaction in one response",
                    )));
                }

                let Some(mut request) = pending_interactions.pop() else {
                    return Err(KernelError::Internal {
                        message: "interaction queue was empty".to_owned(),
                    });
                };
                request.turn_id = turn_id;
                request.id = InteractionId::next();
                request.timeout_ms = None;
                stats.interactions = stats.interactions.saturating_add(1);
                if stats.interactions > self.interactions.policy().max_per_turn {
                    self.step(fsm, turn_id, TurnEvent::StreamFailure)?;
                    return Err(KernelError::InvalidConfig {
                        message: "interaction budget exceeded".to_owned(),
                    });
                }

                self.step(fsm, turn_id, TurnEvent::InteractionPending)?;
                let response = self.interactions.request(request.clone(), cancel).await;
                match response {
                    InteractionResponse::Approved { .. } => {
                        messages.push(Arc::new(Message::user(format!(
                            "[interaction {}] approved",
                            request.id
                        ))));
                        self.step(fsm, turn_id, TurnEvent::InteractionResolved)?;
                        continue 'turn;
                    }
                    InteractionResponse::Selected { option } => {
                        messages.push(Arc::new(Message::user(format!(
                            "[interaction {}] selected: {option}",
                            request.id
                        ))));
                        self.step(fsm, turn_id, TurnEvent::InteractionResolved)?;
                        continue 'turn;
                    }
                    InteractionResponse::Freeform { text } => {
                        messages.push(Arc::new(Message::user(format!(
                            "[interaction {}] answer: {text}",
                            request.id
                        ))));
                        self.step(fsm, turn_id, TurnEvent::InteractionResolved)?;
                        continue 'turn;
                    }
                    InteractionResponse::Rejected { reason } => {
                        messages.push(Arc::new(Message::user(format!(
                            "[interaction {}] rejected: {reason}",
                            request.id
                        ))));
                        self.step(fsm, turn_id, TurnEvent::InteractionResolved)?;
                        continue 'turn;
                    }
                    InteractionResponse::Deferred => {
                        stats.deferred.push(request.id);
                        messages.push(Arc::new(Message::system(format!(
                            "[interaction {}] deferred; do not assume an answer. Continue with independent work and list deferred items in the final report.",
                            request.id
                        ))));
                        if self.interactions.policy().on_timeout == OnTimeout::EndTurn {
                            self.step(fsm, turn_id, TurnEvent::InteractionEndTurn)?;
                            break 'turn;
                        }
                        self.step(fsm, turn_id, TurnEvent::InteractionDeferred)?;
                        continue 'turn;
                    }
                    InteractionResponse::TimedOut => {
                        if self.interactions.policy().on_timeout == OnTimeout::Fail {
                            return Err(KernelError::Timeout {
                                stage: "interaction",
                                limit_ms: self.timeout_ms(),
                            });
                        }
                        stats.deferred.push(request.id);
                        messages.push(Arc::new(Message::system(format!(
                            "[interaction {}] timed out; do not assume an answer. Continue with independent work and list deferred items in the final report.",
                            request.id
                        ))));
                        if self.interactions.policy().on_timeout == OnTimeout::EndTurn {
                            self.step(fsm, turn_id, TurnEvent::InteractionEndTurn)?;
                            break 'turn;
                        }
                        self.step(fsm, turn_id, TurnEvent::InteractionDeferred)?;
                        continue 'turn;
                    }
                    InteractionResponse::Cancelled => {
                        return Err(KernelError::Cancelled {
                            stage: "interaction",
                        });
                    }
                }
            }

            if !calls.is_empty() {
                self.step(fsm, turn_id, TurnEvent::ToolCalls)?;
                stats.tool_calls = stats.tool_calls.saturating_add(calls.len() as u32);
                if stats.tool_calls > self.limits.max_tool_calls {
                    self.step(fsm, turn_id, TurnEvent::TurnsExhausted)?;
                    break 'turn;
                }

                let mut tool_call_message = Message::assistant_tool_calls(calls.clone());
                tool_call_message.content = round_text;
                if !round_reasoning.is_empty() {
                    tool_call_message = tool_call_message.with_reasoning(round_reasoning.clone());
                }
                messages.push(Arc::new(tool_call_message));

                let calls_for_audit = calls.clone();
                let todo_before = self.todo_revision();
                let todo_state_before = self.todo_state();
                let results = self
                    .dispatcher
                    .dispatch(turn_id, calls, &visible_names, cancel)
                    .await;
                if cancel.is_cancelled() {
                    return Err(KernelError::Cancelled { stage: "tools" });
                }
                let todo_after = self.todo_revision();
                if todo_after != todo_before {
                    let (items, in_progress, state, completed, failed) = self.todo_summary();
                    self.events.append(Event::TodoUpdated {
                        turn_id,
                        revision: todo_after,
                        items,
                        in_progress,
                        state: state.as_str().to_owned(),
                    });
                    if !todo_state_before.is_closed() && state.is_closed() {
                        self.events.append(Event::TodoClosed {
                            turn_id,
                            outcome: state.as_str().to_owned(),
                            completed,
                            failed,
                            total: items,
                        });
                    }
                }
                for (index, (call, result)) in
                    calls_for_audit.iter().zip(results.iter()).enumerate()
                {
                    let content = match &result.outcome {
                        ToolOutcome::Success { content } => content.clone(),
                        ToolOutcome::Failure { error } => {
                            format!("error {}: {}", error.code.as_str(), error.message)
                        }
                    };
                    // The live result is bounded exactly like the recorded
                    // summary. A large read_file must not push the current
                    // turn over the context budget; the spill path is carried
                    // in the marker so the model can re-read a region.
                    let outputs = self
                        .outputs_dir
                        .clone()
                        .unwrap_or_else(|| self.workspace_root.join(".keel").join("outputs"));
                    let output = summarize_output(&outputs, turn_id.get(), index, &content).await;
                    let live = if self.options.bounded_tools {
                        output.clone()
                    } else {
                        content
                    };
                    messages.push(Arc::new(Message::tool_result(
                        result.call_id,
                        &result.tool,
                        live,
                    )));

                    let status = match &result.outcome {
                        ToolOutcome::Success { .. } => "ok".to_owned(),
                        ToolOutcome::Failure { error } => {
                            format!("error {}", error.code.as_str())
                        }
                    };
                    stats.tool_summaries.push(keel_core::ToolSummary {
                        name: call.name.clone(),
                        args: redact_secrets(&compact_args(&call.args)),
                        status,
                        output,
                    });
                }

                let mut failures = 0u32;
                let mut repeated = false;
                for (call, result) in calls_for_audit.iter().zip(results.iter()) {
                    if call.name == "write_file"
                        && matches!(&result.outcome, ToolOutcome::Success { .. })
                    {
                        if let Some(ArgValue::String(path)) = call.args.get("path") {
                            stats.answer.record_change(path.clone());
                        }
                    }
                    let reasoning_failure = match &result.outcome {
                        ToolOutcome::Failure { error } => {
                            matches!(error.code, ErrorCode::InvalidArguments)
                        }
                        ToolOutcome::Success { .. } => false,
                    };
                    if !reasoning_failure {
                        continue;
                    }
                    failures = failures.saturating_add(1);
                    if let Some((name, args)) = &stats.previous_failed_call {
                        if name == &call.name && args == &call.args {
                            repeated = true;
                        }
                    }
                    stats.previous_failed_call = Some((call.name.clone(), call.args.clone()));
                }
                if failures == 0 {
                    stats.consecutive_tool_failures = 0;
                    stats.previous_failed_call = None;
                } else {
                    stats.consecutive_tool_failures =
                        stats.consecutive_tool_failures.saturating_add(failures);
                    if repeated || stats.consecutive_tool_failures >= 2 {
                        self.escalate_reasoning(turn_id, stats, "tool failures");
                    }
                }
                stats.empty_rounds = 0;

                self.step(fsm, turn_id, TurnEvent::Ready)?;
                continue 'turn;
            }

            if round_text.is_empty() {
                stats.empty_rounds = stats.empty_rounds.saturating_add(1);
                if stats.empty_rounds >= 2 {
                    self.escalate_reasoning(turn_id, stats, "no progress");
                }
                self.step(fsm, turn_id, TurnEvent::ThinkingOnly)?;
                continue 'turn;
            }

            // A model may report a partial result while its own task list
            // still has unfinished items. Continue the same turn instead of
            // making the user type "continue"; the cap bounds a model that
            // never closes the list, and a wrap-up or the reserved final call
            // is never overridden.
            if !wrapped_up && !final_call_announced && stats.continuations < MAX_CONTINUATIONS {
                if let Some((remaining, prompt)) = self.unfinished_todos() {
                    stats.continuations = stats.continuations.saturating_add(1);
                    messages.push(Arc::new(Message::assistant(round_text.clone())));
                    messages.push(Arc::new(Message::user(prompt)));
                    self.events
                        .append(Event::TurnContinued { turn_id, remaining });
                    self.step(fsm, turn_id, TurnEvent::Continued)?;
                    continue 'turn;
                }
            }

            stats.answer.set_body(&round_text);
            self.step(fsm, turn_id, TurnEvent::DoneSignal)?;
            if let Some(plan) = &self.acceptance {
                let verifier = AcceptanceVerifier::new(&self.workspace_root);
                let report = verifier.verify(&plan.items).await;
                let failures = verification_failures(&report);
                self.events.append(Event::VerificationFinished {
                    turn_id,
                    passed: report.passed(),
                    required_failed: report.required_failed.clone(),
                    required_unverified: report.required_unverified.clone(),
                });
                stats.verification = Some(report.clone());
                if report.passed() {
                    self.step(fsm, turn_id, TurnEvent::DoneSignal)?;
                } else {
                    self.escalate_reasoning(turn_id, stats, "verification failed");
                    if stats.verification_reprompts < plan.max_reprompts {
                        stats.verification_reprompts =
                            stats.verification_reprompts.saturating_add(1);
                        self.step(fsm, turn_id, TurnEvent::UnverifiedHard)?;
                        messages.push(Arc::new(Message::system(format!(
                            "[verification] required items did not pass: {}. Fix the workspace and report the real result; do not claim a check ran unless it did.",
                            failures.join(", ")
                        ))));
                        self.step(fsm, turn_id, TurnEvent::Ready)?;
                        continue 'turn;
                    }
                    self.step(fsm, turn_id, TurnEvent::UnverifiedHard)?;
                    return Err(KernelError::VerificationFailed { failures });
                }
            } else {
                match self.policy.verification_mode() {
                    VerificationMode::None => {
                        self.step(fsm, turn_id, TurnEvent::DoneSignal)?;
                    }
                    other => {
                        return Err(KernelError::Unsupported {
                            feature: format!("verification mode {other:?}"),
                        });
                    }
                }
            }
            break 'turn;
        }

        if fsm.state() == TurnState::Converge && self.acceptance.is_some() {
            let failures = match &stats.verification {
                Some(report) if report.passed() => Vec::new(),
                Some(report) => verification_failures(report),
                None => vec!["verification did not run".to_owned()],
            };
            if !failures.is_empty() {
                return Err(KernelError::VerificationFailed { failures });
            }
        }

        if fsm.state() == TurnState::Converge {
            match self.policy.before_commit(&CommitCtx {
                text: text.as_str(),
                tool_rounds: stats.tool_calls,
            }) {
                CommitDecision::Commit => {
                    self.step(fsm, turn_id, TurnEvent::DoneSignal)?;
                }
                other => {
                    return Err(KernelError::Unsupported {
                        feature: format!("commit decision {other:?}"),
                    });
                }
            }
        }

        Ok(())
    }

    fn step(
        &self,
        fsm: &mut TurnFsm,
        turn_id: keel_core::TurnId,
        event: TurnEvent,
    ) -> Result<TurnState, KernelError> {
        let from = fsm.state();
        let to = fsm.apply(event)?;
        self.events.append(Event::StateChanged {
            turn_id,
            from,
            to,
            event,
        });
        Ok(to)
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_fallback_summary(
        &self,
        turn_id: keel_core::TurnId,
        segment_id: u32,
        record: &SegmentRecord,
        absorbable: &[(u32, SegmentRecord)],
        covers: &[u32],
        previous: Option<&str>,
        budget: u32,
    ) {
        let Some((text, used)) =
            fallback_memo(previous, segment_id, record, absorbable, covers, budget)
        else {
            return;
        };
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let revision = session.set_summary(&text, &used, budget);
        drop(session);
        if let Some(revision) = revision {
            self.events.append(Event::SummaryUpdated {
                turn_id,
                revision,
                covers: used,
                chars: text.chars().count() as u32,
                budget,
                fallback: true,
            });
        }
    }

    fn todo_snapshot(&self) -> Option<TodoList> {
        self.todos.as_ref().map(|handle| {
            handle
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        })
    }

    fn todo_revision(&self) -> u32 {
        self.todos
            .as_ref()
            .map(|handle| {
                handle
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .revision
            })
            .unwrap_or(0)
    }

    fn todo_state(&self) -> TodoState {
        self.todos
            .as_ref()
            .map(|handle| {
                handle
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .state
            })
            .unwrap_or(TodoState::Active)
    }

    /// The prompt that restarts a turn whose model stopped early, or `None`
    /// when the task list is closed or has nothing left to do.
    fn unfinished_todos(&self) -> Option<(u32, String)> {
        let handle = self.todos.as_ref()?;
        let list = handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if list.state != TodoState::Active {
            return None;
        }
        let pending: Vec<_> = list
            .items
            .iter()
            .filter(|item| matches!(item.status, TodoStatus::Pending | TodoStatus::InProgress))
            .collect();
        if pending.is_empty() {
            return None;
        }
        let mut prompt = String::from("The task list still has unfinished items:\n");
        for item in pending.iter().take(20) {
            prompt.push_str(&format!(
                "- #{} [{}] {}\n",
                item.id,
                item.status.as_str(),
                item.text
            ));
        }
        prompt.push_str(
            "Continue with the next item now, in this same turn. Do not stop to summarize \
             what you plan to do. Stop only if you need a decision from the user, and state \
             exactly what you need.",
        );
        Some((pending.len() as u32, prompt))
    }

    fn todo_summary(&self) -> (u32, Option<u32>, TodoState, u32, u32) {
        self.todos
            .as_ref()
            .map(|handle| {
                let list = handle
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                (
                    list.items.len() as u32,
                    list.current().map(|item| item.id),
                    list.state,
                    list.completed() as u32,
                    list.failed() as u32,
                )
            })
            .unwrap_or((0, None, TodoState::Active, 0, 0))
    }

    fn timeout_ms(&self) -> u64 {
        self.limits
            .turn_timeout
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }
}

fn verification_failures(report: &VerificationReport) -> Vec<String> {
    report
        .required_failed
        .iter()
        .chain(report.required_unverified.iter())
        .map(|item_id| {
            let detail = report
                .evidence
                .iter()
                .find(|evidence| evidence.item_id == *item_id)
                .map(|evidence| evidence.detail.trim())
                .filter(|detail| !detail.is_empty());
            match detail {
                Some(detail) => {
                    let short: String = detail.chars().take(300).collect();
                    format!("{item_id}: {short}")
                }
                None => item_id.clone(),
            }
        })
        .collect()
}

/// Builds a deterministic memo from structured records when the model summary
/// fails. Only records that fit the budget are covered; the rest stay visible.
#[allow(clippy::too_many_arguments)]
fn fallback_memo(
    previous: Option<&str>,
    new_id: u32,
    new_record: &SegmentRecord,
    absorbable: &[(u32, SegmentRecord)],
    covers: &[u32],
    budget: u32,
) -> Option<(String, Vec<u32>)> {
    if !covers.contains(&new_id) {
        return None;
    }
    let mut text = String::from("[structured fallback summary]\n");
    text.push_str(&render_summary_record(new_id, new_record));
    if let Some(previous) = previous {
        text.push_str(previous);
        text.push('\n');
    }
    let mut used = vec![new_id];
    let mut tokens = estimate_tokens(&text);
    let mut lines: Vec<String> = Vec::new();
    for (id, record) in absorbable.iter().rev() {
        if !covers.contains(id) {
            continue;
        }
        let line = render_summary_record(*id, record);
        let line_tokens = estimate_tokens(&line);
        if tokens.saturating_add(line_tokens) > budget {
            continue;
        }
        tokens = tokens.saturating_add(line_tokens);
        lines.push(line);
        used.push(*id);
    }
    lines.reverse();
    for line in lines {
        text.push_str(&line);
    }
    Some((trim_to_budget(&text, budget), used))
}

/// Replaces old tool-result bodies with a short marker, keeping the latest
/// tool-call round intact. Returns how many messages were elided.
fn elide_old_tool_results(messages: &mut [Arc<Message>]) -> u32 {
    let Some(last_calls) = messages.iter().rposition(|message| {
        message.role == MessageRole::Assistant && !message.tool_calls.is_empty()
    }) else {
        return 0;
    };
    let mut elided = 0;
    for message in messages[..last_calls].iter_mut() {
        if message.role != MessageRole::Tool {
            continue;
        }
        if message.content.contains("earlier tool result elided") {
            continue;
        }
        if message.content.chars().count() <= 64 {
            continue;
        }
        let call_id = message.tool_call_id.unwrap_or_else(|| ToolCallId::new(0));
        *message = Arc::new(Message::tool_result(
            call_id,
            "elided",
            "[earlier tool result elided to save context]",
        ));
        elided += 1;
    }
    elided
}

/// One structured line for the summary input. The model sees exact outcomes
/// even when the transcript is clipped.
fn render_summary_record(segment_id: u32, record: &SegmentRecord) -> String {
    let mut line = format!(
        "[segment {segment_id}] goal: {}; result: {}",
        record.goal, record.result
    );
    if !record.changes.is_empty() {
        line.push_str("; changes: ");
        line.push_str(&record.changes.join(", "));
    }
    if record.verified {
        line.push_str("; verified");
    }
    if !record.pending.is_empty() {
        line.push_str("; pending: ");
        line.push_str(&record.pending.join("; "));
    }
    line.push('\n');
    line
}
