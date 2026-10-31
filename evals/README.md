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
