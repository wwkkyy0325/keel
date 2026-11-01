# Companion personas

Date: 2026-11-04. Five personas, twenty turns each, `deepseek-flash` with the
companion profile (8k window, no tools, 7,168 working set). One process per
persona on one SQLite store; per-turn acceptance forces a segment seal so fact
extraction and the memo run.

```bash
python evals/harness/run_companions.py --max-total-tokens 2000000
```

## Personas

| Persona | Language | Rules checked every turn |
|---|---|---|
| archivist | Chinese | contains “档案”; at most four sentences; no exclamation; no emoji |
| coach | Chinese | ends with a question; at most three sentences; no emoji |
| pirate | English | contains arr/matey/ahoy/ye; at most three sentences; no emoji |
| poet | Chinese | at most five lines; no lists; contains a natural image; no emoji |
| engineer | Chinese | starts with `TL;DR:`; at most eight lines; no emoji |

The Chinese script states three facts in turns 2-4 and asks for them in turns
17-19. The pirate script does the same in English with `blue whale`,
`Tuesday 8pm` and `starlight`.

## Results

| Persona | Turns | Done | Rule counts | Memory | Tokens |
|---|---|---|---|---|---|
| archivist | 20 | 20 | keyword 20/20, sentences 20/20, no emoji 20/20, no exclaim 20/20 | 3/3 | 45,021 |
| coach | 20 | 20 | question 20/20, sentences 20/20, no emoji 20/20 | 3/3 | 46,944 |
| pirate | 20 | 20 | keyword 20/20, sentences 20/20, no emoji 20/20 | 3/3 | 41,809 |
| poet | 20 | 20 | lines 20/20, no lists 20/20, image 20/20, no emoji 20/20 | 3/3 | 44,302 |
| engineer | 20 | 20 | prefix 20/20, lines 20/20, no emoji 20/20 | 3/3 | 53,443 |

- 100 turns, 100 `Done`, no persona rule violations, 15/15 memory probes hit.
- 231,519 tokens. This harness records totals per persona; it does not split
  input/cache/output, so the cost is not stated as a precise figure (roughly
  ¥0.3 at the hit rates seen elsewhere).
- The run is one sample per persona; the model is not seeded.

## Finding: equivalent time expressions

The first run reported an engineer memory miss. The answer was
`TL;DR: 部署窗口是周二 20:00。` — correct, but the check required the literal
“八点”. The check now accepts `八点`, `晚八点`, `20:00`, `8点` and `8:00` (and
the pirate equivalents), and the rerun passed all three probes. This was a
checker normalisation, not a relaxed memory requirement.

## Limits

- Acceptance seals every turn, which is denser than a normal conversation;
  without it, persona checks would still work but memory extraction would run
  less often.
- Persona compliance is measured by surface rules, not by a human or model
  judge. A reply can satisfy the rules and still be a poor companion.
- One scenario per persona and one model; no repeated runs, so variance is not
  separated.
- The pirate scenario is English because the persona speaks English; the other
  four share the Chinese scenario.
