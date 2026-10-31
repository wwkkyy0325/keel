# Local model context probe — 2026-11-03

A real local model on a separate Windows machine, driven by Keel through the
same OpenAI-compatible provider. The goal was to measure the working set a
local model needs for a real 24-turn task, and to check whether the kernel's
memo and recall behave with a non-DeepSeek endpoint.

## Setup

- Host: `192.168.3.143`, Windows, i9-14900KF, 16 GB RAM, RTX 5070 Ti 16 GB.
- Server: llama.cpp b10964, `FableVibes-14B-Q6_K.gguf` (Qwen3.6 MoE,
  uncensored), 32,768-token context, full GPU offload, q8 KV cache.
- Endpoint: `http://192.168.3.143:8080/v1`, no API key.
- Keel: `variants`-style profile at
  `D:\workspace-new\wwkkyy0325\recall-probe\local\profile.local.toml`,
  no tools, `max_output_tokens = 2048`, `context_window = 32768`.
- Workload: the 24-turn companion script (`companion-24.txt`) with the Marlin
  and pasta probes at turns 23 and 24.
- Working-set arms: 8,192, 16,384 and 24,576. Server context stayed at 32,768.

## Results

| Working set | Done | Overflow | Memo revisions | Summary failures | Recall | Input | Output | Ctx max |
|---|---|---|---|---|---|---|---|---|
| 8k, before fix | 24/24 | 0 | 18 | 17 | 1 | 46,064 | 6,451 | 1,761 |
| 8k, after fix | 24/24 | 0 | 19 | 0 | 2 | 62,146 | 18,778 | 1,695 |
| 16k | 24/24 | 0 | 14 | 0 | 1 | 42,402 | 8,936 | 1,576 |
| 24k | 24/24 | 0 | 14 | 0 | 1 | 54,213 | 11,971 | 2,432 |

All arms completed with no overflow. The assembled context peaked at 2,432
tokens, far below every working-set budget. Input varied with answer
verbosity, not with the budget.

Probe answers:

- 8k: `The config path for the Marlin project is /home/user/marlin/config.yml.`
  (wrong; the recall block with `config/marlin.toml` was present, 258 tokens).
- 16k: an XDG configuration explanation (wrong; recall block present).
- 24k: `/usr/etc/marlin/` (wrong; no recall needed at that point).
- Pasta probe, all arms: a hallucinated recipe or story (the lexical recall
  miss is expected; the model invented the rest).

## Finding 1: the summarizer advertised the interaction tool

Before the fix, every seal failed with `summarizer returned no text`: 17 of 18
revisions fell back to the structured memo. A direct call reproduced it: with
the `keel_interact` control tool in `tools`, the model finished with
`finish_reason = tool_calls` and `kind = plan_approval`; its `content` was
empty. The summarizer only reads text, so it failed.

`ModelRequest` now carries `control_tool`. The driver sets it for main calls
and clears it for the summarizer and for the reserved final call. The wire
builder omits `tools` entirely when the list is empty. After the fix, the same
8k arm had zero summary failures and 19 real model-written revisions.

This is a kernel compatibility bug that DeepSeek hid by ignoring the control
tool. The local endpoint exposed it.

## Finding 2: context is not the bottleneck for this task

At 8k, 16k and 24k the task completed 24/24 with zero overflow. The kernel's
assembled context never exceeded 2,432 tokens, so the 8k floor is not being
approached. The working set could not be lowered below 8k because that is the
profile validation floor for a no-tool profile.

## Finding 3: model quality is the bottleneck

The 14B model hallucinated the Marlin path at every working-set size, including
when the recall block carried the correct string. More context did not improve
correctness. This is a model limitation, not a context or kernel result; the
same probe with DeepSeek answered `config/marlin.toml` from recall.

## Follow-up: Gemma 4 26B

The same host also has `gemma4-26b-heretic-i1-Q3_K_L.gguf`. It was served at
32k with `-ngl 99 --n-cpu-moe 4 -fa on --jinja -rea off
--reasoning-format deepseek`.

| Working set | Done | Overflow | Memo revisions | Summary failures | Recall | Input | Output | Ctx max |
|---|---|---|---|---|---|---|---|---|
| 8k | 24/24 | 0 | 21 | 0 | 2 | 54,117 | 20,139 | 1,744 |
| 16k | 24/24 | 0 | 21 | 0 | 2 | 70,202 | 27,971 | 2,640 |

Probe answers:

- 8k: `The configuration file for your Marlin project is located at
  config/marlin.toml.` (correct, from recall segment 0). The pasta probe
  said it could not find a recipe: the 804-token memo had dropped
  `aglio e olio`, and "recipe" does not match the segment terms.
- 16k: `config/marlin.toml` (correct) and `Aglio e Olio` (correct, from the
  memo). The larger memo kept the incidental phrase.

This separates model quality from context. The 26B answered the explicit
recall probe at 8k, where the 14B hallucinated at 8k, 16k and 24k. The 8k/16k
difference appears only on the paraphrased pasta probe, where recall cannot
match and the memo has to carry the phrase.

## Novel chapter with tools

One chapter was run on the 26B to exercise domain tools, the task state and the
output reserve:

- Profile: `profile.local-novel.toml`, 16k working set, 8k output reserve,
  tools `read_file`, `write_file`, `list_dir`, `code_search`, `todo`.
- Fifteen seeded tasks. The prompt is the first line of the ablation 15-chapter
  script.
- Result: 6 model calls, 18,696 input, 2,251 output, 32.5 seconds. It wrote
  `chapters/ch-001.md` (1,184 CJK characters), updated
  `state/continuity.md` to 49 lines, and marked task 1 completed. Assembled
  context was 1,035 tokens.
- Tool calls, task state and file writes all worked through the local
  endpoint. The chapter was 16 characters short of the 1,200 floor; that is
  model compliance, not a context failure.

## Gemma template note

The first 26B start returned HTTP 500:
`the model produced output that does not match the expected peg-gemma4 format`.
With `--jinja` and reasoning on, the model emitted `<|channel>thought` and
llama.cpp's parser rejected it. `-rea off --reasoning-format deepseek` produced
clean content. This is a llama.cpp/model-template issue, not a Keel bug.

## Limits

- Two local models, one quantisation each, one scripted English workload and
  one Chinese chapter.
- The server context was fixed at 32k; the arms varied only Keel's working set.
- The 14B hallucination result is model-specific; the 26B answered the explicit
  probe at 8k.
- The 8k pasta miss is a memo-coverage result, not proof that 8k is generally
  insufficient; 16k retained the phrase.
- The chapter run is one sample; its 1,184-character length is below the
  1,200 floor, and no long multi-chapter local run was attempted.

## Next

- Run the 26B chapter arm at 8k to see whether the 16k advantage is only output
  verbosity.
- Repeat the pasta probe at 8k and 16k to separate memo variance from budget.
- Compare server-reported prompt tokens with Keel's estimate on the same
  request; the local endpoint reports usage directly.
