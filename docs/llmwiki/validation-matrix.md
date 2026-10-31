# Validation matrix

Last updated: 2026-11-03. A mechanism can be covered by unit tests, synthetic
runs, real runs and ablations; one column being empty is a known gap, not a
claim of failure.

## Coverage

| Mechanism | Unit / synthetic | Real run | Ablation | Repeat |
|---|---|---|---|---|
| Context governor and watermarks | Window matrix 8k-1M | 15-chapter novel | None | No |
| Topic segmentation | Unit tests | 15-chapter, 24-turn | None | No |
| Segment records / digests | Window matrix | 15-chapter | None | No |
| Rolling memo | Unit tests | 15-chapter, 24-turn, local 26B | `no-memo` | No |
| Recall | Unit tests, IDF suite | 24-turn A/B, 15-chapter, local 26B 8k/16k | `no-recall` | No |
| Task state / closure / locked list | Unit tests | 15-chapter, local chapter | `no-todo` | No |
| Derived current task | Unit tests | 15-chapter A/B | None | No |
| Bounded live tool results | Unit tests | 15-chapter, local chapter | `unbounded` | No |
| Mid-turn elision | Unit tests | 15-chapter | None (no close-only arm) | No |
| Wrap-up / final call | Unit tests, window matrix | 15-chapter | None | No |
| Acceptance checklist | Unit tests | Fusion run (in progress) | None | No |
| Interactions | Unit tests | None in the novel workload | None | No |
| Permissions / trash / audit | M2 unit tests | None in the novel workload | None | No |
| Provider compatibility | 14B/26B probes | 24-turn, local chapter | None | No |
| Persona affordances | None | Not measured | None (no A/B) | No |
| Interjections | None | None | None | No (ADR 0010 only) |
| M4 plugins / M5 persistence | Not started | Not started | Not started | Not started |

## Fusion evidence

| Combination | Mechanisms exercised | Result |
|---|---|---|
| 15-chapter `full` | governor, records, todo, tools, elision, weak memo/recall | 15/15, 0 overflow, closed, 776k input |
| 15-chapter `full-derived` | same, one fewer todo round | 15/15, 0 overflow, closed, 722k input |
| 24-turn companion `full` / `no-recall` | memo, recall, provider | 24/24 both; recall A/B answered Marlin vs denied it |
| Local 26B 24-turn 8k / 16k | provider, memo, recall | 24/24; 8k explicit recall correct, 16k pasta correct |
| Local 26B chapter | tools, todo, persona, provider | 1 chapter, 1,184 CJK, state updated, task closed |
| 57-turn fusion run | tools, todo, memo, recall, elision, acceptance, probes | In progress |

## Memory abilities

| Ability | Kernel test | Real run | Gap |
|---|---|---|---|
| Information extraction | Fact scope tests, record extraction | 24-turn, 15-chapter | No standard scenario set |
| Multi-session reasoning | None | None | M5 persistence |
| Temporal reasoning | None | None | No event-time model |
| Knowledge updates | Contradiction, supersede, forget tests | Memo rewrite | No cross-session conflict policy |
| Abstention | Unrelated recall test | 24-turn probes | No answer-level check; acceptance would be host-side |

## Known gaps

- No single run yet combines a pressure-tested memo/recall with tools,
  acceptance and repeated arms; the 57-turn fusion run is the first attempt.
- One run per ablation arm; model variance is not separated.
- The 15-chapter protocol's state file carries the story, so memo and recall are
  not load-bearing there.
- The IDF recall change and the summary floor are validated by unit tests and a
  DeepSeek 8k run, not by a repeated long run or the local 26B.
- Acceptance is wired into the CLI but was not part of a long run before the
  fusion run.
- Persona affordances have no A/B measurement.
- Interjections are a contract only; M4 and M5 are not started.

## How to update

After a run, change one row: add the run name, the metrics that matter
(overflow, memo revisions, recall hits, acceptance reprompts, input/calls) and
whether it was repeated. Do not mark a mechanism "validated" on a single
successful run; state the coverage and the gap.
