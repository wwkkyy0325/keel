# Hardening before M3

Goal: replace "the tests pass" with evidence from repeated real runs, and make
failures observable before memory and compaction change the system.

## Steps

- [x] 1. Readiness assessment and risk register — verify: docs review
- [x] 2. Eval harness: run one case, parse usage/latency and external verify,
      write a JSON report — verify: run `task-001`
- [x] 3. Self-authored agentic cases: multi-file, permission, verification
      failure, context pressure and tool errors — verify: suite report
      (10/10 passed on 2026-11-02)
- [x] 4. Public permissive cases (HumanEval+/MBPP+) as code-correctness smoke
      tests, downloaded locally and never vendored — verify: importer and
      provenance (local-file path tested; HumanEval+ 20/20 and MBPP+ 19/20
      through the local proxy; 20 + 20 is the regression baseline. A 50 + 50
      expansion is deferred for cost, and the suite stops at 300k tokens by
      default.)
- [x] 5. Fault injection: provider 429/500 retry path, stream cut, model-call
      bound, tool repeat, verification failure, context overflow — verify:
      provider and driver tests
- [ ] 6. Dogfood three to five real tasks and record friction — verify: notes in
      this file

## Findings from the first suite

- A turn could reach `Done` after a failed verification when the budget ended
  first. Fixed: a configured acceptance plan requires a passing report, and a
  verification that never ran ends in `verification_failed`.
- Non-terminal shell denial caused repeated tool loops. Added
  `profile.eval.toml` without the shell tool; the kernel verifier still runs
  the command.
- `task-008` was ambiguous about the expected greeting format; clarified the
  prompt instead of blaming the model.
- The `task-010` acceptance command did not flush its temp file; fixed.
- One model stream hung until the 180 s turn deadline. Fixed at two levels:
  transient HTTP/connect/timeout failures are retried before streaming, and
  each model call is bounded to 120 s.
- The Hugging Face datasets server was unreachable here, so the HumanEval+
  importer was validated with a local fixture. The `--download` path is
  implemented but not exercised in this environment.

## Constraints

- Cases live in `evals/cases/`; downloaded datasets live in a gitignored cache.
- Every external dataset is recorded in `docs/spec/05-provenance.md`.
- Never copy benchmark test code into the kernel or a case's setup.
- The harness reports what happened; it does not score the model's narrative.
