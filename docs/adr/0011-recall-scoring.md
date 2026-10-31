# ADR 0011 — Recall scores content terms with IDF

## Status

Accepted.

## Context

Recall originally scored `hits / input_terms.len()`. A question contains
function words, and Chinese has no spaces, so the query becomes a large bag of
bigrams. The useful term is diluted:

- `What was the pasta recipe I mentioned earlier?` produced five terms and one
  match: `1/5 = 0.20`, below the 0.34 floor. The segment held `pasta`,
  `aglio e olio`, and the answer, but recall never fired.
- A Chinese question such as `之前说的意面配方还记得吗` produces ten bigrams;
  `意面` is the only content match, and the rest are meta words or
  segmentation noise.

The local 26B probe at 8k failed the pasta question for this reason. The memo
was smaller at 8k and had dropped the phrase, but the segment was still
retrievable.

## Decision

### Query terms are filtered; segment terms are not

- Segment terms stay as they are, so topic segmentation is unchanged.
- `query_terms` drops English question and meta words (`what`, `where`,
  `mentioned`, `earlier`, `remember`, ...), and common Chinese meta bigrams
  (`之前`, `刚才`, `上次`, `那个`, `什么`, `记得`, ...).
- A term absent from every segment (`df = 0`) is not evidence and does not
  dilute the score. Without this rule, Chinese bigram noise makes a single
  content match score near zero.

### IDF weighting

```text
N  = segments with a non-empty term set
df = segments containing the term
w(t) = ln((N + 1) / (df + 1)) + 1
score = sum(w(t) for matched terms) / sum(w(t) for known query terms)
```

- Rare terms carry more weight. `Marlin`, `BL-238`, `意面` and character names
  dominate; `之前`, `what` and `那个` are cheap.
- `known query terms` means `df > 0`. Unmatched known terms still count, so a
  segment that matches one of two known content terms scores 0.5, not 1.0.
- An artifact path match sets a floor of 0.9.

### Intent and evidence

- If the input contains a recall marker (`remember`, `earlier`, `mentioned`,
  `之前`, `记得`, `刚才`, ...), the threshold is `0.20`; otherwise it is
  `0.34`.
- At least one matched term must be selective: its weight must exceed 1.0, so
  a term present in every segment cannot trigger recall. An artifact match
  bypasses this rule.
- Still at most two segments, with a 300-character excerpt chosen by weighted
  overlap.

### Observability

- `RecalledSegment` carries `score_bp`, `matched`, `query_terms` and
  `known_terms`. Basis points keep the event `Eq`.
- `HistoryRecalled` carries the same fields; the CLI prints
  `recall: score=0.73 hits=3/4 known=4 segments=[0] tokens=257`.
- Raw query terms are not logged; counts and scores are enough to diagnose a
  miss.

## Consequences

- The DeepSeek 24-turn run at the 8k companion profile now recalls the pasta
  segment: `score=1.00 hits=1/2 known=1`, and the answer names
  `aglio e olio`. The Marlin probe scores `0.73` with `3/4` matches.
- Chinese recall works without a tokenizer or an external index.
- A synonym with no shared term still misses (`recipe` alone would not have
  matched); the vector layer remains deferred.
- The threshold change is not global: only an explicit recall marker lowers
  it. Generic questions do not recall on `那个` or `thing`.
- Segment segmentation behavior is unchanged because only query terms are
  filtered.

## Alternatives rejected

- Lower the global threshold to 0.25: false recalls on generic questions.
- Increase the memo budget: it is pinned every turn and was not the primary
  cause; the segment was retrievable.
- Add a vector index now: dependency, model call and nondeterminism, before
  real misses with no shared term.
- A Chinese word segmenter: a dependency and dictionary for a problem IDF plus
  a small bigram list already handles.
