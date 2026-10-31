# Backlog

Items raised during design that are intentionally deferred. Each item states
the trigger that makes it worth doing. "Later" means "not on the M2 critical
path", not "never".

## Context

- **Cross-turn history stage.** Scheduled for M3; design fixed in ADR 0006 and
  steps in `tasks/m3-history.md`. `BuildCtx.history` stays empty until step 1.
- **Memory stage (facts/episodic).** Session and global fact scopes are part of
  ADR 0006; extraction and promotion land in step 8.
- **Compaction ladder.** Replaced by the level ladder in ADR 0006 (`L0` to
  `L4`), triggered on segment seal and budget pressure. `L2` and `L3` stay
  model-free until the summarization research decides otherwise.

## Profiles and prompts

- **Base profile / `default.toml` / `extends`.** Variant profiles are standalone
  and `extends` is rejected. Trigger: a third profile, or the same setting must
  change in three files. Until then the duplication is explicit and auditable.
- **Prompt systemization.** Prompts are minimal placeholders. Trigger: a real
  eval failure caused by missing instructions. Do not write a large persona
  before the first end-to-end run.

## Prompt conventions (decided)

- Prohibitions come first; recommended behavior comes after.
- Instructions are written in English.
- Entity names, identifiers, file paths and user-provided text keep the
  entity's own language and spelling. Do not translate or transliterate them.

## Pipelines and registries

- **Unified pipeline registry.** Context stages are assembled in the host with
  `ContextBuilder::new`; tools use `ToolRegistry`; compaction and verification
  are not registry-driven. Trigger: a second host, plugin packs, or more than
  about four stages. A read-only listing command can be added earlier if
  discoverability becomes the actual pain.

## Verification

- **Human verification and deferred acceptance items.** Trigger: M5, or the
  first plan that needs user sign-off.

## Execution safety

- **OS-level command sandbox.** `shell` runs allowlisted programs with fixed
  arguments and a visible confirmation prompt, but it is not a filesystem or
  network sandbox: an approved program can still read paths outside the
  workspace. The hard deny list does not parse command arguments. Trigger: M5,
  or the first external/untrusted workspace. Until then, `workspace_auto`
  requires confirmation and `full_auto` is an explicit trust decision.
