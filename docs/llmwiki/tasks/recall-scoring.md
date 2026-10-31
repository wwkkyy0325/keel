# Recall scoring

Replace the raw term-overlap ratio with IDF-weighted content terms so English
questions and Chinese bigrams both retrieve absorbed segments.

## Steps

- [x] Split query terms from segment terms: add `query_terms` with English
      question/meta words and Chinese meta bigrams. Verify: segmentation tests
      unchanged, existing recall tests pass.
- [x] Weight terms by IDF over the session; ignore query terms absent from the
      session when normalizing. Verify: the pasta query scores 1.00 with one
      known term; an unrelated query still recalls nothing.
- [x] Require one selective matched term (weight > 1.0) or an artifact; lower
      the threshold to 0.20 only when a recall marker is present. Verify: a
      term present in every segment does not recall.
- [x] Add `score_bp`, `matched`, `query_terms` and `known_terms` to
      `RecalledSegment` and `HistoryRecalled`; print them in the CLI. Verify:
      `recall: score=0.73 hits=3/4 known=4 segments=[0] tokens=257`.
- [x] Validate on a real model at the 8k companion profile. Verify: 24/24
      `Done`; turn 23 score 0.73 and `config/marlin.toml`; turn 24 score 1.00
      and `aglio e olio`.
- [ ] Re-run the 26B local probe at 8k when the GPU is free. The GPU was in use
      by another user during this task, so validation used the remote DeepSeek
      endpoint; the scoring is model-independent.

## Verification commands

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Notes

- Do not filter segment terms; segmentation behavior stays unchanged.
- Do not lower the global threshold; only a recall marker does.
- Do not log raw query terms; keep counts and scores.
- Vector recall remains deferred until a real miss has no shared term at all.
