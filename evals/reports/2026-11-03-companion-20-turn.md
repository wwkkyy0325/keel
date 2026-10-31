# Companion 20-turn real run — 2026-11-03

This run exists because the synthetic window matrix could not answer the real
questions: actual token counts, whether compression fires with real text, and
whether the model still answers after history is compressed.

## Setup

- Profile: `variants/companion/profile.deepseek.toml`, no tools.
- Model: `deepseek-flash` through the OpenAI-compatible endpoint.
- `context_window = 8192`, `budget_tokens = 7168`, output reserve 1024, so the
  run sits at the no-tool floor.
- 20 turns of short Chinese conversation with a remembered preference, a topic
  change, follow-ups and a summary request.
- Raw console capture: `evals/reports/runs/companion-20-turn.log` (gitignored).

## Result

- **20/20 turns reached `Done`**; no `ContextOverflow`, no wrap-up injection,
  no cancellation.
- 22,923 input tokens and 2,966 output tokens, about 25.9k total.
- The remembered fact was used across later turns: after "记住：我早上六点起床"
  the model answered "我一般几点起床？" with "六点", and still used it in the
  plan and final advice turns.
- History used 1 verbatim turn plus up to 11 segment records by turn 20; no
  digest compression was needed at this size.

## What the run exposed

1. **The token estimate was wrong for Chinese.** The kernel used four
   characters per token; turn 1 estimated 93 tokens while the provider reported
   485. Chinese is closer to one token per character, and the estimator now
   counts CJK that way.
2. **Tool schemas and provider framing were invisible to the budget.** The
   control-tool schema and the chat template cost about 390 tokens on every
   request. The driver now measures the gap from reported usage and reserves
   it for later turns: turn 2 onward showed `budget=6776` instead of `7168`.
3. **Chinese topic segmentation was too aggressive.** Whole Chinese sentences
   were single tokens, so almost every turn looked like a new topic and the
   previous turn immediately became a record. Terms now use character bigrams
   and a small follow-up marker list ("刚才", "继续", "earlier"), and the newest
   segment stays verbatim even after it seals.
4. **Compression was exercised with real text.** The model answered follow-up
   questions from segment records, not from raw history.

## Limits

- One provider, one language, one persona, no tools.
- 20 turns at 8k did not reach digest compression or wrap-up; those remain
  covered by deterministic tests and the synthetic matrix.
- Coding with tools has real single-turn evidence (`task-001`) but no real
  20-turn session yet.
