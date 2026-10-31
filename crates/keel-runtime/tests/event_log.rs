use keel_core::{Event, TurnId, TurnState};
use keel_runtime::EventLog;

#[test]
fn append_assigns_monotonic_sequences() {
    let log = EventLog::new();
    assert_eq!(
        log.append(Event::TurnStarted {
            turn_id: TurnId::new(1),
            input: "hi".to_owned(),
        }),
        1
    );
    assert_eq!(
        log.append(Event::TurnFinished {
            turn_id: TurnId::new(1),
            state: TurnState::Done,
            text: "done".to_owned(),
        }),
        2
    );
    assert_eq!(log.len(), 2);
    let snapshot = log.snapshot();
    assert_eq!(snapshot[0].seq, 1);
    assert_eq!(snapshot[1].seq, 2);
    assert_eq!(snapshot[0].event.name(), "turn_started");
}

#[test]
fn entries_can_be_filtered_by_turn() {
    let log = EventLog::new();
    log.append(Event::TurnStarted {
        turn_id: TurnId::new(1),
        input: "one".to_owned(),
    });
    log.append(Event::TurnStarted {
        turn_id: TurnId::new(2),
        input: "two".to_owned(),
    });
    log.append(Event::TurnFinished {
        turn_id: TurnId::new(1),
        state: TurnState::Done,
        text: "done".to_owned(),
    });

    let first = log.for_turn(TurnId::new(1));
    assert_eq!(first.len(), 2);
    assert!(first
        .iter()
        .all(|record| record.event.turn_id() == TurnId::new(1)));
    assert_eq!(log.for_turn(TurnId::new(3)).len(), 0);
}
