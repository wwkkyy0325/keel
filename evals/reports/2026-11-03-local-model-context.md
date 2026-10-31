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

## Limits

- One local model, one quantisation, one scripted English workload.
- The server context was fixed at 32k; the arms varied only Keel's working set.
- The uncensored 14B is not a strong instruction follower; the hallucination
  result should not be generalised to all local models.
- The novel/tool workload was not run locally; tool calling and long Chinese
  output remain untested on this endpoint.

## Next

- Repeat the 24-turn probe on `gemma4-26b-heretic-i1-Q3_K_L` to separate model
  quality from the 14B's behaviour.
- Run one novel chapter through the local endpoint to exercise tool calls and
  the 8k output reserve.
- Compare server-reported prompt tokens with Keel's estimate on the same
  request; the local endpoint reports usage directly.
