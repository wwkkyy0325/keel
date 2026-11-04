# Upgrading the kernel in a derived host

A product pins the kernel and updates it deliberately. This page is the
checklist for one upgrade. The rules behind it are in `docs/adr/0014-kernel-distribution.md`
and the stable surface is in `docs/kernel-contract.md`.

## Pin, never follow

```toml
# Development against a local checkout
keel-runtime = { path = "D:/workspace/Keel/crates/keel-runtime" }

# A frozen release
keel-runtime = { git = "file:///D:/workspace/Keel", tag = "v0.1.0" }
```

Path dependencies follow the working tree and are for joint development. A tag
is the only form that reproduces a build. Never depend on `main` in a product.

## Upgrade steps

1. Read `CHANGELOG.md` and the new ADRs between the two tags.
2. On a branch, change the tag and run `cargo update -p keel-core` (and the
   other `keel-*` packages).
3. Compile. Type changes are the first signal and the cheapest one.
4. Run the product's tests, then the conformance probes (below).
5. Run one or two real smoke cases; mock runs do not catch a behavior change.
6. If the product stores sessions or events, check the versions:
   - `SessionSnapshot.format_version` (kernel refuses a newer snapshot);
   - `keel_core::EVENT_FORMAT_VERSION` for the stored event shape.
   Add a migration if either changed. Migration is the host's job.
7. Tag the product and record the kernel tag it depends on.

## Conformance probes

`keel-testkit` is a dev-dependency (`publish = false`) that drives a real
`TurnDriver` with a mock model:

```toml
[dev-dependencies]
keel-testkit = { path = "D:/workspace/Keel/crates/keel-testkit" }
```

```rust
use keel_runtime::MockStep;
use keel_testkit::{assert_done, assert_event_order, text_turn_events, Probe};

#[tokio::test]
async fn the_kernel_contract_still_holds() {
    let probe = Probe::new(vec![MockStep::Text("hello".to_owned())]).unwrap();
    let outcome = probe.run("hi").await;
    assert_done(&outcome).unwrap();
    assert_event_order(probe.events(), &text_turn_events()).unwrap();
}
```

The probes cover a text turn, a tool turn, a model failure, cancellation and the
snapshot version. They prove the kernel still behaves as the contract says;
they do not prove the host's own composition. A host should also keep a small
integration suite that builds its own driver and tools.

`event_names_contract()` returns every event name the kernel can emit. A host
that matches on events can compare it after an upgrade and fail on an unknown
name instead of silently ignoring a new boundary.

## What counts as breaking

- A type, method or trait change: compile error. Major after 1.0, minor
  before, with an ADR and a changelog entry.
- A new event field: additive. Existing readers must tolerate missing fields
  in older records; `EVENT_FORMAT_VERSION` does not change.
- An event rename or removal, or a field whose meaning changes: breaking.
  Bump `EVENT_FORMAT_VERSION` and document the migration.
- A new event variant: add it to `event_names_contract()`. Hosts that switch
  exhaustively will fail to compile, which is the intended signal.

## Semver check

Before tagging a kernel release, compare the public API with the previous tag:

```bash
cargo install cargo-semver-checks --locked
cargo semver-checks check-release --baseline-rev v0.1.0
```

The check catches a signature change that a changelog entry forgot to mention.
It complements the tests; it does not replace them.
