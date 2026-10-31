# Recall probe — memo sufficiency and the recall counterfactual

This run closes the limit named in `2026-11-03-companion-24-turn-recall.md`:
that report had recall enabled but no arm with recall disabled, so it could not
show that recall changed an answer. This run uses the same mechanism with a
model-level A/B.

## Setup

- Profile: `variants/companion/profile.deepseek.toml`, no tools, 8k window,
  7,168-token working set, 1,024 output reserve.
- Model: `deepseek-flash`.
- 24 scripted turns with frequent topic switches. Turn 1 names the `Marlin`
  project and `config/marlin.toml`; turn 6 mentions `aglio e olio`.
- Probes: turn 23 asks for the Marlin config path; turn 24 asks for the pasta
  recipe.
- Arms: `full` and `--no-recall`. One run per arm.
- Workload and logs: `D:\workspace-new\wwkkyy0325\recall-probe`
  (`companion-24.txt`, `runs/c24-full.log`, `runs/c24-no-recall.log`).

## Results

| Arm | Done | Overflow | Memo revisions | Recall | Input | Output |
|---|---|---|---|---|---|---|
| full | 24/24 | 0 | 21 | 2 (segments 9, 0) | 60,698 | 22,732 |
| no-recall | 24/24 | 0 | 21 | 0 | 61,681 | 22,822 |

Turn 23, the decisive probe:

- full: `It's at config/marlin.toml — the same path you mentioned when you first
  described the project.` Recall returned segment 0 with 257 tokens.
- no-recall: `I don't know — there's no Marlin project in anything we've talked
  about, and I don't have access to your files or systems to go look.`

Turn 24, the lexical-miss probe:

- full: `I don't actually have a pasta recipe from you — nothing with
  ingredients or steps. The closest thread is you asking how to stop the garlic
  burning in aglio e olio...`
- no-recall: `I don't have a pasta recipe from you — nothing about pasta has
  come up in what I can see of our conversation.`

## Findings

### 1. Recall is load-bearing for an absorbed early detail

Turn 23 is the counterfactual the companion report lacked. With recall, the
kernel returned segment 0 and the answer named `config/marlin.toml`. Without
it, the model denied that the project existed. The memo had already rewritten
the early turns out of the projection; recall was the only path back.

### 2. Lexical recall needs a shared term

Turn 24 asked for "the pasta recipe". The turn 6 segment says `aglio e olio`,
`garlic`, `olive oil` and `pasta`, but never `recipe`. The score fell below
`RECALL_MIN_SCORE = 0.34`, so recall did not fire. The full arm still mentioned
`aglio e olio` because its memo kept the phrase; it did not claim a recipe. The
no-recall arm lost the phrase entirely.

This is the known cost of deterministic lexical recall, now measured: naming a
detail works, paraphrasing its category does not.

### 3. Memo rewrites are stochastic at the same inputs

Both arms made 21 revisions under the same 677-token budget. Memo sizes moved
between roughly 1,000 and 2,700 characters; the full-arm memo kept
`aglio e olio`, the no-recall memo did not. Same workload, different rewrite.
Single-run answer differences on details near the memo boundary should not be
attributed to the kernel without repeated runs.

### 4. A larger memo budget can make recall redundant

A secondary probe used the 12,288-token session profile (1,170-token memo
budget) and twelve short settings in one segment. Both arms answered all three
recall questions correctly; the full arm fired recall on turns 14 and 15, but
the memo from turn 13 already contained the answers. Memo budget relative to
the sealed segment, not the presence of recall, decides whether recall is
needed.

Workload: `prompts-v2.txt`, `runs/v2-full.log`, `runs/v2-no-recall.log`.

## Limits

- One run per arm; memo rewrites differ between runs at the same inputs.
- Scripted English topics; the probe wording determines whether a lexical hit
  is possible, so this is not a recall-rate estimate.
- The secondary probe uses a different profile and workload, so its numbers are
  not pooled with the 24-turn arms.
- Lexical recall still has no answer for synonymy. Vector recall remains
  deferred until real tasks show misses that term overlap cannot cover.
- The personas did not yet declare kernel affordances when this probe ran. The
  probe report describes the run as executed; future runs use the updated
  persona and should not be compared token-for-token with these numbers.

## Next

- Add a probe whose question uses a synonym rather than the recorded term, to
  measure the miss rate directly.
- Repeat the turn-23 probe twice to separate memo variance from the recall
  effect.
- Keep the memo budget and coverage rules unchanged until a miss is observed
  with a term that is present in the segment.
