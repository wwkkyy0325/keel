# ADR 0006 — History assembly and memory

## Status

Accepted. Summarization is the one open item; it is tracked in
`docs/llmwiki/tasks/m3-summarization-research.md` and must be decided before
any compression level calls a model.

## Context

The driver currently passes an empty history and every CLI invocation is a
single turn. M3 requires a 20-turn companion conversation with no kernel
changes on the companion path, so the kernel needs a history contract, not a
prompt convention.

Two failure modes bracket the problem. Keeping every turn overflows the
context. Dropping turns silently loses facts the user already established.
Both are invisible until a wrong answer appears several turns later, which is
the risk raised during M2.

The product model is fixed by the user:

- history is segmented by task topic;
- the most recent turns stay in detail;
- older content is compressed according to remaining context, keeping results
  and the key parts of the process;
- tool calls enter history as summaries, and long outputs are truncated at the
  head and tail by character count, with the full output still reachable;
- compression is triggered by stages and is fully automatic; the user never
  manages it;
- facts are sedimented: durable facts are promoted to global scope, everything
  else stays session-scoped.

## Decision

### 1. Data model

- `Session` owns an ordered list of `Segment`s and a session-scoped fact store.
- `Segment` has an id, a topic label, a lexical term signature, the artifact
  paths it touched, a state (`Open` or `Sealed`), raw entries while open, and
  a `SegmentRecord` once compressed.
- `Entry` is one recorded item: a user message, an assistant message, or a
  tool summary. Entries are the only input to compression; the event log stays
  the full audit trail.
- `SegmentRecord` fields: goal, result, key decisions, key errors and their
  resolution, artifacts, verification outcome, open items, incomplete flag.

### 2. Segmentation is kernel-owned

The kernel decides segment boundaries. The model may not open or seal a
segment, and the host does not manage it. The heuristic is deterministic and
makes no model call:

- a segment closes when any of these holds:
  - the input's lexical overlap with the segment signature is below the
    threshold and it references none of the segment's artifacts;
  - the previous turn ended with a passing acceptance report and no open
    items;
  - the segment exceeds the turn cap or the raw-token cap;
  - the idle gap exceeds the configured limit.
- a new input after a seal opens a new segment; a sealed segment is never
  reopened, and a later input that matches it starts a new segment with a
  back-reference.

Thresholds live in configuration and are expressed as fractions of the context
budget where possible, so a larger window keeps more detail without a code
change. Defaults are chosen to under-segment: a wrong seal costs detail, not
correctness.

### 3. A context governor, not a fixed allocation

A fixed share is a quota, not a control loop. The kernel runs a governor that
measures pressure before every model call and decides what to do about it.

**Gauge.** Before each call:

```
capacity       = model_window - output_reserve
ceiling        = min(capacity, working_set_target)
used           = estimated input + tool schemas + system + persona + current input
growth         = max(EWMA of per-call growth, last call growth, floor)
headroom_calls = (ceiling - used) / growth
```

`working_set_target` is the profile's `budget_tokens`, not the model window. A
1M window does not mean a 1M working set: observed sessions plateau near 200k
and come back down, so the target is what keeps cost and latency at the working
set while `capacity` stays the hard limit. Validation requires
`working_set_target + output_reserve <= model_window`.

The estimate is the cheap one used elsewhere; after a call the adapter's
reported usage replaces it when larger.

**Levels are headroom in calls, not percentages.** A percentage of the window
is wrong in both directions: on a 1M window it fires at 850k, long after the
working set has plateaued; on a 16k window it fires after one large tool
result. The trigger is how many more calls fit at the observed growth rate:

| Level | Condition | Action |
|---|---|---|
| green | more than 4 calls of headroom | proceed |
| amber | 4 or fewer | micro-compaction: truncate or drop old tool results, drop promoted records; no model call |
| orange | 2 or fewer | seal the segment and build `L2`/`L3` structured records; shrink the detail share |
| red | 1 or fewer, or `used` above 90% of the ceiling | inject a wrap-up instruction, then compact; if no call fits, compact directly |
| critical | still over the ceiling after compaction | `ContextOverflow` with the composition in the error |

The 90% guard exists because growth can spike after the estimate was taken. The
thresholds move with the growth rate: a fast coding turn compacts earlier in
absolute tokens, a slow chat runs longer.

**The detail share is a function of the level**, not a constant: 80% at green,
60% at amber, 40% at orange, 20% at red, always with the two-turn floor. This
is the dynamic behavior the fixed 70% share lacked.

**Wrap-up.** At red the kernel injects one instruction before the model call:
do not start new work, finish the step in flight, then report goal, done,
pending, artifacts and decisions. That report seeds the segment record. The
injection happens once per threshold crossing, and is skipped when there is
nothing worth compacting.

**Two budgets, one wrap-up path.** The context window is not the only budget.
The turn also has a model-call limit and a wall-clock limit. When one model
call remains, the kernel forbids new tool calls and asks for the final answer;
the wrap-up report and the final answer share the same reserved call. An abrupt
`TurnsExhausted` is the fallback only when even that call does not fit.

**Safe points.** Every action runs at the top of the model-call loop, after
tool results are appended and before messages are built. Never during a stream,
an interaction wait, or verification. Compaction never touches messages
produced in the current turn, only completed turns and sealed segments.

**Coordination rules.**

- hysteresis: after a compaction, do not compact again until the ratio falls
  back below amber; at most two compactions per turn;
- circuit breaker: three consecutive compaction failures end the turn with a
  typed error instead of looping;
- boundary marker: compaction inserts a `HistoryBoundary` message so the model
  can tell summarized history from verbatim history;
- re-inject after compaction: current task, open items, artifacts and the
  latest wrap-up report are pinned again;
- cost guard: skip an action whose estimated savings are below a floor, so a
  compaction cannot cost more than it saves;
- determinism: given the same session and budget, the governor takes the same
  decisions.

**Assembly order.** Once the governor has decided, assembly is:

1. pinned input, never compressed;
2. the detail window, sized in tokens at the current level's share, no turn
   ceiling, two-turn floor;
3. older turns of the current segment as digests, newest first;
4. sealed segment records, newest first, emitted in chronological order;
5. session facts, emitted by the memory stage before history.

If the pinned parts alone exceed the budget, the turn ends in `ContextOverflow`;
the kernel does not silently emit a partial prompt.

**Minimum supported window.** Fixed per-call overhead is the system and persona
prompt, the tool schemas, the output reserve and the current input. Measured
here: prompt about 150 tokens, six tool schemas about 1.5k, output reserve up
to `max_output_tokens`, current input about 200 tokens. That leaves roughly:

| Window | Usable after overhead | Provisional floor for |
|---|---|---|
| 8k | about 6.5k with no tools and a 1k output reserve | companion, no tools |
| 16k | about 10k with six tools and a 4k reserve | coding |
| 32k and above | comfortable for coding | — |

Below the floor the config is rejected with a clear error rather than running
and thrashing. These numbers are provisional; the window-matrix test in the M3
task doc moves them with evidence. The output reserve is a cap, not a constant
charge: a tool-call turn reserves less than a final-answer turn.

### 4. Compression ladder and triggers

- `L0` raw entry.
- `L1` tool output truncated at the head and tail, by character count.
- `L2` turn digest: input, assistant text, tool calls with status, verification
  result.
- `L3` segment record.
- `L4` fact folding: a segment record is dropped once its facts are promoted
  and it has no open items.

Triggers are exactly two: a segment seal, and a governor level of amber or
above. There is no per-turn background summarization; it would spend tokens on
every turn to save tokens later. Micro-compaction runs only when its estimated
savings clear the cost guard, so an edit that saves less than it costs is
skipped.

### 5. Retention follows facts, not a fixed count

Segment records are not kept to a fixed number:

- a record with open items is pinned and is dropped last;
- a record whose facts are all promoted and whose artifacts are not referenced
  again is droppable first;
- everything else is dropped oldest-first only under pressure.

This makes the number of retained segments a function of what the session
actually established, which is the stated intent.

### 6. Sufficiency gate

Before a segment is compressed, the kernel checks whether the candidate record
carries enough to continue without the raw entries: goal, result, one line per
artifact that changed, every encountered error with a resolution or an open
item, and a verification outcome when a plan existed.

- If the check passes, the raw entries are dropped.
- If it fails, the segment stays raw and the attempt is recorded as
  `CompressionSkipped` with the missing fields.
- Under hard pressure the record is emitted with `incomplete = true` and a
  visible marker, so the model knows detail was lost instead of assuming it
  never existed.

### 7. Tool entries

- The call is recorded as tool name plus key arguments.
- The result is recorded as status, exit code, and the head and tail of the
  output by character count.
- Only when a result is truncated is the full output written once to
  `.keel/outputs/<id>`; the truncation marker carries that path so the model
  can re-read it with `read_file` when the summary is not enough.
- Results produced earlier in the same turn are not truncated in place;
  truncation applies when the entry enters history.
- `read_file` results keep path and line range, so the model can re-request
  any region instead of receiving a cut file.

### 8. Sedimentation

When a segment seals, the kernel extracts candidate facts from structure it
already has: verification outcomes, tool outcomes, changed artifacts, and
user-stated constraints. Candidate facts carry a statement, scope,
confidence, and provenance (segment and turn).

Promotion is strict and never decided by a keyword alone:

- only a sentence-initial imperative counts as a constraint; questions,
  first-person narration, cues in the middle of a sentence, and phrasing tied
  to the current task are rejected;
- an explicit request (`remember`, `记住`, `请记住`, `from now on`, `以后都`)
  promotes the fact immediately; the user performed the act of asking;
- a durable cue (`always`, `never`, `以后`, `永远`) without an explicit request
  makes the sentence a *candidate*, not a global fact;
- a candidate becomes global only after the same statement appears in at least
  two separate turns; until then it stays session-scoped;
- a different statement for the same key resets the count and demotes the
  earlier fact;
- everything else stays session-scoped and is discarded at session end.

The kernel never infers "global" from wording alone, because the same words can
describe a habit, a question, this task, or this workspace.

Session facts are emitted by the memory stage; global facts are loaded by the
host into that stage at session start.

### 9. Invariants

- History never contains secrets; redaction happens before recording.
- Current-turn tool results are never truncated in place.
- Reasoning content is not replayed from history.
- Compression never mutates the message list of the turn in progress.
- Given the same session and budget, assembly is deterministic.
- Assembly emits `HistoryAssembled` with levels used, verbatim turns, records,
  dropped counts, and tokens, so the composition is observable in every run.
- The governor never drops messages of the turn in progress. If pressure
  remains after compaction it fails with `ContextOverflow` instead of sending
  a prompt it knows is incomplete.
- One model call is reserved for wrap-up and the final answer when the turn
  budget is nearly exhausted.
- Every governor decision emits `ContextPressure` with the ratio, level,
  action and estimated savings.

### 10. Summarization is an open research item

`L2` and `L3` are structured extracts in the first implementation: no model
call, no cost, deterministic, and testable. A model-written summary may replace
or augment them only after the research note compares quality, cost, drift and
prompt-injection risk. The interface is level-based so the mechanism can change
without changing assembly.

## Consequences

- The driver must own a `Session` across turns, and the CLI needs a session
  loop; one-shot runs keep a one-segment session.
- `BuildCtx` must expose the remaining budget and the session view, and a
  context stage must be able to emit several messages, because history is a
  sequence of user and assistant turns rather than one text block.
- The companion profile can run 20 turns with no kernel branch; it only sets
  the memory and context configuration.
- Compression quality is measurable: `CompressionSkipped`, `HistoryAssembled`,
  `ContextPressure` and probe questions in the eval harness give evidence
  instead of intuition.
- The driver gains governor state (level, compactions this turn, consecutive
  failures) and one reserved model call, so turn-budget tests must account for
  the wrap-up path.
- Facts have provenance, so a wrong memory can be traced to the turn that
  produced it.

## Alternatives rejected

- Flat last-N turns: cannot keep a long task's result while dropping its
  process, and overflows exactly when the task is most expensive. A fixed
  N = 6 draft of this ADR was also wrong: it is a cap, so it never protects the
  budget of a tool-heavy coding turn, while it cuts a short-turn chat at six
  when fifty would fit.
- Fixed summary every K turns: spends tokens on every turn and compresses
  content that still fits.
- Model-managed segmentation: unreliable, manipulable, and makes assembly
  non-deterministic.
- Fixed segment-record count: ignores which segments actually produced durable
  facts and which are still open.
- Silent drop on overflow: the model then answers from a prompt that is
  missing content with no signal that it is missing.
- Keeping raw tool outputs in history: dominates the budget after two or three
  large reads.
