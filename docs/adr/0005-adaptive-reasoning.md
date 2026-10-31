# ADR 0005 — Adaptive reasoning effort

## Status

Accepted.

## Context

A fixed reasoning effort is wrong in both directions: high effort burns tokens
and latency on simple turns, low effort fails hard turns. Letting the user pick
an effort does not solve this because users cannot predict the difficulty of a
turn. Letting the model pick is also unreliable and easy to manipulate.

Some providers expose thinking as an on/off switch. If the model owns that
switch, an off state can persist after the task becomes hard, so the model
never turns thinking back on. The kernel must own the decision instead.

Providers also differ: DeepSeek exposes `none`, `low`, `high` and `max` and
requires `reasoning_content` to be echoed back when thinking mode is used with
tool calls. An adapter cannot treat reasoning as a local UI setting.

## Decision

1. Reasoning effort is a kernel-owned, per-turn monotonic ladder:
   `Low -> High -> Max`. A turn starts at `Low`, never at `Off`.
2. The model cannot request or change the level. There is no reasoning tool and
   no user-facing on/off switch in the kernel.
3. Escalate one level when a real failure signal appears:
   - a required acceptance item returns `Failed`;
   - the same tool with the same arguments fails twice with invalid
     arguments;
   - two consecutive tool calls fail because the model supplied invalid
     arguments;
   - a round produces no new information (no tool call, no new content, or the
     same action repeats);
   - `before_commit` returns `Reprompt` and the retry still fails.
4. The level never decreases within a turn; it resets to `Low` on the next
   turn.
5. Context overflow and compaction are **not** reasoning signals. They are
   capacity events handled by compaction, a larger window or a subagent. Higher
   effort can increase context use and make the problem worse.
6. Environmental tool outcomes — permission denied, not found, timeout,
   cancellation and I/O — are not reasoning failures. A model probing for a
   file or waiting on an approval will not think its way past the environment;
   escalating only adds cost.
7. At `Max`, a still-failing turn enters `Escalating` or ends in `Error`; it
   does not retry forever.
8. Providers map kernel levels to their own controls and clamp unsupported
   levels down. When thinking is used, reasoning text is stored on the
   assistant message and echoed back with tool calls when the provider
   requires it.
9. Hosts may later set a cost ceiling, but `Off` is not a default and is not a
   user-facing switch. A host that caps reasoning logs the clamp.

## Consequences

- The kernel needs real verification signals to escalate. Wiring the acceptance
  verifier into the `Verify` phase is a prerequisite for useful escalation.
- DeepSeek thinking mode requires reasoning content persistence; the adapter
  strips nothing and the driver attaches it to assistant tool-call messages.
- Simple turns run at low cost; hard turns pay more only after observed failure.
- The first version has no `Off` switch. A host-level ceiling can be added when
  a privacy or cost requirement appears.

## Alternatives rejected

- Fixed profile effort: cannot adapt, and the current DeepSeek profile needed a
  tactical `none` value only to avoid the thinking-mode wire error.
- User-selected effort: shifts an unpredictable decision to the user.
- Model-selected effort: unreliable and manipulable.
- Starting at `Off`: without a verifier, a plausible wrong answer produces no
  signal, so the turn may never escalate.
