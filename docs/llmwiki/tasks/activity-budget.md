# Activity-based timeouts and parallel dispatch

A wall-clock limit cuts a download, a build or a slow model at an arbitrary
moment. The turn should be stopped when it is stuck, not when it is slow. This
task replaces the clock where it was doing the work of a liveness check, and
bounds parallelism instead of starting every read at once.

## Scenarios

| # | Scenario | Expected |
|---|---|---|
| 1 | `cargo build` runs six minutes, printing progress | not killed; the five-minute window only counts silence |
| 2 | a command hangs with no output | killed after the inactivity window |
| 3 | a command streams output for two hours | killed by the hard cap, not the inactivity window |
| 4 | a model streams a long answer for eight minutes | not cut; the window resets on every chunk |
| 5 | a model produces nothing for five minutes | the call fails with a timeout |
| 6 | a round asks for twenty file reads | at most eight run at once; the rest start as permits free |
| 7 | a round edits files | mutations stay serialized, in order |

## Steps

- [x] **1. Tool timeout is an inactivity window.**
      `command::run` stamps the last byte read from either pipe and stops the
      process after `spec.timeout` of silence; a two-hour hard cap bounds a
      command that never stops printing. Verify:
      `a_command_that_keeps_printing_outlives_the_inactivity_window`,
      `a_timeout_returns_without_waiting_for_the_command`.

- [x] **2. Model-call timeout is an inactivity window.**
      The stream loop resets its deadline on every chunk, so a long answer is
      not cut; a silent stream still fails. Verify: the existing stream tests
      and `the_last_model_call_is_reserved_for_the_answer`.

- [x] **3. The turn deadline is a backstop.**
      The 24-hour deadline stays; the 200-call cap bounds the turn in
      practice. Verify: `the_default_limits_fit_a_long_task`.

- [x] **4. Parallel reads are capped.**
      A semaphore of eight bounds the concurrent read batch; writes remain
      serialized. Verify: `read_tools_run_concurrently` (existing) and the
      dispatch tests, plus `a_large_read_round_is_bounded_and_complete`.

- [x] **5. The cost budget is derived and invisible.**
      `TurnLimits::max_turn_input_tokens` is the working set times one
      hundred, with a one-million floor; the profile does not gain a key.
      Reaching it records `TurnBudgetExhausted` and forces the answer call
      through the existing final-call path. No prompt, event payload or tool
      result contains the number, so the model cannot count against it.
      Verify: `the_turn_budget_forces_the_final_call_without_telling_the_model`
      (the final request carries no tools and no message mentions tokens) and
      `the_default_limits_fit_a_long_task`.

## Next: background commands

The model still waits for every tool it starts. A long task should be
startable in the background, with its result delivered at a safe point, so the
conversation can keep moving. That is a kernel-visible contract change; the
design is `docs/adr/0016-background-commands.md`.

## Boundaries

- The inactivity window is a liveness signal, not a performance target: a slow
  command that prints once a minute is not stopped.
- The hard cap is two hours per command, not per turn.
- The turn backstop is 24 hours; a host can lower it with
  `--turn-timeout-ms`, and the call budget usually ends the turn first.
- Parallelism applies to read-only tools only. A mutating tool keeps the
  round's order and the workspace's consistency.
