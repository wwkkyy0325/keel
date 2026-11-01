def run(transitions, start, events):
    if start not in transitions:
        raise ValueError("unknown start state")
    state = start
    for event in events:
        table = transitions.get(state, {})
        if event not in table:
            raise ValueError("unknown event " + event + " in " + state)
        state = table[event]
    return state
