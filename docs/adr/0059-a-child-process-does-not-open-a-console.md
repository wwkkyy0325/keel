# ADR 0059 — A child process does not open a console window

## Status

Accepted.

## Context

Keelson runs the kernel in a Windows GUI process
(`windows_subsystem = "windows"`), which has no console. Every
`std::process::Command` or `tokio::process::Command` that does not ask for
`CREATE_NO_WINDOW` creates a new console window for the child. A turn with a
handful of shell commands, git probes and `ffprobe` calls flashed a black
window for each, and a background command left one open for its whole run.

## Decision

1. `keel-runtime` creates shell and background commands with
   `CREATE_NO_WINDOW` on Windows.
2. The `taskkill` call that kills a process tree sets the same flag, so a
   timeout or cancellation is silent too.
3. Output is still piped and captured; only the window is suppressed.

## Consequences

- A GUI host no longer flashes a console for each command.
- A command that needs an interactive console cannot get one through the
  shell tool. The tool is for non-interactive programs; this was never
  supported.
- Unix behavior is unchanged.

## Alternatives rejected

- Starting the host with a console: the desktop is a GUI app, and the console
  would be permanent.
- Wrapping the command in `cmd /c`: adds a shell layer, changes quoting and
  breaks the no-shell rule.
