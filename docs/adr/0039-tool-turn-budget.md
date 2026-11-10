# ADR 0039 — A turn's tool results share one budget

## Status

Rejected. Not released. The experiment was built on top of 0.2.1 and
reverted; 0.2.1 remains the released version.

## Context

Two bounds already apply to tool output. `summarize_output` caps one result
at its head and tail (2,000 + 1,000 characters) and spills the rest. When the
message list overflows the working set, `elide_old_tool_results` replaces
every tool result before the last round with a generic marker. Neither bounds
the sum: a turn with sixty or a hundred calls keeps one summarized result per
call until the working set is already exceeded.

The 0.2.1 validation replay measured the cost. One turn made 100 tool calls;
its segment held 214,704 characters of answer plus tool blocks, 57,647
estimated tokens, and the turn's peak context was 122,510 tokens under a
229,266 ceiling. The ladder escalated twice to hold output the model had
already read. History was not the cause: the segment sealed at the next turn,
so its text never entered a later prompt.

## Proposal

Before every model call, sum the tool-result messages and, while the sum is
over a budget, elide the oldest result before the latest tool-call round.
Keep the latest round intact. An elided result becomes `tool: [tool result
elided: N chars; full output: <path>]` when it was spilled, so the model can
re-read it. Reuse `ContextElided`; gate the pass on `bounded_tools`.

## Evidence

The proposal was implemented, tested and replayed against a no-elision
control on the same 14 prompts (105–118), the same model (q2_0) and the same
server, within one hour:

| Run | Done | Wall | Model calls | Peak context |
|---|---|---|---|---|
| No elision (control) | 14/14 | 816 s | 309 | 102 k |
| Budget 24,000 | 11/14 | 1,488 s | 394 | 84 k |
| Budget 12,000 | 10/14 | 1,927 s | 455 | 83 k |

The 0.2.1 replay of the same window, from the previous evening, matches the
control (14/14, 971 s). The failures in the elided runs are turns cut at the
300 s timeout after two to four times the control's tool calls.

The mechanism works as written — it lowers peak context — but the model
responds by re-scanning the workspace. The discarded results are the ones it
was still editing against, and the extra calls cost more than the saved
context. Turn-level contexts are also not comparable across runs once one
turn diverges, so the direction of these two points, not their exact size,
is the trustworthy part.

## Consequences

- 0.2.1 keeps its behavior: per-result summary and spill, plus the overflow
  elision at the ladder cap. That is sufficient for the measured workload.
- A future attempt should not evict content the model is mid-edit against,
  and should compare against a control in the same session; the replay
  harness now records `elided_messages` per turn for that.
