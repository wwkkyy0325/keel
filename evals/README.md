# Evals

Cases are self-authored. Each coding case has:

```
task-001/
  task.md          issue-style prompt, no solution
  setup/           initial workspace
  hidden/          tests applied after the run
  reference/       expected fix, used to validate the case
  verify.sh        accepts or rejects a workspace
  meta.toml        timeout, profile, model and seeds
```

## Validate a case

The case must reject the initial workspace and accept the reference fix:

```bash
tmp=$(mktemp -d)
cp evals/cases/coding/task-001/setup/* "$tmp/"
bash evals/cases/coding/task-001/verify.sh "$tmp"   # non-zero

tmp=$(mktemp -d)
cp evals/cases/coding/task-001/setup/* "$tmp/"
cp evals/cases/coding/task-001/reference/* "$tmp/"
bash evals/cases/coding/task-001/verify.sh "$tmp"   # zero
```

## Real model run

Copy `setup/` into an empty workspace, start the CLI with the profile named in
`meta.toml`, and give it `task.md`. Then run `verify.sh` against that
workspace. A real run requires a local `.env` with the provider key; keys do
not belong in the chat or the repository.

```bash
workspace=$(mktemp -d)
cp evals/cases/coding/task-001/setup/* "$workspace/"
cargo run -p keel-cli -- \
  --workspace "$workspace" \
  --profile variants/coding/profile.deepseek.toml \
  --acceptance evals/cases/coding/task-001/acceptance.toml \
  --input "$(cat evals/cases/coding/task-001/task.md)"
bash evals/cases/coding/task-001/verify.sh "$workspace"
```

## Mechanism checks

A case can declare what the run must have exercised. `run_case.py` reads the
persisted event stream (`--view events`) and the per-call tool counts, then
fails the case when a rule is not met:

```toml
[mechanisms]
min_tool_calls = 20
min_model_calls = 5
max_permission_deny = 0
forbid_budget_exhaustion = true
```

The report records `mechanisms` next to the verify result: model calls, tool
calls, continuations, wrap-ups, final calls, permission decisions per kind,
the largest tool batch in one round and the number of multi-tool rounds. A
passing case therefore also says the tool loop stayed active, parallelism
happened and the turn did not burn its cost budget.

`long-refactor` is the long-horizon case: twenty service modules at the
workspace root, all raised from `LIMIT = 10` to `LIMIT = 25` in one turn,
checked by two hidden tests.

`subagent-delegation` proves the delegation path: the parent must call
`spawn_agent`, the child writes `alpha.txt`, and the verifier checks the
artifact and rejects the setup. The run also reports the parent's tool calls
and the delegated tokens.

## Long-horizon cases

Two cases exercise a long run rather than a single fix:

- `coding/long-refactor` — twenty service modules updated in one turn.
- `novel/novel-6` — a six-chapter Chinese novella generated in one turn with
  the writer profile (`variants/coding/profile.deepseek.writer.toml`), checked
  for chapter count, length, outline and character sheet. The working set is
  24k: six chapters of tool arguments plus the transcript do not fit in 16k.
- `coding/consumer-app` — a working `keel-notes` consumer crate built inside
  an empty workspace, with path dependencies on the three kernel crates. This
  case needs the unattended profile
  (`variants/coding/profile.deepseek.consumer.toml`, `full_auto`): it reads the
  kernel sources outside the workspace and runs `cargo`, and both would be
  confirmations elsewhere.

A case that cannot finish inside its working set is a finding, not a flake:
the report records the overflow and the chapters that were written.
