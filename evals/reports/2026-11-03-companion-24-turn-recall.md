# Companion 24-turn run with recall probes — 2026-11-03

This run checks the rolling session memo and the deterministic recall path on a
real model. The question is not whether the session fits: the budget does. The
question is whether an early detail survives a long sequence of topic switches,
and whether recall can bring it back when the memo has dropped it.

## Setup

- Profile: `variants/companion/profile.deepseek.toml`, no tools.
- Model: `deepseek-flash` through the OpenAI-compatible endpoint.
- `context_window = 8192`, `budget_tokens = 7168`, output reserve 1024.
- 24 scripted turns. Most turns switch topic so a segment seals and the memo is
  rewritten; turns 23 and 24 probe details from turns 1 and 6.
- Early details:
  - turn 1, project codename and config path: `Marlin`,
    `config/marlin.toml`;
  - turn 6, pasta recipe: `aglio e olio`.
- Probes:
  - turn 23: "What is the Marlin project's config path?"
  - turn 24: "What was the pasta recipe?"

## Result

- **24/24 turns reached `Done`**; no `ContextOverflow`, no wrap-up injection, no
  cancellation, no summary failure.
- **20 memo revisions.** The text moved from 484 to 2679 characters against a
  675–677 token budget. It did not only grow: revision 5 was 2645 characters,
  revision 6 was 2041, revision 18 was 1902. The rewrite is rebalancing, not
  appending.
- Context estimates stayed between 110 and 1479 tokens against a 6755–6774
  budget. Per-turn input was 504–3321 tokens and output 64–1699 tokens.
- Recall fired on the probes:
  - segment 0 with 96 tokens; answer: "It's `config/marlin.toml` — that's what
    you told me at the start of our session."
  - segment 4 with 205 tokens; answer: "It was **aglio e olio** — garlic,
    olive oil, pasta, basically nothing else."
- Recall also fired on several intermediate turns (segments 2, 5, 11, 14 and
  the pair 18 and 14). Those blocks stayed bounded at 200–397 tokens.
- The deterministic test `an_absorbed_topic_is_recalled_by_its_terms` covers the
  mechanism-level comparison: the memo block does not contain the early term,
  and the recall block does.

## What the run shows

1. **The memo stays bounded and elastic.** At 675 tokens it grows while topics
   accumulate, then rewrites shorter when the content no longer fits. No
   record-block cliff appeared.
2. **The memo alone is lossy.** Early details disappear long before turn 23;
   that is expected, and it is why recall exists.
3. **Lexical recall is enough for named details.** Both probes named a
   distinctive term from the early turn (`Marlin`, `config`, `pasta`,
   `recipe`), and the segment signature matched without a model or an index.
4. **Recall is cheap.** One or two records plus a 300-character excerpt cost
   about 100–400 tokens, a small fraction of the history allowance.

## Limits

- One provider, one run, scripted topics. The model-level A/B with recall
  disabled is now in `2026-11-03-recall-probe.md`: without recall the model
  denied that the early project existed.
- Lexical recall needs a shared term. Paraphrase and synonymy will miss; that is
  the known cost of not having vectors.
- The run measures mechanism and a few probe answers, not general answer
  quality.
- Vector recall remains deferred. It should be reconsidered only if real tasks
  show misses that lexical matching cannot cover.
