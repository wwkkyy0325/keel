def run(transitions, start, events):
    state = start
    for event in events:
        state = transitions[state].get(event, state)
    return state
