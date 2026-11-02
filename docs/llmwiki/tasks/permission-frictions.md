# Permission frictions

The review against the backstop principle (ADR 0015) found three places where
the gate blocks explicit work with no way to approve it, or hides the fact
that it blocked something.

The rule they violated: a permission boundary exists for implicit effects the
user did not ask for. An explicit request is already the user's decision, so
the gate must be able to ask instead of refusing.

## Scenarios

| # | Scenario | Expected |
|---|---|---|
| 1 | "add these variables to `.env.example`" | allowed; a template file is not a credential |
| 2 | the agent reads `.env` in the workspace without being asked | one confirmation that names the secret pattern; deny stops the call, allow lets it through |
| 3 | a tool requests `.ssh/id_rsa` outside every root | refused, no prompt, including under full auto |
| 4 | the agent reads `.git/config` inside the workspace under full auto | confirmation, not silence |
| 5 | `shell` runs `docker ps`, a program outside the standard list | one confirmation that says it is not standard; allow runs it |
| 6 | `shell` runs `cargo test` | unchanged: one execution confirmation, no "not standard" marker |
| 7 | a tool call is denied during a turn | the collapsed turn line reports `1 blocked` |
| 8 | any tool touches `.keel/trash` | refused, even in full auto and with a session grant |

## Steps

- [x] **1. Split the deny list into structural and secret.**
      `.keel` and OS areas stay an unconditional deny. Credential names
      inside a declared root return `Confirm` with the reason; outside every
      root they stay `Deny`. A session path grant may open a secret inside a
      root; it may not open one outside, and it never bypasses the structural
      list.
      Verify: `a_template_file_is_not_a_secret`,
      `reading_a_secret_inside_the_workspace_asks`,
      `a_secret_outside_every_root_is_refused`,
      `full_auto_still_confirms_sensitive_paths`,
      `the_rest_of_keel_stays_denied`,
      `structural_denials_survive_full_auto_grants`, and the dispatcher test
      `a_secret_inside_the_workspace_needs_approval_before_the_tool_runs`.

- [x] **2. `.env` templates are ordinary files.**
      `.env.example`, `.env.sample`, `.env.template`, `.env.dist` and
      `.env.defaults` are excluded from the name pattern for read and write.
      Verify: scenario 1 as a unit test.

- [x] **3. A shell program outside the standard list asks instead of failing.**
      The tool no longer returns `invalid_arguments` before the gate; its
      permission request carries the marker "not in the standard program
      list". Programs on the list keep their current prompt.
      Verify: `an_unknown_program_reaches_the_gate`,
      `a_standard_program_has_no_marker`.

- [x] **4. The collapsed turn line reports denials.**
      Count `PermissionDecided` with `decision = "deny"` for the running turn
      and add `N blocked` to the summary fields.
      Verify: `a_blocked_tool_is_counted_in_the_summary`.

- [x] **5. A large file is truncated, not refused.**
      `read_file` reads at most 8 MiB, keeps the valid UTF-8 prefix and
      appends a marker with the real size, instead of failing before the
      read. Verify: `read_file_truncates_a_file_over_the_limit`.

## Deferred, with the reason

| Item | Why it stays |
|---|---|
| `context_window` floor (8192 without tools, 16384 with) | A deterministic profile check, not a blocked task: the governor's stages and tool headroom need the room, and a smaller window would overflow by construction. Lowering it is a governor contract change with its own evidence, not a friction fix |
| MCP nested arguments as one JSON string | The flat parameter model is deliberate (ADR 0013). Typed nested parameters are a kernel feature, not a permission friction |
| MCP `sampling` refused | A server that spends tokens and leaves the audit path is a deliberate refusal; the plugin file is the place to document the limitation |
| `/var`, `/home` refused as extra roots | The guardrail asks for the smallest directory a job needs, and it is documented as a guardrail, not a boundary. A deployment directory under `/var` is refused today; refining the rule needs a real case and its own test, not a permission change |
| Checkpoints pruned to the newest 20 | It is the agent's own state, not user data, and `--keep-checkpoints 0` keeps every turn |

## Boundaries

- The allowlist no longer blocks; it labels. `full_auto` still auto-approves
  every execution, which is what full auto has always meant. The hard
  structural list and the outside-root secret rule still apply there.
- A secret inside a root is still never read silently: the prompt names the
  pattern, and the refusal path is a normal tool error.
- The sandbox (ADR 0015) does not reuse this list verbatim: a process cannot
  answer a prompt, so it denies credential directories under the home, not
  credential *names* inside the project.
