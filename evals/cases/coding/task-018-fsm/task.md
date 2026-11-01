# Reject unknown states and events in a state machine

`machine.run(transitions, start, events)` silently ignores an event with no transition and starts from an unknown state. Raise `ValueError` for an unknown start state and for an event that is not defined for the current state, and return the final state for a valid sequence. An empty event list returns the start.

Do not change the tests. Run the test suite to confirm the fix.
