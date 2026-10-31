use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use keel_core::{
    estimate_tokens, BuildCtx, CancelToken, ContextRecipe, ContextStage, Fragment, KernelError,
    Message, MessageRole,
};

/// Result of one context assembly pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltContext {
    pub fragments: Vec<Fragment>,
    pub stages: Vec<String>,
    pub estimated_tokens: u32,
    pub budget_tokens: u32,
}

/// Ordered registry of context stages plus deterministic budget enforcement.
pub struct ContextBuilder {
    stages: Vec<Arc<dyn ContextStage>>,
}

impl ContextBuilder {
    pub fn new(stages: Vec<Arc<dyn ContextStage>>) -> Self {
        Self { stages }
    }

    pub fn stage_names(&self) -> Vec<&'static str> {
        self.stages.iter().map(|stage| stage.name()).collect()
    }

    /// Builds the context for one turn.
    ///
    /// Stages run in priority order; equal priorities keep recipe order. A
    /// fragment that does not fit the budget is truncated at the remaining
    /// token allowance, and later stages are dropped. The result is
    /// deterministic for identical inputs.
    pub async fn build(
        &self,
        ctx: &BuildCtx<'_>,
        recipe: &ContextRecipe,
        cancel: &CancelToken,
    ) -> Result<BuiltContext, KernelError> {
        if recipe.budget_tokens == 0 {
            return Err(KernelError::InvalidConfig {
                message: "context budget must be positive".to_owned(),
            });
        }

        let mut seen = BTreeSet::new();
        for name in &recipe.stages {
            if !seen.insert(name.clone()) {
                return Err(KernelError::InvalidConfig {
                    message: format!("duplicate context stage `{name}`"),
                });
            }
        }

        let mut selected: Vec<(usize, Arc<dyn ContextStage>)> = Vec::new();
        for (index, name) in recipe.stages.iter().enumerate() {
            let stage = self
                .stages
                .iter()
                .find(|stage| stage.name() == name)
                .cloned()
                .ok_or_else(|| KernelError::InvalidConfig {
                    message: format!("unknown context stage `{name}`"),
                })?;
            selected.push((index, stage));
        }
        selected.sort_by_key(|(index, stage)| (stage.priority(), *index));

        let mut fragments = Vec::new();
        let mut stages = Vec::new();
        let mut used = 0u32;

        'stages: for (_, stage) in selected {
            let stage_ctx = ctx.with_budget(recipe.budget_tokens.saturating_sub(used));
            let built = tokio::select! {
                _ = cancel.cancelled() => {
                    return Err(KernelError::Cancelled { stage: "context" });
                }
                built = stage.build(&stage_ctx) => built,
            };
            let Some(built) = built else {
                continue;
            };
            if built.is_empty() {
                continue;
            }

            let mut emitted = false;
            for fragment in built {
                let tokens = fragment.estimated_tokens();
                if used.saturating_add(tokens) <= recipe.budget_tokens {
                    used = used.saturating_add(tokens);
                    fragments.push(fragment);
                    emitted = true;
                    continue;
                }

                let remaining = recipe.budget_tokens.saturating_sub(used);
                if remaining == 0 {
                    break 'stages;
                }
                let keep = (remaining as usize).saturating_mul(4);
                let text: String = fragment.text.chars().take(keep).collect();
                if text.is_empty() {
                    break 'stages;
                }
                used = recipe.budget_tokens;
                fragments.push(Fragment {
                    role: fragment.role,
                    text,
                });
                stages.push(stage.name().to_owned());
                break 'stages;
            }
            if emitted {
                stages.push(stage.name().to_owned());
            }
        }

        Ok(BuiltContext {
            fragments,
            stages,
            estimated_tokens: used,
            budget_tokens: recipe.budget_tokens,
        })
    }
}

/// Produces the system prompt fragment.
pub struct SystemStage;

impl SystemStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SystemStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContextStage for SystemStage {
    fn name(&self) -> &'static str {
        "system"
    }

    fn priority(&self) -> i32 {
        0
    }

    async fn build(&self, ctx: &BuildCtx<'_>) -> Option<Vec<Fragment>> {
        if ctx.system_prompt.trim().is_empty() {
            return None;
        }
        Some(vec![Fragment {
            role: MessageRole::System,
            text: ctx.system_prompt.to_owned(),
        }])
    }
}

/// Produces the latest user message fragment.
pub struct LatestStage;

impl LatestStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LatestStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContextStage for LatestStage {
    fn name(&self) -> &'static str {
        "latest"
    }

    fn priority(&self) -> i32 {
        10
    }

    async fn build(&self, ctx: &BuildCtx<'_>) -> Option<Vec<Fragment>> {
        if ctx.input.is_empty() {
            return None;
        }
        Some(vec![Fragment {
            role: MessageRole::User,
            text: ctx.input.to_owned(),
        }])
    }
}

/// Emits recorded history, keeping the newest messages that fit.
///
/// History runs before `latest`, so it leaves room for the current input.
/// Only user and assistant text is emitted; tool-call structure is not
/// replayed from earlier turns.
pub struct HistoryStage;

impl HistoryStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HistoryStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContextStage for HistoryStage {
    fn name(&self) -> &'static str {
        "history"
    }

    fn priority(&self) -> i32 {
        9
    }

    async fn build(&self, ctx: &BuildCtx<'_>) -> Option<Vec<Fragment>> {
        if ctx.history.is_empty() {
            return None;
        }
        // Never spend the room the current input needs.
        let budget = ctx.budget_tokens.checked_sub(estimate_tokens(ctx.input))?;
        if budget == 0 {
            return None;
        }

        // A turn starts at a user message. Keep or drop whole turns so an
        // assistant message never appears without the request it answered.
        let history = ctx.history;
        let mut turns: Vec<&[Arc<Message>]> = Vec::new();
        let mut start = 0usize;
        for (index, message) in history.iter().enumerate() {
            if index > 0 && message.role == MessageRole::User {
                turns.push(&history[start..index]);
                start = index;
            }
        }
        turns.push(&history[start..]);

        let mut chosen: Vec<&Arc<Message>> = Vec::new();
        let mut used = 0u32;
        for turn in turns.iter().rev() {
            let tokens = turn.iter().fold(0u32, |total, message| {
                total.saturating_add(message.estimated_tokens())
            });
            if used.saturating_add(tokens) > budget && !chosen.is_empty() {
                break;
            }
            used = used.saturating_add(tokens);
            chosen.extend(turn.iter().rev());
        }
        if chosen.is_empty() {
            return None;
        }
        chosen.reverse();
        Some(
            chosen
                .into_iter()
                .filter(|message| {
                    matches!(message.role, MessageRole::User | MessageRole::Assistant)
                        && !message.content.trim().is_empty()
                })
                .map(|message| Fragment {
                    role: message.role,
                    text: message.content.clone(),
                })
                .collect(),
        )
    }
}
