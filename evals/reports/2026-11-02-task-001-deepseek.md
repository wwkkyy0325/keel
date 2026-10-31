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

## Agent behavior

- The model used workspace tools to inspect and write the file.
- Shell attempts to run Python were denied because stdin was not a terminal,
  so the terminal interaction port deferred the permission request.
- The model reported that it could not execute the tests and marked the fix as
  unverified. It did not claim a test run that did not happen.
- The external `verify.sh` run is the authoritative evidence for this case.

## Wire note

`deepseek-flash` defaults to thinking mode. In a tool loop the provider expects
`reasoning_content` to be echoed back; the adapter does not do that yet. The
profile sets `reasoning_effort = "none"` so the task can run in non-thinking
mode.

## Not measured

- Token usage and cost were not captured by the CLI.
- Driver-side acceptance verification is not wired yet; this run used the
  external `verify.sh`.
