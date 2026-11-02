# Keel

A thin agent kernel with interchangeable variants.

Keel is a clean-room project. It keeps the load-bearing mechanisms of an agent
runtime — turn state machine, tool protocol, context assembly, permissions,
memory, model wire — small and stable, while variants (coding, companionship,
automation) arrive as policy profiles and plugins.

**Status:** the kernel and host base are implemented and evaluated. Twenty
coding cases and five companion personas run against `deepseek-flash`; the
remaining M6 work is packaging and release. The mechanism-level evidence is in
`docs/case-study.md`.

## Why "Keel"

The keel is the structural spine of a ship: it carries the load, and the hull,
rigging and cabins are built around it. Keel is that spine for agent variants.

## Three layers

| Layer | Responsibility |
|---|---|
| `kernel` | Mechanisms and invariants: turn FSM, tool protocol, context assembly, permission enforcement, cancellation, event log, model wire |
| `middle` | Policy and composition: profile resolution, tool visibility, context recipe, thinking budget, verification mode, memory policy, model route |
| `plugins` | Capabilities: tools, channels, memory backends, UI, domain packs |

The kernel never knows variant names. The middle layer answers the kernel's
policy questions. Plugins add capability and never bypass permissions.

## Kernel vs reference host

This repository is kernel-first. The crates under `crates/` are the reusable
kernel; `hosts/keel-cli` is a thin reference host that exercises it and serves
as an executable contract test. A derived project depends on the kernel crates
and brings its own host, UI, storage and composition instead of forking the
kernel.

The kernel contract, stability tiers and update procedure are in
`docs/kernel-contract.md`; version history is in `CHANGELOG.md`. The
kernel-only build proves the boundary:

```bash
cargo build -p keel-core -p keel-policy -p keel-runtime -p keel-model-openai -p keel-model-anthropic
cargo run -p keel-runtime --example minimal_turn
cargo run -p keel-runtime --example behaviors
```

`minimal_turn` is a complete host in one file: a profile, two context stages,
one tool, a mock model and a driver. It is the smallest starting point for a
derived project; publishing the kernel is not required for personal use.

`behaviors` is the runnable verification harness: with the mock provider and no
network it checks turn continuation, the token budget, activity-based command
timeouts, the permission matrix, large-file truncation, the non-standard
program marker and the parallel-read cap, then prints `N passed, M failed`.

## Implemented

- Turn FSM, streaming OpenAI-compatible provider, tool loop, cancellation and
  turn deadlines.
- Profile-driven policy, two permission levels, soft delete and typed
  interactions.
- Bounded context: governor, segment records, rolling memo, scoped facts,
  lexical recall and tool-result elision.
- Acceptance checklists (compute, command, test, file hash, human,
  unverifiable) with checks between retries.
- SQLite host store: resume, per-turn checkpoints, branches, session deletion,
  fact/task history, context snapshots and fault-injection tests.
- Composed read models for a frontend: `--view sessions|session|turn|memory`,
  decoded answers, a typed memory timeline and turn paging.
- MCP plugins over stdio and streamable HTTP, `--plugins-check [--json]`,
  roots/ping replies and process-tree cleanup.
- Bounded tool execution: command output keeps a head and tail (64 KiB per
  stream) instead of buffering everything, `read_file` reads at most 8 MiB and
  appends a truncation marker instead of refusing a larger file, and a
  timed-out or cancelled command kills its process tree.
- One turn finishes a multi-step task. The limits (500 model calls, a 24-hour
  backstop, a 1,000-call tool budget in the coding profiles) are runaway
  protection, not a per-task allowance, and an unfinished `todo` list keeps the
  turn going instead of asking the user to type "continue". A per-turn
  input-token budget bounds cost; it is derived from the working set and the
  model never sees the number.
- Timeouts measure activity: a command that keeps printing is not cut at a
  fixed deadline, a silent one is stopped after five minutes, and a model
  stream resets its window on every chunk. Read-only calls run in parallel,
  eight at a time.
- Windows `.cmd`/`.bat` shims (`npm`, `npx`, `pnpm`, `yarn`) resolve through
  PATH, so allowlisted names start without a shell.
- Explicit undo: `list_trash` lists recoverable versions and `restore_file`
  restores the newest one, or a specific entry; the current version is backed
  up first. `delete_file` moves a file into the trash before removing it;
  `move_file` backs up an existing destination; `make_dir` creates parents.
- Symlink-safe paths: the dispatcher canonicalizes every requested path before
  the permission gate, so a link inside the workspace cannot redirect a read
  or write outside it; `..` never climbs above the root.
- Extra roots: `--root PATH[:ro|:rw]` adds read-only or read-write scopes for
  tasks that span directories; `/roots` lists them. The most specific scope
  wins, execution still asks, and drive/system roots are refused.
- Per-call usage and cache telemetry, plus a cost model in
  `docs/llmwiki/cache-economics.md`.

## Layout

- `crates/` — reusable libraries.
- `hosts/` — executables (the CLI).
- `variants/` — composition data: profiles, prompts, plugin selection.
- `examples/` — a disposable workspace for the terminal walkthrough.
- `evals/` — capability evidence.
- `docs/` — specs, ADRs, architecture and the case study.

The workspace has six members: `keel-core`, `keel-policy`,
`keel-model-openai`, `keel-model-anthropic`, `keel-runtime` and
`hosts/keel-cli`. Crates are split only when a second consumer appears; see
`docs/spec/03-modules.md`.

## Clean-room notice

This repository is an independent implementation. It must not contain code,
documents, tests, configuration, data or internal vocabulary from any previous
employer or client project. All design input comes from `docs/spec/` and public
sources recorded in `docs/spec/05-provenance.md`.

## Start here

1. `CLAUDE.md` — working rules for this repository.
2. `docs/architecture.md` — how the layers and one turn fit together.
3. `docs/spec/00-brief.md` — problem, users, non-goals.
4. `docs/spec/01-kernel.md` — kernel contract.
5. `docs/spec/02-profile.md` — policy/profile schema.
6. `docs/adr/` — accepted decisions, including storage, plugins and memory.
7. `docs/case-study.md` — measured behaviour and failure analysis.
8. `docs/spec/06-roadmap.md` — milestone order.

## Run

```bash
cargo run -p keel-cli -- --help
cargo run -p keel-cli -- --version
cargo run -p keel-cli -- --profile variants/coding/profile.toml --input "hello"
cargo run -p keel-cli -- --profile variants/companion/profile.toml --input "hello"
```

The M1 mock model makes no network calls. The coding profile exercises one
`echo` tool call; the companion profile exercises the no-tools policy.

Keep one session across turns with `--session`; it reads one input per line
until EOF and the history stage carries earlier turns into later ones:

```bash
printf 'first\nsecond\n' | cargo run -p keel-cli -- \
  --profile variants/companion/profile.toml --session
```

For DeepSeek (`deepseek-flash`, OpenAI-compatible):

```bash
cp .env.example .env
# edit .env and set DEEPSEEK_API_KEY only
cargo run -p keel-cli -- --profile variants/coding/profile.deepseek.toml --input "hello"
```

The profile carries `base_url = "https://api.deepseek.com"` and
`model = "deepseek-flash"`; only the key belongs in `.env`.

For an OpenAI endpoint:

```bash
cp .env.example .env
# edit .env and set OPENAI_API_KEY
cargo run -p keel-cli -- --profile variants/coding/profile.openai.toml --input "hello"
```

For an Anthropic endpoint:

```bash
cp .env.example .env
# edit .env and set ANTHROPIC_API_KEY
cargo run -p keel-cli -- --profile variants/coding/profile.anthropic.toml --input "hello"
```

The Anthropic profile uses the Messages API (`/v1/messages`), streaming text,
tool use and usage. Prompt-cache and extended-thinking controls are not wired
yet.

The CLI loads `.env` automatically. The environment variable wins; when it is
absent the host reads the profile's `api_key_keyring` entry from the OS
keyring. Credentials never belong in the chat or a profile.

The workspace defaults to the current directory: start the CLI in the folder
you want it to work in. `--workspace PATH` is for scripts and evals that run
from another directory while still loading `.env` and the profile here:

```bash
cargo run -p keel-cli -- \
  --workspace /tmp/keel-task-001 \
  --profile variants/coding/profile.openai.toml \
  --input "fix the mean function"
```

Keel uses two permission levels: `workspace_auto` is the permission boundary
and `full_auto` removes the workspace boundary while keeping the structural
deny list. It gates tool calls, not processes: an approved `shell` command or
an MCP server is not confined to the workspace. A credential name inside a
root still asks for confirmation, and outside every root it is refused; a
`.env.example` is an ordinary file. `shell` runs any program without a shell,
marking the ones outside its standard developer list in the confirmation. Paths
are canonicalized before the gate, so a symlink inside the workspace cannot
redirect a read or write outside it. Deletion is recoverable through the
trash; interactions (questions, plan approval, permission confirmation and
escalation) are kernel primitives, not domain tools. See ADR 0002 and ADR
0003.

A task that spans more than one directory adds labeled roots instead of a
wider workspace:

```bash
cargo run -p keel-cli -- --profile variants/coding/profile.deepseek.toml \
  --workspace D:\Downloads \
  --root E:\Archive:rw \
  --root D:\Photos:ro \
  --input "move the old installers to E:\Archive and list large photos"
```

`--root` is repeatable. `:ro` allows reads only, `:rw` (the default) allows
reads and writes; execution still asks for confirmation. Relative tool paths
stay in the workspace, other roots use absolute paths, and the most specific
root wins. A drive root or a system directory (`C:\`, `C:\Windows`,
`C:\Users`) is refused. In the TUI, `/roots` lists every scope. Use one
writable root per phase and keep the others read-only; separate sessions when
the phases should not share undo history.

Tool calls stop at 30 s and the turn at 180 s. A first build or `npm install`
may need more; raise the limits with `--tool-timeout-ms` and
`--turn-timeout-ms`:

```bash
cargo run -p keel-cli -- --profile variants/coding/profile.deepseek.toml \
  --tool-timeout-ms 300000 --turn-timeout-ms 900000 --input "run the full suite"
```

A timed-out or cancelled command is killed with the processes it started, so a
stray server does not keep a port after the turn.

## Undo

`write_file` copies the previous version into `.keel/trash` before an
overwrite. The coding profiles also expose the recovery tools, so rollback does
not mean rebuilding the file or running destructive git commands:

- `list_trash` lists the newest backups with their entry id and original path.
- `restore_file` with `path` restores the newest backup for that file; with
  `entry` it restores a specific version. The current version is backed up
  first, so a restore is itself reversible.
- `delete_file` moves a regular file into the trash (copying only when the
  trash is on another volume) before removing it, so a deletion is recoverable
  without doubling disk usage.
- `move_file` moves a file or directory on one volume; an existing destination
  file is backed up first. Moving it back is the inverse operation.
- `make_dir` creates a directory and any missing parents; it is idempotent.

Shell commands can still modify or delete files without a backup; use the
typed file tools for edits that must be undoable. The configured outputs
directory is readable so a truncated tool result can be re-read; the rest of
`.keel` stays denied.

## Terminal UI

`--tui` runs an interactive terminal host: streaming transcript, one line per
tool call, a permission modal, and slash commands. It needs a real terminal.

```bash
cargo run -p keel-cli -- --tui --profile variants/coding/profile.deepseek.toml \
  --store keel.db --session-id mem
```

- `Enter` sends; `Ctrl-C` cancels a running turn (quits when idle);
  `PgUp`/`PgDn` scroll; `Ctrl-L` clears the transcript.
- While a turn runs, `Enter` queues an interjection instead. The driver
  delivers it before the next model call; `/cancel` still cancels, and a
  message that arrives too late is reported after the turn.
- `--input TEXT` (or a job's `input`) pre-fills the prompt; it is not sent
  until you press `Enter`.
- In a permission prompt, `↑`/`↓` selects an option and `Enter` confirms
  (default: Allow once); `Esc` defers. `d`/`a`/`s`/`f` remain shortcuts.
- `Shift+Tab` switches `workspace_auto` ↔ `full_auto`; the bottom status line
  shows the current mode. `Ctrl-O` expands or hides the process behind every
  finished turn, so tool calls, their output and intermediate text stay
  available without cluttering the default view.
- A tool line keeps a bounded, redacted preview of what it returned: the head
  and tail while the turn runs, the whole preview under `Ctrl-O`. The preview
  stays in memory and is never written to the store.
- The transcript keeps every turn; `PgUp`/`PgDn` scroll it and a scrollbar
  appears when it is longer than the viewport.
- `/help`, `/clear`, `/cost`, `/sessions`, `/roots`, `/cancel`, `/quit`.
- While a turn runs, streamed text and tool lines are visible. When it
  finishes they collapse to the answer plus one aggregate line (`tools ·
  files · tokens · duration`), so the transcript keeps results, not process.
- Start with `--store` and `--session-id` to resume; the bottom status line
  shows the workspace, model and permission mode, with key hints on the right.

For a scripted demo with no model key, `--demo` (mock provider only) streams a
preamble, asks for `shell` permission, runs a two-second command that prints
`check passed`, streams the answer piece by piece, then collapses the process
into the result:

```bash
cargo run -p keel-cli -- --tui --demo --profile variants/coding/profile.toml
```

Press `a` in the permission prompt to allow the tool once.

### First real run

`examples/demo-workspace` is a three-test Python module with one failing test.
Start the UI against it, approve `shell` once, and watch the agent run the
tests, fix `stats.py` and run them again:

```bash
cargo run -p keel-cli -- --tui \
  --workspace examples/demo-workspace \
  --profile variants/coding/profile.deepseek.toml \
  --store keel.db --session-id demo
```

Enter:

> the tests in this directory fail; run them, fix the code, and run them again

1. `python -m unittest -v` asks for `shell` permission. `Enter` allows it
   once; the tool line keeps the head and tail of the failing output while the
   turn runs.
2. `read_file` and `write_file` follow; `write_file` backs up `stats.py` into
   `.keel/trash` before the overwrite.
3. The tests run again and pass; the turn collapses to the answer and one
   aggregate line (`tools · files · tokens · duration`).
4. `Ctrl-O` expands the process, including the output of both test runs;
   `Shift+Tab` switches the session to `full_auto`.
5. Restarting with `--store keel.db --session-id demo` resumes the session.

If the edit is wrong, ask for `restore_file` with `path = "stats.py"`; the
current version is backed up first, so the restore is itself reversible.

The UI is deliberately small: no markdown rendering, syntax highlighting,
mouse or model picker. It is a reference host, not a product; see
`docs/llmwiki/tasks/tui.md`.

## Bundle and state

A bundle names the profile, optional plugin file and optional state directory
in one file; every path is relative to the bundle, so it works from any
working directory:

```toml
# variants/coding/bundle.toml
name = "coding"
profile = "profile.deepseek.toml"
# state_dir = "../../.keel/state"   # optional; defaults to KEEL_HOME or ~/.keel
```

```bash
cargo run -p keel-cli -- --tui --bundle variants/coding/bundle.toml
```

`--bundle` is mutually exclusive with `--profile` and `--plugins`;
`--workspace` still selects the task directory. The state root defaults to
`KEEL_HOME`, then `~/.keel` (`%USERPROFILE%\.keel` on Windows); `--state-dir`
or a bundle's `state_dir` overrides it, and `--store` overrides only the
database. The store lives at `<state>/keel.db`, the trash at
`<state>/trash` and truncated tool output at `<state>/outputs`, so sessions
and undo history survive workspace cleanup and no `.keel` directory appears
in the task directory. If no home can be found, the host falls back to the
workspace-local layout. The outputs directory keeps the newest 200 spill
files and prunes older ones; unrelated files are left alone.

The reference bundles are `variants/coding/bundle.toml` and
`variants/companion/bundle.toml`; both use `deepseek-flash` and need
`DEEPSEEK_API_KEY`. For the no-key mock, keep using
`--profile variants/coding/profile.toml`.

## Jobs

A job binds composition, scope, state and the first input into a reusable
task. Every path is relative to the job file:

```toml
# jobs/tidy-downloads.toml
bundle = "../Keel/variants/coding/bundle.toml"
workspace = "D:/Downloads"
session_id = "tidy"
input = "Organize this folder: create Archive, move the old installers, and delete junk."

[[roots]]
path = "E:/Archive"
access = "rw"

[[roots]]
path = "D:/Photos"
access = "ro"
```

```bash
cargo run -p keel-cli -- --job jobs/tidy-downloads.toml
```

Explicit CLI flags win over the job, and job values win over the defaults:
`--workspace`, `--input`, `--session-id`, `--state-dir` and `--store`
override the job's fields, and CLI `--root` entries are added to the job's
roots. `--profile`, `--plugins` or `--bundle` on the CLI replace the job's
composition as a whole. A job may set `bundle` or `profile`/`plugins`, not
both. `input` runs in line mode; with `--tui --job` (or `--tui --input`) the
text is placed in the prompt for review and is not submitted automatically.

## Store, branches and views

`--store` keeps sessions in SQLite; a new process resumes from the snapshot.
Without `--store`, the default state root supplies `<state>/keel.db` (see
Bundle and state), so a normal run persists its session. The view, delete and
fork commands take `--state-dir PATH` instead of `--store` when using the
default.

```bash
cargo run -p keel-cli -- --profile variants/companion/profile.deepseek.toml \
  --session --store keel.db --session-id mem

# A branch starts from the state after a turn; the parent is untouched.
cargo run -p keel-cli -- --profile variants/companion/profile.deepseek.toml \
  --session --store keel.db --session-id alt --fork-from mem:3

# Delete one session and its rows; branches are independent.
cargo run -p keel-cli -- --store keel.db --delete-session alt
```

Checkpoints keep the 20 most recent turns per session by default; a checkpoint
that a branch was forked from is always kept. `--keep-checkpoints N` changes
the cap and `--keep-checkpoints 0` keeps every checkpoint.

Composed views are what a frontend renders; it does not join tables or decode
JSON columns:

```bash
cargo run -p keel-cli -- --store keel.db --view sessions
cargo run -p keel-cli -- --store keel.db --view session --session-id mem --limit 20
cargo run -p keel-cli -- --store keel.db --view turn --session-id mem --turn 3
cargo run -p keel-cli -- --store keel.db --view memory --session-id mem
cargo run -p keel-cli -- --store keel.db --view events --session-id mem --limit 5
```

## Plugins

A plugin is an external process or a remote MCP endpoint. `plugins.toml` lists
servers; the host performs the handshake and registers their tools.

```bash
cargo run -p keel-cli -- --plugins-check --json --plugins plugins.toml
cargo run -p keel-cli -- --profile variants/coding/profile.toml \
  --plugins plugins.toml --input "list the workspace"
```

`--plugins-check` is the gate for a configuration page: it starts every server,
lists its tools and exits non-zero if any fails. A changed file needs a host
restart; state is in SQLite, so restarting does not lose a session. One server
message is capped at 8 MiB; a larger stdio line or HTTP body is reported as an
error instead of being buffered. See ADR 0013.

## Evals

```bash
python evals/harness/run_case.py evals/cases/coding/task-001
python evals/harness/run_suite.py --filter coding
python evals/harness/run_companions.py
```

Model runs are local because they need a key; CI covers the deterministic
parts. Reports live in `evals/reports/`; the measured results are summarised in
`docs/case-study.md`.

## Secrets

Keys never go into the chat or the repository. The host resolves a credential
in this order:

1. The profile's `api_key_env` variable, normally loaded from `.env`
   (gitignored; copy `.env.example`).
2. The profile's `api_key_keyring` entry, in the OS keyring. Store it with the
   secret on stdin so it never appears in shell history or the process list:

```bash
# Windows Credential Manager / macOS Keychain / Linux keyutils
printf '%s' "$KEY" | cargo run -p keel-cli -- --keyring-set keel:anthropic
cargo run -p keel-cli -- --keyring-delete keel:anthropic
```

A terminal echoes typed input; pipe the secret from a password manager or
secret store instead. The Linux backend is the kernel keyring, so it is
session-scoped, not a Secret Service replacement.

Input is never blocked; when it matches a secret pattern, the turn records a
`SecretWarning` event and the CLI prints the notice with that turn's result.
Test keys and hashes trigger the same patterns, so blocking would make the CLI
unusable. The event log redacts detected secrets. If a real key was ever pasted
into a chat, rotate it.

## Verification

```bash
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo deny check

# Performance gates (release; ignored by default)
cargo test --release -p keel-runtime --test performance -- --ignored --nocapture
```

CI runs the same format, lint and test gates on Ubuntu, Windows and macOS,
plus the kernel-only build and `minimal_turn` example on each platform.
`cargo deny` runs in a separate Linux job.

## License

Apache-2.0. See `LICENSE`.
