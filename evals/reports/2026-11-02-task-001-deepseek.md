# task-001 — real model run

- Date: 2026-11-02
- Profile: `variants/coding/profile.deepseek.toml`
- Provider: DeepSeek, OpenAI-compatible
- Model: `deepseek-flash`
- Workspace: a fresh copy of `evals/cases/coding/task-001/setup/`
- Verifier: `evals/cases/coding/task-001/verify.sh`

## Result

PASS. The hidden unittest suite ran four tests and all passed.

The model changed `calc.py` to return `0.0` for an empty list and keep the
mean for non-empty lists.

The second run captured usage and latency:

- input tokens: 6952
- output tokens: 485
- total tokens: 7437
- wall clock: 5916 ms
- events: 45

A later run also passed kernel-side acceptance:
`acceptance.toml` ran `python -c` in the workspace with exit code 0, and the
turn printed `verification: passed`.

## Agent behavior

- The model used workspace tools to inspect and write the file.
- Shell attempts to run Python were denied because stdin was not a terminal,
  so the terminal interaction port deferred the permission request.
- The model reported that it could not execute the tests and marked the fix as
  unverified. It did not claim a test run that did not happen.
- The external `verify.sh` run is the authoritative evidence for this case.

## Wire note

`deepseek-flash` defaults to thinking mode. In a tool loop the provider expects
`reasoning_content` to be echoed back. The driver now stores it on assistant
tool-call messages and the wire echoes it, including an empty string when the
model produced no reasoning. The profile declares `supports_reasoning = true`
and the kernel starts at `Low`.

## Adaptive reasoning runs

- First adaptive run: started at `Low`, escalated to `High` after tool calls
  failed. It passed 4/4 with 7170 tokens and 6930 ms. The trigger was too broad:
  permission denials and ordinary file probes are environmental, not reasoning
  failures.
- Refined run: only invalid tool arguments and no-progress rounds escalate.
  Permission denied, not found, timeout, cancellation and I/O do not. The turn
  stayed at `Low`, passed 4/4, and used 15361 tokens in 10702 ms.
- Both runs reported honestly that shell execution was unavailable.

## Not measured

- Token usage and cost were not captured by the CLI.
- Driver-side acceptance verification is not wired yet; this run used the
  external `verify.sh`.
