use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use keel_core::{
    BuildCtx, CancelToken, CompactionStep, ContextRecipe, ContextStage, Fragment, KernelError,
    MessageRole, TurnId,
};
use keel_runtime::ContextBuilder;

struct StaticStage {
    name: &'static str,
    priority: i32,
    text: String,
}

#[async_trait]
impl ContextStage for StaticStage {
    fn name(&self) -> &'static str {
        self.name
    }

    fn priority(&self) -> i32 {
        self.priority
    }

    async fn build(&self, _ctx: &BuildCtx<'_>) -> Option<Fragment> {
        Some(Fragment {
            role: MessageRole::System,
            text: self.text.clone(),
        })
    }
}

struct SlowStage;

#[async_trait]
impl ContextStage for SlowStage {
    fn name(&self) -> &'static str {
        "slow"
    }

    fn priority(&self) -> i32 {
        0
    }

    async fn build(&self, _ctx: &BuildCtx<'_>) -> Option<Fragment> {
        tokio::time::sleep(Duration::from_secs(30)).await;
        Some(Fragment {
            role: MessageRole::System,
            text: "late".to_owned(),
        })
    }
}

fn recipe(stages: &[&str], budget_tokens: u32) -> ContextRecipe {
    ContextRecipe {
        stages: stages.iter().map(|stage| (*stage).to_owned()).collect(),
        budget_tokens,
        compaction: Vec::<CompactionStep>::new(),
    }
}

fn build_ctx<'a>(input: &'a str, system_prompt: &'a str) -> BuildCtx<'a> {
    BuildCtx {
        turn_id: TurnId::new(1),
        input,
        history: &[],
        system_prompt,
    }
}

#[tokio::test]
async fn stages_run_in_priority_order() {
    let builder = ContextBuilder::new(vec![
        Arc::new(StaticStage {
            name: "low",
            priority: 10,
            text: "low".to_owned(),
        }),
        Arc::new(StaticStage {
            name: "high",
            priority: 0,
            text: "high".to_owned(),
        }),
    ]);
    let ctx = build_ctx("input", "system");
    let built = builder
        .build(&ctx, &recipe(&["low", "high"], 100), &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(built.stages, vec!["high".to_owned(), "low".to_owned()]);
}

#[tokio::test]
async fn equal_priorities_keep_recipe_order() {
    let builder = ContextBuilder::new(vec![
        Arc::new(StaticStage {
            name: "first",
            priority: 5,
            text: "first".to_owned(),
        }),
        Arc::new(StaticStage {
            name: "second",
            priority: 5,
            text: "second".to_owned(),
        }),
    ]);
    let ctx = build_ctx("input", "system");
    let built = builder
        .build(
            &ctx,
            &recipe(&["second", "first"], 100),
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(built.stages, vec!["second".to_owned(), "first".to_owned()]);
}

#[tokio::test]
async fn build_is_deterministic() {
    let builder = ContextBuilder::new(vec![
        Arc::new(StaticStage {
            name: "a",
            priority: 0,
            text: "aaaa".to_owned(),
        }),
        Arc::new(StaticStage {
            name: "b",
            priority: 1,
            text: "bbbb".to_owned(),
        }),
    ]);
    let ctx = build_ctx("input", "system");
    let first = builder
        .build(&ctx, &recipe(&["a", "b"], 100), &CancelToken::new())
        .await
        .unwrap();
    let second = builder
        .build(&ctx, &recipe(&["a", "b"], 100), &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(first, second);
}

#[tokio::test]
async fn budget_truncates_and_drops_later_stages() {
    let builder = ContextBuilder::new(vec![
        Arc::new(StaticStage {
            name: "a",
            priority: 0,
            text: "aaaa".to_owned(),
        }),
        Arc::new(StaticStage {
            name: "b",
            priority: 1,
            text: "bbbbbbbb".to_owned(),
        }),
        Arc::new(StaticStage {
            name: "c",
            priority: 2,
            text: "cccc".to_owned(),
        }),
    ]);
    let ctx = build_ctx("input", "system");
    let built = builder
        .build(&ctx, &recipe(&["a", "b", "c"], 2), &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(built.estimated_tokens, 2);
    assert_eq!(built.stages, vec!["a".to_owned(), "b".to_owned()]);
    assert_eq!(built.fragments[0].text, "aaaa");
    assert_eq!(built.fragments[1].text, "bbbb");
}

#[tokio::test]
async fn unknown_stage_is_an_error() {
    let builder = ContextBuilder::new(vec![]);
    let ctx = build_ctx("input", "system");
    let error = builder
        .build(&ctx, &recipe(&["missing"], 100), &CancelToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, KernelError::InvalidConfig { .. }));
}

#[tokio::test]
async fn duplicate_stage_in_recipe_is_an_error() {
    let builder = ContextBuilder::new(vec![Arc::new(StaticStage {
        name: "a",
        priority: 0,
        text: "a".to_owned(),
    })]);
    let ctx = build_ctx("input", "system");
    let error = builder
        .build(&ctx, &recipe(&["a", "a"], 100), &CancelToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, KernelError::InvalidConfig { .. }));
}

#[tokio::test]
async fn cancellation_stops_context_build() {
    let builder = ContextBuilder::new(vec![Arc::new(SlowStage)]);
    let ctx = build_ctx("input", "system");
    let cancel = CancelToken::new();
    cancel.cancel();
    let error = builder
        .build(&ctx, &recipe(&["slow"], 100), &cancel)
        .await
        .unwrap_err();
    assert!(matches!(error, KernelError::Cancelled { .. }));
}
