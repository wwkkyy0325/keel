# ADR 0015 — OS confinement is a backstop for implicit effects

## Status

Deferred. The design below is kept for when a real need appears; the plan is
`docs/llmwiki/tasks/os-sandbox.md` and the first three steps are cheap, but
the kernel work that shipped instead (activity timeouts, background commands,
sub-agents) addressed the immediate task-completion problems. Nothing here is
implemented.

## Context

The permission gate decides *whether* a call may run. It does not see what the
process does after it starts. `shell` and MCP servers run with the user's full
privileges: a build script, a postinstall hook or a test in an untrusted
repository can delete files, rewrite configuration or read credentials after
the user approved `cargo test`.

An earlier draft of this ADR confined the process to the workspace and offered
a global off switch. That is the wrong shape. It treats every write outside
the project as a violation, so a legitimate task — "update my notes in
`~/notes`", a toolchain writing its cache, `git push` reading `~/.ssh` — stops
until the user disables the sandbox and loses the protection with it. That is
a net loss: the tool becomes harder to use and the dangerous case is still
unprotected because the user turned the whole thing off.

The backstop should only stop what is both **implicit** (the user did not ask
for it, so the gate never saw it) and **irreversible** (a deletion, an
overwrite, a credential leaving the machine). Everything else stays open,
because blocking it fails legitimate work without preventing damage.

## Decision

### 1. Two things, and only two

| Layer | What it stops | Why it is here |
|---|---|---|
| `write` | Creating, modifying, deleting or renaming anything outside the derived writable roots | Data loss and tampering are what cannot be undone |
| `secrets` | Reading a fixed set of credential locations (`~/.ssh`, `~/.gnupg`, `~/.aws`, `~/.kube`, `~/.netrc`, `~/.cargo/credentials.toml`, the gh/azure/gcloud config trees, …) | A process that prints a key to stdout sends it to the model; that is irreversible too |

Everything else is deliberately outside the backstop:

- ordinary reads, including the rest of the home directory;
- the network;
- writes inside the derived roots, which include the toolchain caches that
  build tools legitimately use;
- any path the user granted explicitly. A gate decision and a sandbox root
  cannot disagree: the roots are derived from the permission surface, and
  `/allow` adds to both.

### 2. One switch, one recovery

The profile surface is one boolean:

```toml
[sandbox]
enabled = false   # default: true
```

The in-session recovery is one command that names the thing that was blocked:

```
/sandbox                     # backend, layers, writable roots, denied secret set
/allow C:\work\notes         # add a writable root for this session (TTL), audited
```

`/allow` is the existing path grant with a TTL, not a second mechanism. The
failure path uses it directly: when a confined process exits non-zero, the
tool result carries the OS error with the path, and the host prints one line
in the same place the tool output appears:

```
cargo was blocked writing /home/u/.gradle/caches (sandbox).
/allow /home/u/.gradle/caches to permit it for this session.
```

No modal, no restart, no config file. A user who cannot be bothered to read
the docs still sees the blocked path and the one command that fixes it.

### 3. Defaults are derived, never configured

Writable:

- the workspace and each extra root configured read-write;
- the state root (store, trash, spills);
- the system temp directory;
- toolchain caches, resolved from a built-in table keyed by the launched
  program: `cargo`/`rustc` → `~/.cargo/registry`, `~/.cargo/git`,
  `~/.cargo/bin`, `~/.rustup`; `npm`/`npx`/`pnpm`/`yarn` → `~/.npm`,
  `~/.cache`, the platform npm cache; `pip`/`python` → the pip cache;
  `go` → `~/go/pkg`, the build cache. Only existing paths are added.
  Credential files that sit next to a cache (`~/.cargo/credentials.toml`,
  `~/.npmrc`) are not covered by these subdirectory entries.
- the home directory's ordinary entries, enumerated at spawn and added
  read-write except the secret names. That is what keeps "the agent writes my
  notes" working; the exclusion is by name, and the set is fixed in code.

Readable: the roots above, plus the per-platform system set (`/usr`, `/lib*`,
`/bin`, `/etc`, `C:\Windows`, `C:\Program Files`, …), plus everything under
home except the secret set.

Environment: `PATH`, the home variable, temp, locale. Key-shaped names are
removed unconditionally, with or without a sandbox.

Every list is data in the host. `/sandbox` prints what is in force, so a
widening is inspectable rather than silent.

### 4. When the backstop is wrong

A false denial is a bug, not a user setting. The fix order:

1. add the missing path to the derived table in code, or remove a name from
   the secret set, with a test;
2. `/allow` for the session when the need is specific to this user;
3. `enabled = false` only when a platform cannot provide the layer at all.

The evidence step (task step 8) measures exactly this: how many times ordinary
work needs a user action. The target is zero. A backstop the user has to
manage has already failed.

### 5. Capability negotiation is an API, not a setting

A backend reports the layers it enforces. A derived host that needs a hard
guarantee calls `SandboxRefusal` in code; the reference host never refuses. It
launches and prints one line at startup when a layer is missing:

```
sandbox: job object (process tree, environment; filesystem unavailable on Windows)
```

The user is told what is protected; they are not asked to configure it.

### 6. The kernel owns the port, not a platform

`keel-core` gains `Sandbox`, `SandboxRequest`, `SandboxGuard`,
`SandboxCapabilities` and `SandboxRefusal`. The trait has two hooks:

- `prepare(&SandboxRequest, &mut std::process::Command)` — on macOS it
  rewrites the program into `sandbox-exec`; on Linux it attaches a `pre_exec`
  closure that only calls `restrict_self` on a pre-built ruleset descriptor.
- `spawned(&mut SandboxGuard, pid)` — Windows assigns the new process to a job
  object. `std` cannot create a suspended child and resume it, so the
  assignment happens immediately after `CreateProcess`. The guarantee is
  lifetime and fan-out, not "no instruction ran before the assignment"; that
  is documented, not hidden.

A new crate `keel-sandbox` implements the backends and depends only on
`keel-core`. It has two consumers (the shell tool and MCP launch), which is
the bar `03-modules.md` sets for a new crate. The kernel crates keep no
platform syscall code.

### 7. Backends

| Platform | `process_tree` | `write` | `secrets` | Mechanism |
|---|---|---|---|---|
| Linux | yes | yes (Landlock ABI ≥ 1) | yes (no rule for the secret paths) | Ruleset built in the parent; `PR_SET_NO_NEW_PRIVS` and `landlock_restrict_self` in `pre_exec`; process group |
| macOS | yes | yes | yes | Generated SBPL profile passed to `sandbox-exec -p` |
| Windows | yes | **no** | **no** | Job object: kill on close, active-process and memory limits |

Both Landlock and `sandbox-exec` require no ACL change to the user's files.
Windows AppContainer is the only unprivileged mechanism that can restrict the
filesystem, but it needs a capability SID and explicit ACEs on every root,
which mutates the user's directories and breaks toolchains that read outside
the granted roots. It stays out until someone accepts that cost; Windows
reports `write = false` and the startup line says so.

`sandbox-exec` is deprecated but still shipped. It is isolated in one backend;
if a future macOS removes it, the backend reports the layer absent and the
startup line changes. It never silently degrades.

Landlock has no deny rule, so the secret set is expressed by not granting a
rule: the home's top-level entries are enumerated and every name in the fixed
secret set is skipped, and toolchain caches are granted as subdirectories so a
credential file at the cache root stays unreadable. The enumeration is a
single read of one directory per spawn.

### 8. Network stays open in v1

Blocking it breaks `cargo` and `npm` on the first run, and a network setting is
the knob this ADR removes. Linux also cannot express it uniformly: Landlock
restricts TCP only from ABI 4 and says nothing about UDP or DNS. v1 therefore
leaves the network open and says so in the startup line and the README. The
port keeps a network layer for a host that later supplies a backend with a
whole-network guarantee.

This is a real gap: a compromised build can still upload what it can read.
The `secrets` layer exists precisely because the model channel makes a printed
key irreversible; without a network control the two layers only reduce the
blast radius, and the README must say that rather than implying full
isolation.

### 9. MCP servers and audit

A stdio MCP server launches through the same `Sandbox`, with roots equal to
the session scopes plus its own cache directory under the state root. A remote
HTTP server is outside the backstop; the plugin file marks it as such.

`SandboxApplied` records backend, layers, roots and tool when a confined
process starts. `/allow` emits the same grant audit a permission confirmation
does. There is no per-syscall logging; the OS denial surfaces in the tool
result.

## Consequences

- The profile gains one boolean. Everything else is code and defaults the
  user never sees.
- The first blocked build prints the path and the one command that fixes it;
  the user does not have to know what Landlock is.
- Writes inside the derived roots are not protected: `rm -rf` in the
  workspace still succeeds. Trash covers typed file tools, not shell commands.
- Toolchain caches are writable because the tools need them; a malicious build
  script can also write there. That is the price of not breaking builds, and
  `/sandbox` prints it.
- Windows has process containment and environment hygiene only. The README and
  the startup line say so.
- Sandbox setup is a spawn-time cost (target: under 10 ms per process,
  measured); the turn hot path is untouched.

## Alternatives rejected

- **Confine to the workspace and offer a global off switch.** Every legitimate
  out-of-project write fails, and the fix removes the protection entirely.
  This is the "sandbox for its own sake" failure.
- **Confine all reads.** It breaks compilers, `git` and anything that scans the
  home directory, and without a network control it does not stop exfiltration
  anyway. Only the credential set is worth denying.
- **A permission prompt that offers "run unconfined".** It teaches the user to
  pick the bypass on the first failure; `/allow` names the path instead.
- **A profile table with mode, network, required layers and path lists.**
  Seven keys, all of them maintainer concerns, none of them decidable by a
  user who does not know the platform mechanisms.
- **Container or VM as the primary mechanism.** Requires Docker/WSL2 or
  equivalent, differs per platform, and adds seconds to a tool call. It
  remains a valid host-supplied backend behind the same port.
- **Platform code in `keel-runtime`.** A kernel crate would gain `cfg(windows)`
  unsafe blocks and a dependency on the `windows` crate.
