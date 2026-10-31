# Eval harness

Runs one or more cases against the real CLI and records what happened.

## One case

```bash
python evals/harness/run_case.py evals/cases/coding/task-001
```

The harness:

1. copies `setup/` into a temporary workspace;
2. runs `keel-cli` with the case profile and optional `--acceptance` file;
3. runs `verify.sh` against the workspace;
4. writes a JSON report under `evals/reports/runs/`.

A case passes only when the CLI exits zero, the turn reaches `Done`, and
`verify.sh` exits zero. The JSON also records usage, duration, reasoning level
and the kernel verification line.

## Suite

```bash
python evals/harness/run_suite.py
python evals/harness/run_suite.py --filter task-001
python evals/harness/run_suite.py --profile variants/coding/profile.toml
```

`--profile` is useful for a network-free mock smoke run; those cases will not
solve, so use it only to exercise the harness.

## Datasets

Downloaded public datasets belong in a gitignored cache, not in `setup/`.
Record every dataset in `docs/spec/05-provenance.md` before using it.

Public dataset importer:

```bash
# HumanEval+ from a local JSONL/JSON download
python evals/harness/import_public.py --dataset humaneval \
  --input /path/to/HumanEvalPlus.jsonl --limit 20

# MBPP+ from a local JSONL/JSON download
python evals/harness/import_public.py --dataset mbpp \
  --input /path/to/MbppPlus.jsonl --limit 20

# Or via the Hugging Face datasets server, when reachable
python evals/harness/import_public.py --dataset humaneval --download --limit 20
```

Generated cases go to `evals/cases/public/`, which is gitignored. The importer
was validated with both fixtures:
`evals/harness/fixtures/humaneval_sample.jsonl` and
`evals/harness/fixtures/mbpp_sample.jsonl`. In both, the setup fails and the
reference passes.
