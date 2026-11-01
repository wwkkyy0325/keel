# License and asset audit

Date: 2026-11-01. This is an engineering audit, not legal advice.

## Project license

Keel is Apache-2.0; the full text is in `LICENSE`. The workspace crates declare
`license = "Apache-2.0"` in `Cargo.toml`.

Open item: the Apache text has no copyright owner line and there is no
`NOTICE` file. Add one before a public release if the project needs an
attribution notice.

## Rust dependencies

Policy is enforced by `cargo deny check licenses` with an allowlist in
`deny.toml`: MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause,
BSD-3-Clause, ISC, Unicode-3.0, Zlib and Unlicense. `openssl` and `openssl-sys`
are banned; the HTTP client uses rustls. Unknown registries and git sources are
denied. The last check on this tree reported advisories, bans, licenses and
sources all ok.

Direct dependencies and their declared licenses:

| Crate | Version | License |
|---|---|---|
| async-trait | 0.1.92 | MIT OR Apache-2.0 |
| dotenvy | 0.15.7 | MIT |
| reqwest | 0.12.28 | MIT OR Apache-2.0 |
| rusqlite | 0.32.1 | MIT |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.154 | MIT OR Apache-2.0 |
| sha2 | 0.10.9 | MIT OR Apache-2.0 |
| tempfile | 3.27.0 | MIT OR Apache-2.0 |
| thiserror | 2.0.21 | MIT OR Apache-2.0 |
| tokio | 1.54.1 | MIT |
| toml | 0.8.23 | MIT OR Apache-2.0 |

The allowlist covers the transitive tree; `cargo deny` is the authoritative
check. Re-run it after any dependency change.

## Datasets

- HumanEval+ and MBPP+ (EvalPlus): Apache-2.0. Downloaded locally into
  `evals/cases/public/`, which is gitignored; no dataset rows are vendored.
- BigCodeBench: Apache-2.0, planned; not downloaded or vendored.
- SWE-bench harness: MIT; each repository keeps its own license. Not used yet;
  future cases run in isolated workspaces and copy no code.

The provenance ledger in `docs/spec/05-provenance.md` records every external
input and its license.

## External services and plugins

- MCP servers used for testing (filesystem, memory) are installed by the user
  and are not distributed with Keel; their own licenses apply. The Python
  SQLite fixture in `evals/harness/fixtures/` is original and uses only the
  standard library.
- Model providers are reached over their APIs; no model weights or vendor code
  are included.
- Hosted MCP endpoints are not configured in the repository; keys stay in the
  environment.

## Prompts, personas and docs

All prompts, persona files, specs, ADRs and reports are original work for this
repository. Public papers and documentation listed in the provenance ledger
were read for comparison only; no text or code was copied. The memory
comparison notes cite public papers; vendor claims are marked unverified.

## Assets

There are no fonts, images, audio or other binary assets in the repository.
The Python fixtures and profiles are plain text.

## Names and trademarks

"Keel" is the project name. "DeepSeek", "OpenAI", "Model Context Protocol" and
other product names are used descriptively; no affiliation is implied.

## Checklist before release

- [ ] Add a `NOTICE` or copyright line if attribution is required.
- [ ] Re-run `cargo deny check` on the release lockfile.
- [ ] Confirm `evals/cases/public/` and `.env` are absent from the release.
- [ ] Confirm no API key or token appears in git history or reports.
- [ ] Review third-party dataset terms again if any dataset is vendored.
