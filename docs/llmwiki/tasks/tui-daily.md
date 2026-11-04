# TUI for daily use

The kernel is at a stopping point: 429 tests, 20 curated cases, and every
mechanism exercised by a real model. The reference host already has a
deliberately simplified TUI (`tui.md`). This iteration makes it usable every
day without turning it into a dashboard.

## Goals

- One screen, three zones, nothing else.
- Show only what the user acts on: the conversation, what the agent is doing
  right now, and the one decision that is waiting.
- Two settings at most: permission mode and language. Everything else comes
  from the profile.
- Chinese and English, chosen automatically, not configured per screen.

## Non-goals

- Panels for tasks, jobs, agents or cost. A panel is a second screen and a
  second way to miss something.
- Model picker, theme picker, keybinding editor, layout editor.
- Syntax highlighting, tables, images, mouse.
- A second process for the UI.

## Screen

```
┌────────────────────────────────────────────────────────────┐
│ you  把 tests/stats_test.py 的问题修一下                    │
│                                                            │
│ keel 找到了：均值计算用了整数除法。改完跑测试。              │
│ · read_file  tests/stats.py            ok  0.1s            │
│ · shell      python -m unittest -v     ok  1.9s            │
│                                                            │
│ 两个用例都通过了。                                          │
├────────────────────────────────────────────────────────────┤
│ Idle · tasks 2/2 · 4.1s                        Confirm     │
│ > _                                                        │
└────────────────────────────────────────────────────────────┘
```

The English chrome is the same layout:

```
│ Idle · tasks 2/2 · 4.1s                        Confirm     │
│ > _                                                        │
```

Rules:

1. **No header.** The terminal title carries the product name; a header line
   is a permanent row for nothing.
2. **The transcript is the page.** User messages and final answers are kept.
   While a turn runs, tool lines appear; when it ends, they collapse into one
   aggregate line: tools, files and duration, no token counts.
3. **One status line.** Left: state (`Idle`, `Working`, `Waiting`, `Cancelled`),
   open tasks when a list exists, elapsed time. Right: permission mode, plus
   `Ctrl-C` only while a turn runs. Nothing else; tokens live in `/cost`.
4. **One input line**, which grows for a pasted multi-line message.

## What is shown, and what is not

| Event | On screen | Why |
|---|---|---|
| User message | Always | The conversation |
| Assistant text | Streamed, kept | The answer |
| Tool call | One line: tool, short target, ok/fail, duration | "What is it doing" |
| Tool output | Hidden; `Ctrl-O` expands head and tail while running; the preview stays after | Necessary on failure, noise otherwise |
| Tool failure | One line with the error message | The user must see it |
| Permission | Inline prompt: tool, action, path, reason, four scopes | The one blocking decision |
| Turn continued | Nothing; the status line keeps `Working` | Internal |
| Budget wrap-up / final call | One short status hint, no numbers | Internal |
| Background start | One line: `bg-3 python … started` | So the user knows it runs |
| Background finish | System message with exit code and output path (already) | The result |
| Sub-agent start | The `spawn_agent` tool line, target is the role | "What is it doing" |
| Sub-agent finish | One line: role, state, calls, tokens | The result |
| Context pressure, summaries, facts | Nothing | Internal |
| Reasoning | Nothing | Noise |
| Token counts per call | Nothing; `/cost` prints the turn total | Not actionable |

No new panels. Everything above lands in the transcript or the status line.

## Process details

- While a turn runs, tool lines stream in: one line per call, plus a bounded
  output excerpt for a failure.
- When the turn ends, those lines collapse into one aggregate line under the
  answer. The transcript keeps the user message, the answer and the aggregate.
- `Ctrl-O` or `/details` toggles the process for the whole conversation. The
  toggle only filters hidden blocks; it never re-runs, reorders or drops
  messages.
- A per-turn expansion would need a cursor over blocks, a selection model and
  a second key set. A global toggle is one key, and it answers the question the
  user actually asks ("what did it do"). If it ever proves insufficient, the
  per-turn variant can be added on top; the block model already carries the
  turn boundary.

## Settings

Two, and both have a visible default:

1. **Permission mode** — `Shift+Tab` toggles `Confirm` and `Auto` for the
   session; the status line shows the current one. Never a modal, never a
   config file.
2. **Language** — resolved once at startup:

   ```
   profile [ui] language = "auto" | "zh" | "en"   (default auto)
     → LC_ALL / LC_MESSAGES / LANG contains "zh"
     → OS UI language (sys-locale)
     → "en"
   ```

   There is no `/language` command and no language row on screen. A user whose
   shell locale disagrees with their preference sets one profile key; a user
   with a normal locale never touches it. Tests override with `--lang`.

Everything else (model, tools, permissions, budget) stays in the profile.

## Bilingual rules

- Every user-facing string comes from one table keyed by a `UiText` enum with
  `zh` and `en` arms. No literal chrome text in the renderer.
- The model channel stays English: `[background bg-3 finished]`, tool names,
  event names, paths and commands are never translated. The model answers in
  the user's language on its own.
- CJK is double width: wrapping, cursor movement and scrolling use display
  width, not `chars().count()`. The existing backspace test stays.
- Goldens exist in both languages: `tui-idle.zh.txt`, `tui-idle.en.txt`,
  `tui-prompt.zh.txt`, `tui-prompt.en.txt`.

## Keys

| Key | Action |
|---|---|
| `Enter` | Send |
| `Shift+Enter` | Newline (terminals that report it; `Alt+Enter` fallback) |
| `Ctrl-C` | Cancel the running turn; when idle, clear the input; on an empty input, confirm quit |
| `Ctrl-O` | Show or hide tool output and details (same as `/details`) |
| `Shift+Tab` | Toggle `Confirm` / `Auto` |
| `PageUp` / `PageDown` | Scroll the transcript |

Slash commands stay local and few: `/help`, `/details`, `/cost`, `/clear`,
`/cancel`, `/quit`. `/details` is the discoverable form of `Ctrl-O`. Session
switching stays on the command line (`--store`, `--session-id`).

## States

- **Idle, empty**: one hint line, `> _`.
- **Working**: streamed text, tool lines, status `Working`, `Ctrl-C` on the
  right.
- **Waiting for permission**: the prompt sits inline above the input; the
  status reads `Waiting`; `↑`/`↓` select, `Enter` confirms, letters still work.
- **Finished with open tasks**: one hint line under the answer, input
  prefilled with a continuation (already implemented); no dialog.
- **Cancelled**: the partial answer stays, status `Cancelled` for one turn,
  input usable.
- **Error**: one red line with the message; the session stays open.
- **Terminal cannot run the UI** (piped stdin): exit with a clear message.

## Failure paths

- Resize: the layout recomputes on every event; no stored pixel state.
- Long output: the live preview is bounded by the kernel (4 KB, redacted);
  `Ctrl-O` reads the stored preview, never the file.
- Secrets: the kernel redacts previews and persisted events; the UI never
  prints a tool argument it was not given.
- Terminal restore: raw mode and the alternate screen are restored on quit,
  panic and `Ctrl-C`.
- Width under 40 columns: degrade to a single column, status text truncated
  from the left, never wrapped into the input.

## Implementation steps

- [ ] Text table and language resolution (`UiText`, `Lang`, profile key,
      env, `sys-locale`, `--lang` test flag). Verify: unit tests for the
      resolution order and both arms of every string.
- [ ] Layout simplification: drop the header, one status line, one growing
      input. Verify: goldens `tui-idle.zh.txt` and `tui-idle.en.txt`.
- [ ] Tool line compaction: one line per call with target and duration;
      failure keeps the error; `Ctrl-O` expands. The finished-turn aggregate
      drops the token count. Verify: a running turn renders N lines; a
      finished turn renders the aggregate without tokens.
- [ ] Sub-agent and background lines from the event log
      (`BackgroundStarted`, `SubAgentFinished`), no panels. Verify: both
      events render one line in each language.
- [ ] Status line: state, open tasks, elapsed, mode, and `Ctrl-C` while a
      turn runs. Verify: a turn in each state renders the right left and right
      halves.
- [ ] Permission prompt strings through the table; keys unchanged. Verify:
      goldens `tui-prompt.zh.txt` and `tui-prompt.en.txt`.
- [ ] `/cost` prints the turn total and the session total; `/help` lists the
      six keys and five commands. Verify: the command tests.
- [ ] Manual smoke: raw mode restore, resize, `Ctrl-C`, paste with CJK, 80 and
      40 columns.

## Boundaries

- Host only (`hosts/keel-cli`); the kernel crates never depend on `ratatui`,
  `crossterm`, `sys-locale` or the UI loop.
- No new kernel events. The UI renders what the kernel already emits.
- The line UI (`--input` without `--tui`) keeps its output contract; scripts
  and evals are unaffected.
