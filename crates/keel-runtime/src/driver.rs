use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use keel_core::{
    detect_secret, BuildCtx, CancelToken, CommitCtx, CommitDecision, ErrorCode, Event,
    InteractionId, InteractionRequest, InteractionResponse, KernelError, Message, ModelChunk,
    ModelError, ModelProvider, ModelRequest, ModelRole, OnTimeout, Policy, SecretKind, ToolOutcome,
    TurnEvent, TurnState, VerificationMode,
};

use crate::context::ContextBuilder;
use crate::dispatch::ToolDispatcher;
use crate::event_log::EventLog;
use crate::fsm::TurnFsm;
use crate::interaction::InteractionBroker;
use crate::permission::PermissionEngine;
use crate::policy::TurnLimits;
use crate::registry::ToolRegistry;

/// Host configuration that is not one of the seven policy questions.
pub struct TurnDriverConfig {
    pub events: Arc<EventLog>,
    pub permissions: Arc<PermissionEngine>,
    pub interactions: Arc<InteractionBroker>,
    pub system_prompt: String,
    pub max_output_tokens: u32,
    pub limits: TurnLimits,
}

#[derive(Default)]
struct TurnStats {
    tool_calls: u32,
    interactions: u32,
    deferred: Vec<InteractionId>,
    usage: keel_core::TokenUsage,
}

/// Terminal result of one turn.
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    pub turn_id: keel_core::TurnId,
    pub state: TurnState,
    pub text: String,
    pub error: Option<KernelError>,
    pub deferred_interactions: Vec<InteractionId>,
    /// Secret-like input seen at turn start. The turn still runs; the host
    /// surfaces the notice with this turn's result.
    pub secret_warning: Option<SecretKind>,
    /// Token usage summed over model calls in this turn.
    pub usage: keel_core::TokenUsage,
    /// Wall-clock duration of the turn.
    pub duration_ms: u64,
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
    next_turn: AtomicU64,
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
            next_turn: AtomicU64::new(1),
        }
    }

    /// Runs one turn. The returned state is always terminal.
    pub async fn run(&self, input: &str, cancel: CancelToken) -> TurnOutcome {
        let started = std::time::Instant::now();
        let turn_id = keel_core::TurnId::new(self.next_turn.fetch_add(1, Ordering::Relaxed));
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

        TurnOutcome {
            turn_id,
            state,
            text,
            error,
            deferred_interactions: stats.deferred,
            secret_warning,
            usage: stats.usage,
            duration_ms: started.elapsed().as_millis() as u64,
        }
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

        let recipe = self.policy.context_recipe();
        let build_ctx = BuildCtx {
            turn_id,
            input,
            history: &[],
            system_prompt: &self.system_prompt,
        };
        let built = self.context.build(&build_ctx, &recipe, cancel).await?;
        self.events.append(Event::ContextBuilt {
            turn_id,
            stages: built.stages.clone(),
            tokens: built.estimated_tokens,
            budget: built.budget_tokens,
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

        let mut model_calls = 0u32;

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

            let estimated_tokens = messages.iter().fold(0u32, |total, message| {
                total.saturating_add(message.estimated_tokens())
            });
            if estimated_tokens > recipe.budget_tokens {
                self.step(fsm, turn_id, TurnEvent::Overflow)?;
                return Err(KernelError::ContextOverflow {
                    used: estimated_tokens,
                    budget: recipe.budget_tokens,
                });
            }
            model_calls += 1;

            let route = self.policy.model_route(ModelRole::Main);
            let request = ModelRequest {
                messages: messages.clone(),
                tools: visible.clone(),
                thinking_budget: self.policy.thinking_budget(),
                max_output_tokens: self.max_output_tokens,
                route: route.clone(),
            };
            self.events.append(Event::ModelStarted {
                turn_id,
                provider: route.provider.clone(),
                model: route.model.clone(),
            });

            let stream_result = tokio::select! {
                _ = cancel.cancelled() => {
                    return Err(KernelError::Cancelled { stage: "model" });
                }
                result = tokio::time::timeout_at(deadline, self.model.stream(request, cancel)) => result,
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

            let usage = loop {
                let chunk = tokio::select! {
                    _ = cancel.cancelled() => {
                        return Err(KernelError::Cancelled { stage: "model stream" });
                    }
                    result = tokio::time::timeout_at(deadline, stream.recv()) => result,
                };

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

            stats.usage.input_tokens = stats.usage.input_tokens.saturating_add(usage.input_tokens);
            stats.usage.output_tokens = stats
                .usage
                .output_tokens
                .saturating_add(usage.output_tokens);
            self.events.append(Event::ModelFinished {
                turn_id,
                tool_calls: calls.len(),
                usage,
            });

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

                if !round_text.is_empty() {
                    messages.push(Arc::new(Message::assistant(round_text)));
                }
                let mut tool_call_message = Message::assistant_tool_calls(calls.clone());
                if !round_reasoning.is_empty() {
                    tool_call_message = tool_call_message.with_reasoning(round_reasoning.clone());
                }
                messages.push(Arc::new(tool_call_message));

                let results = self
                    .dispatcher
                    .dispatch(turn_id, calls, &visible_names, cancel)
                    .await;
                if cancel.is_cancelled() {
                    return Err(KernelError::Cancelled { stage: "tools" });
                }
                for result in results {
                    let content = match &result.outcome {
                        ToolOutcome::Success { content } => content.clone(),
                        ToolOutcome::Failure { error } => {
                            format!("error {}: {}", error.code.as_str(), error.message)
                        }
                    };
                    messages.push(Arc::new(Message::tool_result(
                        result.call_id,
                        &result.tool,
                        content,
                    )));
                }

                self.step(fsm, turn_id, TurnEvent::Ready)?;
                continue 'turn;
            }

            if round_text.is_empty() {
                self.step(fsm, turn_id, TurnEvent::ThinkingOnly)?;
                continue 'turn;
            }

            self.step(fsm, turn_id, TurnEvent::DoneSignal)?;
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
            break 'turn;
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

    fn timeout_ms(&self) -> u64 {
        self.limits
            .turn_timeout
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }
}
