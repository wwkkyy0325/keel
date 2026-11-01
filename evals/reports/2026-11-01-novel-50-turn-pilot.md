# Novel 50-chapter pilot — 2026-11-01

This run asked whether Keel can carry a long-form generation project: a Chinese
novel written chapter by chapter, with a story bible and a continuity state
file maintained across fifty turns. It did not finish the novel. The failure
is in the task protocol, not in the context governor.

## Setup

- Profile: `variants/coding/profile.deepseek.novel.toml` (new variant).
- Model: `deepseek-flash`; context window 128k, working set 16k, output cap
  raised to 8,192, tools 24, model calls 32 via the new host flag.
- Project: `D:\workspace-new\wwkkyy0325\ascent-novel`.
- 54 turns: 50 chapter prompts plus four no-tool probes after chapters 10, 25,
  40 and 50.
- Each chapter prompt asked for 2,500-3,000 Chinese characters, a 150-character
  answer summary, and a state update.

## Result

- **34 turns `Done`, 20 turns `Error`**.
- **40 of 54 turns changed no file.**
- 14 files, 31,464 Chinese characters: `ch-001` to `ch-006` (plus a stray
  `ch-006-tail.md`), `ch-018` to `ch-020`, `ch-025`, `ch-026`, `ch-046`,
  `ch-049`.
- Missing: chapters 7-17, 21-24, 27-45, 47-48. The validator reports 36 missing
  chapters, five chapters below 2,200 characters, and a state/chapter mismatch.
- Total usage: 1,045,176 input tokens and 107,375 output tokens.
- Recall fired on **40 of 54 turns**, averaging **620 tokens** per turn; the
  measured values reached 742 tokens against a nominal 400-token cap.
- Four memo revisions (2,162, 570, 1,105 and 2,132 characters); two updates
  failed with `summarizer returned no text`.
- Context estimates stayed between roughly 2.4k and 11.7k against a 15.4k
  budget. There was no `ContextOverflow`.

## Failure modes

1. **One turn asked for too much.** Reading the state and bible, writing a
   ~3,000-character chapter through a tool call, updating an increasingly long
   state file, and answering is at the edge of the output and tool budgets.
   Many turns ended with a status report instead of a `write_file`; several
   ended with no answer.
2. **The state file hijacked the task.** The model added a long "缺口" section
   after chapter 5 and then followed its own gap list instead of the prompt's
   chapter number. It later wrote 18-20 and 25-26 while the prompts asked for
   7-13. The state grew to a second narrative and changed its own format
   (`Last chapter written: **26**`), which also broke the validator until the
   parser was fixed.
3. **Long tool arguments were truncated.** The earlier single-turn smoke
   reproduced `invalid tool arguments: EOF while parsing a string at line 1
   column 6100`. The 20 no-answer turns cluster at large outputs; several show
   exactly 4,096 output tokens. A chapter written entirely inside one
   `write_file` JSON is too close to the provider limit.
4. **Recall over-triggered.** The chapter prompts share a formula
   (`继续写... 写 chapters/ch-NNN.md ... 更新 state`), so the lexical score
   matched old segments on most turns. Recall became a near-constant 600-700
   token block and displaced detail. The first recalled item also bypassed the
   400-token cap because the loop admitted one item unconditionally; that is
   now fixed.
5. **The memo was not the bottleneck.** It produced four revisions and two
   failures, but the run never reached a stable chapter sequence where memo
   quality could be judged.

## What the run proves

- Per-call context stayed well inside the governor; compression did not cause
  the failure.
- The 400-token recall cap was not actually enforced for the first hit; the
  measured 620-742 tokens confirms the user's suspicion about hard caps.
- Lexical recall on formulaic prompts is noisy. A higher score threshold or a
  distinctiveness requirement is needed before it is useful in this kind of
  project.
- The binding constraints are the per-turn action budget and the state
  protocol, not the window.

## Next iteration

1. **Split the workflow**: a chapter turn only writes the chapter; the next
   turn updates the state and answers. This avoids competing tool calls.
2. **Compact, fixed state**: keep a short table in `state/continuity.md` and
   move notes to an append-only ledger the model does not rewrite.
3. **Shorten chapters** to 1,800-2,200 characters; 50 chapters still yield
   about 100k characters.
4. **Strict prompt**: write only the chapter number in this prompt; ignore any
   missing chapters and any gap list.
5. **Raise the action budgets** and print `error:` on session turns, so the
   next failure is visible.
6. **Run a ten-chapter smoke first**, then the full fifty if it holds.

The 1M-character target remains out of reach for one 50-turn session at the
current output cap; the project is structured for later batches.
