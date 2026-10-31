# Backlog

Items raised during design that are intentionally deferred. Each item states
the trigger that makes it worth doing. "Later" means "not on the M2 critical
path", not "never".

## Context

- **Cross-turn history stage.** Current context is single-turn: `BuildCtx.history`
  is empty and the model must not refer to earlier turns. Trigger: a product
  requirement for follow-up conversation, or M3 companion.
- **Memory stage (facts/episodic).** No consumer in the coding variant yet.
  Trigger: M3 companion, or coding needs cross-task facts.
- **Compaction ladder (trim, rolling summary, LLM compact).** Current behavior
  is an explicit `ContextOverflow` error, which is safer than silent loss.
  Trigger: a real task hits the budget, or M5.
- **Decided now:** context assembly is single-turn. It must not include a
  history stage and must not ask the model to refer to "earlier" content.
  When the budget is tight, drop from the front and keep the most recent
  fragments.

## Profiles and prompts

- **Base profile / `default.toml` / `extends`.** Variant profiles are standalone
  and `extends` is rejected. Trigger: a third profile, or the same setting must
  change in three files. Until then the duplication is explicit and auditable.
- **Prompt systemization.** Prompts are minimal placeholders. Trigger: a real
  eval failure caused by missing instructions. Do not write a large persona
  before the first end-to-end run.

## Pipelines and registries

- **Unified pipeline registry.** Context stages are assembled in the host with
  `ContextBuilder::new`; tools use `ToolRegistry`; compaction and verification
  are not registry-driven. Trigger: a second host, plugin packs, or more than
  about four stages. A read-only listing command can be added earlier if
  discoverability becomes the actual pain.

## Verification

- **Command and test verifiers.** They need the structured shell tool so they
  can run without string interpolation. Trigger: shell tool lands.
- **Human verification and deferred acceptance items.** Trigger: M5, or the
  first plan that needs user sign-off.
