# Bound a turn's tool results

Goal: a tool-heavy turn must not grow the live conversation with every result
it has already read. The per-result bound (head/tail + spill) exists; the
aggregate was unbounded, and the 0.2.1 replay showed one turn carrying 57.6k
tokens of tool output into a 122.5k context.

Outcome: rejected and reverted. Both budgets lowered peak context and both
cost more than they saved: the model re-scanned the workspace it had already
read. 0.2.1 behavior is kept. The numbers are in ADR 0039.

1. [x] Add a turn tool budget and an elision pass before each model call.
   Verify: tests for a long loop, the latest round and a short turn.
2. [x] Keep the recovery path in the marker (spill path, or call again).
   Verify: an elided message keeps `full output:`.
3. [x] Emit `ContextElided` for the pass.
   Verify: the test counts the event.
4. [x] ADR 0039, workspace version 0.2.2.
   Verify: workspace tests, fmt, clippy.
5. [x] Keelson pin 0.2.2 and an `el=` column in the replay harness.
   Verify: doctor prints 0.2.2; Keelson tests pass.
6. [x] Validation replay: 105–118, q2_0, against a no-elision control.
   Verify: no-elision 14/14 Done in 816 s; 24k 11/14 in 1488 s; 12k 10/14 in
   1927 s. The budget was reverted, the version stayed 0.2.1.
