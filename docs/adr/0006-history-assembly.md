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

### 3. Assembly is budget-driven

Assembly runs per model call, after the pinned stages, with the budget
remaining at the history stage:

1. the current input is pinned and never compressed;
2. walk the current segment backwards and keep whole turns verbatim while they
   fit; the floor is two turns;
3. if the floor does not fit, degrade the oldest of those turns to a digest
   rather than cutting it mid-sentence;
4. spend what remains on older turns of the current segment as digests, newest
   first;
5. then on sealed segment records, newest first, emitted in chronological
   order;
6. then on session facts, which are emitted by the memory stage before
   history.

If the pinned parts alone exceed the budget the turn ends in
`ContextOverflow`; the kernel does not silently emit a partial prompt.

Nothing is fixed at compile time: the detail window is the largest number of
turns that fits between the floor and the ceiling.

### 4. Compression ladder and triggers

- `L0` raw entry.
- `L1` tool output truncated at the head and tail, by character count.
- `L2` turn digest: input, assistant text, tool calls with status, verification
  result.
- `L3` segment record.
- `L4` fact folding: a segment record is dropped once its facts are promoted
  and it has no open items.

Triggers are exactly two: a segment seal, and budget pressure during assembly.
There is no per-turn background summarization; it would spend tokens on every
turn to save tokens later.

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

Promotion is strict:

- a fact becomes global only when it is durable, does not reference a
  transient task, and was confirmed by a verifier or stated by the user;
- everything else stays session-scoped and is discarded at session end;
- a later contradiction demotes or drops the earlier fact.

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
- Compression quality is measurable: `CompressionSkipped`, `HistoryAssembled`
  and probe questions in the eval harness give evidence instead of intuition.
- Facts have provenance, so a wrong memory can be traced to the turn that
  produced it.

## Alternatives rejected

- Flat last-N turns: cannot keep a long task's result while dropping its
  process, and overflows exactly when the task is most expensive.
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
