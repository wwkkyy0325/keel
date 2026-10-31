use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use keel_core::{
    BuildCtx, CancelToken, ContextRecipe, ContextStage, Fragment, KernelError, MessageRole,
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

        for (_, stage) in selected {
            let fragment = tokio::select! {
                _ = cancel.cancelled() => {
                    return Err(KernelError::Cancelled { stage: "context" });
                }
                fragment = stage.build(ctx) => fragment,
            };
            let Some(fragment) = fragment else {
                continue;
            };

            let tokens = fragment.estimated_tokens();
            if used.saturating_add(tokens) <= recipe.budget_tokens {
                used = used.saturating_add(tokens);
                stages.push(stage.name().to_owned());
                fragments.push(fragment);
                continue;
            }

            let remaining = recipe.budget_tokens.saturating_sub(used);
            if remaining == 0 {
                break;
            }
            let keep = (remaining as usize).saturating_mul(4);
            let text: String = fragment.text.chars().take(keep).collect();
            if text.is_empty() {
                break;
            }
            used = recipe.budget_tokens;
            stages.push(stage.name().to_owned());
            fragments.push(Fragment {
                role: fragment.role,
                text,
            });
            break;
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

    async fn build(&self, ctx: &BuildCtx<'_>) -> Option<Fragment> {
        if ctx.system_prompt.trim().is_empty() {
            return None;
        }
        Some(Fragment {
            role: MessageRole::System,
            text: ctx.system_prompt.to_owned(),
        })
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

    async fn build(&self, ctx: &BuildCtx<'_>) -> Option<Fragment> {
        if ctx.input.is_empty() {
            return None;
        }
        Some(Fragment {
            role: MessageRole::User,
            text: ctx.input.to_owned(),
        })
    }
}
