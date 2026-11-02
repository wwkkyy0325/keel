# Autonomy in one turn, and a credential the user hands over

Two product complaints, one principle: the agent works for the user, not for
the mechanism.

1. A complex task stopped after a few tool calls because the turn budget was
   twelve calls, twelve model calls and three minutes. The user had to type
   "continue" repeatedly to move one step at a time.
2. The user pastes a credential (an app id, a GitHub token, a key) and asks
   the agent to put it in a configuration file. The gate and the secret
   notice treated that as a leak instead of a request.

## Scenarios

| # | Scenario | Expected |
|---|---|---|
| 1 | a coding task needs 20+ tool calls | one turn completes it; no "continue" |
| 2 | the model reports "first item done" while `todo` has pending items | the driver continues the same turn, once per stop |
| 3 | the model keeps stopping without closing the list | at most six continuations, then the turn ends with the list reported |
| 4 | the context is near its budget and the wrap-up was sent | no continuation; the model is allowed to report |
| 5 | the reserved final call is running | no continuation |
| 6 | the user pastes `ghp_…` and asks for it in `.env` | not blocked; the file is written after the gate's confirmation; the value is absent from the event log and the answer |
| 7 | the user pastes an app id that matches no pattern | no notice; ordinary work |

## Steps

- [x] **1. Limits are runaway protection, not a budget.**
      `RuntimeConfig::limits` defaults to 200 model calls, a one-hour turn
      deadline and a five-minute tool timeout; the profile's `tools.budget`
      still narrows tool calls. Coding profiles go from 12 to 120 tool calls.
      Verify: `the_default_limits_fit_a_long_task`; the profile tests still
      load and validate.

- [x] **2. Continue while the task list is open.**
      Before a text-only round ends the turn, the driver checks the shared
      `todo` list. If it is `Active` with pending or in-progress items, it
      records `TurnContinued`, pushes the partial text and a reminder, and
      runs another round. `TurnEvent::Continued` keeps the FSM explicit;
      six continuations cap a model that never closes the list.
      Verify: `a_turn_continues_while_the_task_list_has_unfinished_items`.

- [x] **3. The coding profiles expose `todo`.**
      A model cannot manage a long turn without a plan, and the continuation
      reads its list. `todo` is now visible in the coding profiles.
      Verify: `both_variants_validate_against_the_standard_plugins` and the
      profile tests (the test registry registers the tool).

- [x] **4. A pasted credential is a request, not a leak.**
      The host notice no longer says "rotate it": it says the value was not
      blocked, is redacted from the log and belongs in the named file or
      `.env`. The TUI shows the same notice; the coding prompt tells the agent
      to write the value to the file the user named and not repeat it.
      Verify: `redact_secrets` tests plus the dispatcher test that a `.env`
      write asks first.

## Boundaries

- The continuation is not a new turn: no new user input, no new session
  record, and the same turn deadline applies.
- It never overrides the wrap-up instruction or the final call, and it is
  capped at six. A model that wants to stop must close or abandon the list.
- A secret still enters the model context, because the user typed it. The
  protection is that it does not enter the event log, the answer or the tool
  audit; the README says so instead of implying otherwise.
- The tool budget is still a profile field. The default in the profiles is
  larger; a batch or eval run can set it smaller.
