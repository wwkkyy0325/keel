# Hardening suite — 2026-10-31

## Setup

- Cases: 10 self-authored coding tasks.
- Profile: `variants/coding/profile.eval.toml`.
- Model: `deepseek-flash`.
- Harness: `evals/harness/run_suite.py`.
- Each case copies `setup/`, runs the CLI with its acceptance checklist, and
  then runs `verify.sh` against the workspace.

## Result

10/10 passed. The run used 152,807 tokens over 103 seconds (about 15,281
tokens and 10.3 seconds per case).

Raw summary: `evals/reports/runs/summary-20261102T165646Z.json`.

## What the suite covers

- Single-file bug fixes: pagination, ranking, de-duplication, retry policy.
- Data handling: JSON merge, safe file storage, JSON Lines totals.
- Multi-file change: `app/format.py` + `app/main.py`.
- CLI argument validation.
- Large workspace surface for search and context pressure (`task-010`).
- Kernel acceptance commands and external hidden tests for every case.

## Findings

- A budget-exhausted turn could report `Done` with a failed verification.
  Fixed: a configured acceptance plan requires a passing report, and missing
  verification ends in `verification_failed`.
- Non-terminal shell denial caused repeated loops; the eval profile now hides
  `shell`, while the kernel verifier still executes the acceptance command.
- One task prompt was ambiguous; the prompt was clarified rather than treating
  the failure as a model defect.
- One acceptance command had a file-flush bug; fixed.
- One provider stream hung until the turn deadline; provider timeout and retry
  are still open.

## Not covered

- Public datasets (HumanEval+/MBPP+), fault injection and real dogfooding.
- Multi-turn context, compaction and memory.
