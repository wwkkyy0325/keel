# Summary quality floor

A model-written summary that is too short must not absorb the records it
cannot replace. A 126-character Chinese memo against a 756-token budget was
the observed case.

## Steps

- [x] Add a summary floor of `max_tokens / 4`, clamped to 64..256. Verify: 756
      gives 189, 4,096 gives 256, 128 gives 64.
- [x] Reject a summary below the floor with a typed error so the driver keeps
      the previous memo and writes the structured fallback. Verify: a
      126-character Chinese summary is rejected; a normal long summary passes.
- [x] Document the gate in ADR 0006. Verify: the sufficiency-gate section names
      the summary floor and the fallback path.
- [ ] Re-run a real 24-turn session to confirm the fallback rate does not rise
      on normal summaries. Blocked on a free GPU for the local model; the
      deterministic tests cover the boundary.

## Verification commands

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
