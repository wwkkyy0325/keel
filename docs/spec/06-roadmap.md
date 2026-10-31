# 06 — Roadmap

Each milestone has a verifiable definition of done. Do not start the next one
until the current one passes.

## M0 — Boundary and paperwork (1-3 days)

- [ ] Contract, confidentiality and side-project clauses reviewed.
- [ ] New repository, name, license.
- [ ] `CLAUDE.md`, `docs/coding-rules.md`.
- [ ] `docs/spec/00`-`05` written and reviewed.
- [ ] `PROVENANCE.md` and `docs/spec/05-provenance.md` current.
- [ ] No employer code, docs, data, names or `reference/` material present.

Done when: the spec set is committed and a reviewer can answer "where did each
input come from?"

## M1 — Kernel contract and mock loop (1-2 weeks)

Only three workspace members: `keel-core`, `keel-runtime`, `hosts/keel-cli`.
Policy, permissions and the mock model are modules inside `keel-runtime`.

- [ ] `keel-core`: IDs, messages, tool schema/result, error envelope, events.
- [ ] `keel-runtime`: FSM, turn driver, tool dispatch, context assembly,
      policy module, permission module, mock model.
- [ ] `hosts/keel-cli`: load a profile, run one turn.
- [ ] Three tools: `echo`, `read_file`, `write_file`.
- [ ] Event log, cancellation, timeout.
- [ ] 20+ unit/integration tests.

Done when: `cargo test -p keel-runtime` passes and `keel-cli` runs one turn;
no network or real model.

## M2 — One real provider and coding profile (1-2 weeks)

- [ ] OpenAI-compatible streaming provider.
- [ ] `keel-policy`: profile schema, resolution, `RuntimeConfig`.
- [ ] Coding profile + 6 tools.
- [ ] One coding case passes `verify.sh`.
- [ ] Kernel overhead benchmark recorded.

Done when: one task is solved end-to-end with a real model.

## M3 — Companion profile (1 week)

- [ ] Persona stage, episodic memory, no tools.
- [ ] Companion profile.
- [ ] 20-turn conversation runs with no kernel changes.

Done when: the only diff from M2 is profile/plugin data.

## M4 — Plugins (2-3 weeks)

- [ ] Manifest, registry, lifecycle.
- [ ] Coding and companion packs.
- [ ] Load/unload/health; denied capability escalation.
- [ ] Contract tests for every extension point.

Done when: removing a plugin removes its contributions and no kernel edit is
needed to add a third.

## M5 — Permissions, memory, compaction (2-3 weeks)

- [ ] Four permission levels, path scan, audit.
- [ ] Workspace trust and external-path confirmation.
- [ ] Memory: diary + facts + promotion.
- [ ] Compaction ladder.
- [ ] Fault-injection tests.

Done when: the permission and failure test suites pass.

## M6 — Evals and portfolio (1-2 weeks)

- [ ] 20 coding cases, 5 companion characters.
- [ ] CI smoke/nightly.
- [ ] Report with failures, cost and latency.
- [ ] README, architecture note, case study.
- [ ] License/asset audit and legal review.

Done when: the report is reproducible from the repository.

## After M6

- process-isolated plugins;
- vector memory / KG (optional);
- additional variants;
- packaging and release.
