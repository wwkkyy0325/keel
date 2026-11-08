# Security

## Reporting a vulnerability

Do not open a public issue. Use GitHub's **Report a vulnerability** tab on
`https://github.com/wwkkyy0325/keel`, or contact the maintainer listed on the
repository profile. Include the affected version, a minimal reproduction and
the impact you observed.

## Scope

Keel gates tool calls, not processes. `workspace_auto` confines file paths;
an approved `shell` command or an MCP server runs outside that boundary. OS
confinement (Landlock, `sandbox-exec`, Job Objects) is a backstop, not a
virtual machine. Treat the permission layer as policy, not isolation.

The kernel's guarantees are listed in `docs/spec/01-kernel.md`. A bypass of the
permission gate, path canonicalization, cancellation or the audit event stream
is a security issue. A tool doing something the operator approved is not.

## Secrets

Never commit `.env`, keys, tokens or generated state. The CLI redacts detected
secret patterns from the event log, but redaction is not a substitute for
keeping keys out of prompts and transcripts. If a key was pasted into a chat,
rotate it.

## Supported versions

Until 1.0, only the latest workspace version receives fixes.
