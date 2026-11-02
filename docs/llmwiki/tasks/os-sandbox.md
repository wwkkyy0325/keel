# OS sandbox

ADR 0015 fixes the contract: the backstop covers only the implicit,
irreversible effects of a process the user already approved — writes outside
the derived roots, and reads of a fixed credential set. The setting surface is
one boolean plus `/allow <path>`.

The existing `sandbox-and-file-tools.md` covers the *logical* sandbox (the
permission gate). This task is about process confinement, which the gate
cannot see.

## Steps

- [ ] **1. Port and defaults.**
      `keel-core` gains `Sandbox`, `SandboxRequest`, `SandboxGuard`,
      `SandboxCapabilities`, `SandboxRefusal`; `NullSandbox` reports every
      layer absent. The profile gains `[sandbox] enabled` (default `true`) and
      nothing else; unknown keys stay errors. The host derives writable roots
      from the permission surface, the state root and the temp directory.
      `SandboxApplied` is emitted when a confined process starts and is
      skipped when `enabled = false`.
      Verify: `the_sandbox_is_on_by_default`,
      `the_request_contains_every_permission_root`,
      `turning_the_sandbox_off_starts_an_unconfined_process`, and a profile
      with `[sandbox] mode` is rejected.

- [ ] **2. Environment allowlist in both launch paths.**
      `keel-runtime::command` and the host's MCP spawn pass only `PATH`, the
      home variable, temp and locale. Key-shaped names are removed even when
      the sandbox is off.
      Verify: a shell call prints its environment and the test asserts
      `ANTHROPIC_API_KEY` is absent while `PATH` survives;
      `an_mcp_server_does_not_inherit_the_api_key`.

- [ ] **3. Derived tables: toolchain caches and home entries.**
      A built-in table keyed by program adds cache subdirectories that exist
      (`cargo`/`rustc`, `npm`/`npx`/`pnpm`/`yarn`, `pip`/`python`, `go`); the
      home's top-level entries are enumerated at spawn. A fixed secret set
      (`.ssh`, `.gnupg`, `.aws`, `.kube`, `.netrc`, `.npmrc`, `.pypirc`,
      gh/azure/gcloud config, `~/.cargo/credentials.toml`) is never granted.
      Verify: `cargo` gets its cache subdirectories and not
      `~/.cargo/credentials.toml`; a program outside the table gets no cache
      root; the home entry list skips every secret name;
      `the_effective_roots_are_printed`.

- [ ] **4. Linux: Landlock backend in `keel-sandbox`.**
      Build the ruleset descriptor in the parent, then
      `prctl(PR_SET_NO_NEW_PRIVS)` and `landlock_restrict_self` in
      `pre_exec`; the closure allocates nothing. Handle write-class rights
      only (`WRITE_FILE`, `REMOVE_*`, `MAKE_*`, `REFER`, `TRUNCATE`); read
      rights are handled only to remove the secret paths. Without Landlock the
      backend reports the layer absent.
      Verify: write outside the roots fails while the same command launched by
      the test without a sandbox succeeds (the control must not go through the
      permission gate, or it would pass for the wrong reason); write inside
      succeeds; reading a planted fake `~/.ssh/id_ed25519` fails while
      reading an ordinary home file succeeds; ABI < 1 skips with a printed
      reason.

- [ ] **5. macOS: seatbelt backend.**
      Generate an SBPL profile (`deny default`, allow reads of the system
      set, the roots and the home minus the secret set; allow writes on the
      roots) and launch through `sandbox-exec -p`. Quote every path; no
      interpolation from model input.
      Verify: the same four assertions as the Linux step; a missing
      `sandbox-exec` skips with a printed reason.

- [ ] **6. Windows: job object backend.**
      Assign the child to a job with kill-on-close, an active-process limit
      and an optional memory limit; refuse breakaway. `write` and `secrets`
      report absent.
      Verify: a grandchild spawned by the tool dies when the job closes; the
      process limit is enforced; the handle is released on every exit path.

- [ ] **7. Host wiring and the recovery path.**
      Startup prints the effective line. `/sandbox` prints backend, layers,
      roots and the denied secret set; `/allow <path>` adds a writable root
      for the session with the permission engine's TTL grant and the same
      audit. The first non-zero exit from a confined command prints one line
      naming the blocked path and the `/allow` command. MCP stdio servers
      launch through the same backend with a cache directory under the state
      root.
      Verify: `--plugins-check` passes with a sandboxed stdio server;
      `a_blocked_command_names_the_path_and_the_allow_command`;
      `/allow` widens the sandbox request and is audited;
      `sandbox_status_prints_the_effective_roots`.

- [ ] **8. Evidence for the default.**
      Run the 20 coding cases and the 50-chapter run with the backstop on.
      Record every denial that needed a user action and the spawn overhead
      (target < 10 ms). The target for ordinary work is zero denials; a denial
      is fixed by a derived root or a secret-set change in code, not by a
      user key.
      Verify: `evals/reports/` entry with the denial list and the overhead
      number.

- [ ] **9. Deferred spike: Windows AppContainer.**
      Needs a capability SID, ACEs on each root and process-attribute
      plumbing. It mutates the user's directory ACLs and breaks toolchains
      that read outside the granted roots; do not start it without a decision
      to accept that cost.

## Verification matrix

CI runs each backend's tests on its own OS and must not pass vacuously: every
"denied" assertion is paired with a control that the same operation succeeds
without the sandbox. A skipped test prints why it skipped.

| OS | Backend test | Control |
|---|---|---|
| Ubuntu | `crates/keel-sandbox/tests/linux.rs` | the same command without a sandbox |
| macOS | `crates/keel-sandbox/tests/macos.rs` | the same command without a sandbox |
| Windows | `crates/keel-sandbox/tests/windows.rs` (job) | the same command without a sandbox |

## Boundaries

- The network is not confined in v1; a compromised build can still upload what
  it can read, and the README says so instead of implying isolation.
- Writes inside the derived roots are not protected: `rm -rf` in the workspace
  still succeeds. Trash covers typed file tools only.
- Ordinary reads stay open, including most of the home directory.
- No administrator or root privileges; no kernel extension, driver or system
  policy change; no ACL change to the user's files.
- The sandbox does not mediate the model API call; a secret that reaches the
  conversation through an allowed read is outside its scope.
- A remote MCP server is not sandboxable; the plugin file says so.
