Build a small consumer project on top of the Keel kernel.

Where you work: the workspace the shell starts in. Run `pwd` first, then
create everything with paths relative to it. Every file you write must stay
inside that workspace; never use an absolute path for a write.

The kernel crates live at these absolute paths and are read-only for you:

- `D:/workspace-new/wwkkyy0325/Keel/crates/keel-core`
- `D:/workspace-new/wwkkyy0325/Keel/crates/keel-policy`
- `D:/workspace-new/wwkkyy0325/Keel/crates/keel-runtime`

Read `D:/workspace-new/wwkkyy0325/Keel/crates/keel-runtime/examples/minimal_turn.rs`
first; it is the shape to follow.

Create `keel-notes/` in the workspace with a Rust binary crate named
`keel-notes`:

1. `keel-notes ask "<text>"` runs exactly one kernel turn with the mock model
   and prints two lines: `state: <state>` and `answer: <answer>`.
2. The profile is embedded in the binary as a string. `prompt.md` sits next to
   `Cargo.toml` and is loaded through
   `from_profile_str(PROFILE, Path::new(env!("CARGO_MANIFEST_DIR")))`.
3. The mock model answers `mock turn complete: echoed N bytes`, where N is the
   byte length of the input.
4. At least one unit test runs the same wiring and asserts the answer.
5. `cargo test` and `cargo run -- ask "hello"` must succeed from `keel-notes/`.

Constraints:

- depend on the three kernel crates by path, using the absolute paths above
  with forward slashes (Cargo accepts them on Windows);
- do not modify the Keel repository; make no network calls at run time;
- run `cargo fmt` and `cargo clippy -- -D warnings` when they are available.
