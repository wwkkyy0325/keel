# Validation matrix

Last updated: 2026-11-04. A mechanism can be covered by unit tests, synthetic
runs, real runs and ablations; one column being empty is a known gap, not a
claim of failure.

## Coverage

| Mechanism | Unit / synthetic | Real run | Ablation | Repeat |
|---|---|---|---|---|
| Context governor and watermarks | Window matrix 8k-1M, tool headroom | 15-chapter, 57-turn, 11-turn rerun | None | No |
| Topic segmentation | Unit tests | 15-chapter, 24-turn | None | No |
| Segment records / digests | Window matrix | 15-chapter | None | No |
| Rolling memo | Unit tests, reasoning-none test | 15-chapter, 24-turn, 57-turn, local 26B | `no-memo` | No |
| Recall | Unit tests, IDF suite | 24-turn A/B, 15-chapter, local 26B 8k/16k | `no-recall` | No |
| Task state / closure / locked list | Unit tests, revert on failure | 15-chapter, 57-turn, local chapter | `no-todo` | No |
| Derived current task | Unit tests | 15-chapter A/B | None | No |
| Bounded live tool results | Unit tests | 15-chapter, local chapter | `unbounded` | No |
| Mid-turn elision | Unit tests | 15-chapter | None (no close-only arm) | No |
| Wrap-up / final call | Unit tests, window matrix | 15-chapter | None | No |
| Acceptance checklist | Unit tests | Fusion run (in progress) | None | No |
| Interactions | Unit tests | None in the novel workload | None | No |
| Permissions / trash / audit | M2 unit tests | None in the novel workload | None | No |
| Provider compatibility | 14B/26B probes | 24-turn, local chapter | None | No |
| Prompt cache reuse | Per-call usage lines | 24k novel, 2 turns (67–83%) | Stable-prefix rule not started | No |
| Persona affordances | None | Not measured | None (no A/B) | No |
| Interjections | None | None | None | No (ADR 0010 only) |
| Session persistence (host store) | Snapshot round-trip, projection replace, history, context snapshot, event stream, turn error, fault injection (failed save rolls back) | Two-process resume | None | No |
| Branching / deletion | Checkpoint per turn, fork restore, pruning with pinned fork points, fork/delete errors | Mock parent/branch/delete and a retention smoke | None | No |
| Frontend views | Composed panels, ordering, decoded answer/usage/context, typed memory timeline, event stream, turn paging, golden file, real-binary contract tests | CLI smoke of session, turn, memory, events and two-page session views | None | No |
| Companion personas | Per-turn rule checks, three memory probes | Five personas, 20 turns each, one process per persona | None | No |
| Performance gates | Release tests: turn, context, dispatch, recall, enqueue, 500-turn stress, event persistence | Local release run | None | No |
| M4 plugins | Schema mapping and description, roots/ping replies over stdio and HTTP, mock HTTP MCP server, process-tree kill | Filesystem, memory, Python SQLite probe and call with a roots round-trip | None | No |
| Resource bounds | 16 MiB capture, 200 kB real command, sparse 9 MiB `read_file`, shell timeout, descendant kill | Python fixture, Windows | None | No |

## Fusion evidence

| Combination | Mechanisms exercised | Result |
|---|---|---|
| 15-chapter `full` | governor, records, todo, tools, elision, weak memo/recall | 15/15, 0 overflow, closed, 776k input |
| 15-chapter `full-derived` | same, one fewer todo round | 15/15, 0 overflow, closed, 722k input |
| 24-turn companion `full` / `no-recall` | memo, recall, provider | 24/24 both; recall A/B answered Marlin vs denied it |
| Local 26B 24-turn 8k / 16k | provider, memo, recall | 24/24; 8k explicit recall correct, 16k pasta correct |
| Local 26B chapter | tools, todo, persona, provider | 1 chapter, 1,184 CJK, state updated, task closed |
| 57-turn fusion run | tools, todo, memo, recall, elision, acceptance, probes | **Failed at 16k**: 49/57 Done, 28/50 chapters, 8 overflow, acceptance missed 16 silent skips |
| 15-turn fusion slice | tools, sequential todo, 24k, per-turn acceptance, memo, recall | 15/15 Done, 0 overflow, 14/14 chapters, 14/14 tasks closed, acceptance 15/15; recall marker noise fixed afterwards |
| 3-turn micro-slice | same, narrowed recall markers | 3/3 Done, 0 overflow, 2/2 chapters, 2/2 tasks closed, no recall on chapter turns |
| Local 26B 15-turn slice | tools, sequential todo, 20k, per-turn acceptance | 1 turn: chapter written, state missing; acceptance caught; stopped |
| Local 26B micro-slice | same, explicit state instruction | 1/2 chapters; turn 2 missing `ch-002.md` while task 2 completed; acceptance caught; stopped |
| Full 50 first attempt | DeepSeek, 24k, per-turn acceptance | 10 turns: 9 Done, 1 provider timeout; 10 chapters, no gaps, tasks 9/50, acceptance 9/9 |
| Full 50 second attempt | DeepSeek, 24k, `--retry-errors 2` | 11 turns: replayed a completed chapter, wrote chapter 9 for a chapter-8 request, acceptance caught it; stopped |
| Full 50 third attempt | DeepSeek, 24k, checklist between retries, 300 s deadline | **57/57 Done, 50/50 chapters, 50/50 tasks closed, 57/57 acceptance, 0 overflow, 78.9% cache hit, 43 min** |

## Memory abilities

| Ability | Kernel test | Real run | Gap |
|---|---|---|---|
| Information extraction | Fact scope tests, record extraction, reply-instruction filter | 24-turn, 15-chapter, two-process memory eval | Other imperative phrasing can still become a fact |
| Multi-session reasoning | Snapshot restore test | Two-process eval: 3/3 stored facts answered | One scenario set, one model |
| Temporal reasoning | None | Two-process eval: "before" answered, absolute time refused | No event-time model in the prompt |
| Knowledge updates | Contradiction, subject supersede, forget tests | Rerun: one current fact after the update, answered 海鸥; "before" from the memo | Supersede needs an explicit update cue; model variance not separated |
| Abstention | Unrelated recall test | Two-process eval refused a car plate | Answer-level check is model-dependent |

## Known gaps

- The 57-turn 16k fusion is a negative result: the host loop advanced after
  errors and silent skips, acceptance checked only the latest chapter, the
  locked task plan was not sequential, and 16k was too small for three bible
  reads plus a chapter write. Its fixes are listed in `tasks/fusion-run.md`.
- The 24k fusion completed once: 57/57 Done, 50/50 chapters, 50/50 tasks,
  57/57 acceptance, 0 overflow, 1 recall turn with 2 hits. It is a single
  successful run; model variance is not separated.
- The memo fallback fired on 22 of 56 revisions (20 empty, 2 short) before the
  summarizer stopped spending output tokens on reasoning; an 11-turn rerun had
  10/10 model-written summaries and no fallback.
- Session state is in-process: a new process starts the task list, memo and
  recall empty. Multi-session memory and resume depend on M5 persistence.
- One run per ablation arm; model variance is not separated.
- The 15-chapter protocol's state file carries the story, so memo and recall are
  not load-bearing there.
- The IDF recall change and the summary floor are validated by unit tests and a
  DeepSeek 8k run, not by a repeated long run or the local 26B.
- Acceptance is wired into the CLI but was not part of a long run before the
  fusion run.
- Cache reuse is measured on two turns only. The cost model in
  `cache-economics.md` predicts a small ordering penalty; the cache-order
  profile exists but has not run.
- Persona affordances have no A/B measurement.
- Interjections are a contract only, with no run. M4 covers the MCP path,
  restart-based configuration and a non-Node server family; server-initiated
  requests and the nested-schema description are open. M5 covers resume,
  branching, deletion and fault injection; checkpoint retention is open.

## How to update

After a run, change one row: add the run name, the metrics that matter
(overflow, memo revisions, recall hits, acceptance reprompts, input/calls) and
whether it was repeated. Do not mark a mechanism "validated" on a single
successful run; state the coverage and the gap.
