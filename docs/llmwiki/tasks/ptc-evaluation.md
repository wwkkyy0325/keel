# PTC evaluation

Question: should the kernel gain a code-execution path where the model writes
code that calls tools, so intermediate tool results never enter the context?

Status: closed, 2026-11-07. Decision: no. Report:
`evals/reports/2026-11-07-tool-result-share.md`.

## Steps

- [x] Keep the eval state home so the event stream survives a run.
      Verify: `run_case.py --keep` leaves `<state_home>/keel.db`; the path is
      in the report.
- [x] Analyze per-call input growth.
      Verify: `evals/harness/tool_share.py <state>` prints calls, tools,
      injected tokens, share and the longest read chain.
- [x] Measure three cases spanning the range.
      Verify: task-001 2.2%, task-010-report 9.7%, long-refactor 5.0% of input.
- [x] Apply the pre-committed rule: tool results at least 50% of input and at
      least 8 model calls per turn.
      Verify: the first condition fails by 5-20x; the second holds.
- [x] Record the decision and the reproduction command.

## What would reopen it

A workload where one task reads a large part of a large repository. Measure it
with `tool_share.py` before writing any sandbox code.

## Follow-ups this produced

- `long-refactor` passes verification but fails `tool_calls 13 < 20`: the model
  bulk-edits with `shell` instead of twenty `write_file` calls. The mechanism
  threshold assumes a method, not an outcome. Decide whether to lower it or to
  drop `shell` from the profile for that case.
