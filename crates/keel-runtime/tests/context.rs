use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use keel_core::{
    BuildCtx, CancelToken, CompactionStep, ContextRecipe, ContextStage, Fact, FactScope, Fragment,
    KernelError, MessageRole, TurnId,
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

    async fn build(&self, _ctx: &BuildCtx<'_>) -> Option<Vec<Fragment>> {
        Some(vec![Fragment {
            role: MessageRole::System,
            text: self.text.clone(),
        }])
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

    async fn build(&self, _ctx: &BuildCtx<'_>) -> Option<Vec<Fragment>> {
        tokio::time::sleep(Duration::from_secs(30)).await;
        Some(vec![Fragment {
            role: MessageRole::System,
            text: "late".to_owned(),
        }])
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
        records: &[],
        facts: &[],
        system_prompt,
        budget_tokens: 4096,
        detail_share: 0.8,
        events: None,
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

struct MultiStage {
    name: &'static str,
    priority: i32,
}

#[async_trait]
impl ContextStage for MultiStage {
    fn name(&self) -> &'static str {
        self.name
    }

    fn priority(&self) -> i32 {
        self.priority
    }

    async fn build(&self, _ctx: &BuildCtx<'_>) -> Option<Vec<Fragment>> {
        Some(vec![
            Fragment {
                role: MessageRole::User,
                text: "one".to_owned(),
            },
            Fragment {
                role: MessageRole::Assistant,
                text: "two".to_owned(),
            },
        ])
    }
}

struct BudgetProbe {
    name: &'static str,
    text: String,
    seen: Arc<Mutex<Vec<u32>>>,
}

#[async_trait]
impl ContextStage for BudgetProbe {
    fn name(&self) -> &'static str {
        self.name
    }

    fn priority(&self) -> i32 {
        0
    }

    async fn build(&self, ctx: &BuildCtx<'_>) -> Option<Vec<Fragment>> {
        self.seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(ctx.budget_tokens);
        Some(vec![Fragment {
            role: MessageRole::System,
            text: self.text.clone(),
        }])
    }
}

#[tokio::test]
async fn a_stage_can_emit_several_messages() {
    let builder = ContextBuilder::new(vec![Arc::new(MultiStage {
        name: "multi",
        priority: 0,
    })]);
    let ctx = build_ctx("input", "system");
    let built = builder
        .build(&ctx, &recipe(&["multi"], 100), &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(built.fragments.len(), 2);
    assert_eq!(built.fragments[0].role, MessageRole::User);
    assert_eq!(built.fragments[0].text, "one");
    assert_eq!(built.fragments[1].role, MessageRole::Assistant);
    assert_eq!(built.fragments[1].text, "two");
}

#[tokio::test]
async fn each_stage_sees_the_remaining_budget() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let builder = ContextBuilder::new(vec![
        Arc::new(BudgetProbe {
            name: "first",
            text: "x".repeat(40),
            seen: seen.clone(),
        }),
        Arc::new(BudgetProbe {
            name: "second",
            text: String::new(),
            seen: seen.clone(),
        }),
    ]);
    let ctx = build_ctx("input", "system");
    builder
        .build(
            &ctx,
            &recipe(&["first", "second"], 100),
            &CancelToken::new(),
        )
        .await
        .unwrap();

    let seen = seen.lock().unwrap();
    assert_eq!(*seen, vec![100, 90]);
}

#[tokio::test]
async fn facts_are_emitted_as_a_memory_block() {
    let builder = ContextBuilder::new(vec![Arc::new(keel_runtime::MemoryStage::new())]);
    let facts = vec![
        Fact {
            key: "constraint:use tabs".to_owned(),
            statement: "always use tabs".to_owned(),
            scope: FactScope::Global,
            confirmed: true,
            segment_id: 0,
        },
        Fact {
            key: "artifact:a.rs".to_owned(),
            statement: "modified a.rs".to_owned(),
            scope: FactScope::Session,
            confirmed: true,
            segment_id: 1,
        },
    ];
    let ctx = BuildCtx {
        turn_id: TurnId::new(1),
        input: "input",
        history: &[],
        records: &[],
        facts: &facts,
        system_prompt: "system",
        budget_tokens: 4096,
        detail_share: 0.8,
        events: None,
    };
    let built = builder
        .build(&ctx, &recipe(&["memory"], 100), &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(built.fragments.len(), 1);
    assert_eq!(built.fragments[0].role, MessageRole::System);
    assert!(built.fragments[0].text.starts_with("[memory]"));
    assert!(built.fragments[0].text.contains("always use tabs (global)"));
    assert!(built.fragments[0].text.contains("modified a.rs (session)"));
}
