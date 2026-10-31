use std::sync::{Mutex, MutexGuard};

use keel_core::{redact_secrets, Event, TurnId};

/// One event with its append sequence number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRecord {
    pub seq: u64,
    pub event: Event,
}

/// Append-only, in-memory event log.
///
/// There is deliberately no removal API. Appends take a short synchronous
/// lock; telemetry sinks are a later, asynchronous concern.
#[derive(Debug, Default)]
pub struct EventLog {
    entries: Mutex<Vec<EventRecord>>,
}

impl EventLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&self, event: Event) -> u64 {
        let mut entries = self.entries();
        let seq = entries.len() as u64 + 1;
        entries.push(EventRecord {
            seq,
            event: redact_event(event),
        });
        seq
    }

    pub fn len(&self) -> usize {
        self.entries().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries().is_empty()
    }

    pub fn snapshot(&self) -> Vec<EventRecord> {
        self.entries().clone()
    }

    pub fn for_turn(&self, turn_id: TurnId) -> Vec<EventRecord> {
        self.entries()
            .iter()
            .filter(|record| record.event.turn_id() == turn_id)
            .cloned()
            .collect()
    }

    fn entries(&self) -> MutexGuard<'_, Vec<EventRecord>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The event log is an audit record and may be exported. Redact detected
/// secrets before they reach disk; the host still refuses secret-bearing input
/// before the model call.
fn redact_event(event: Event) -> Event {
    match event {
        Event::TurnStarted { turn_id, input } => Event::TurnStarted {
            turn_id,
            input: redact_secrets(&input),
        },
        Event::TurnFinished {
            turn_id,
            state,
            text,
        } => Event::TurnFinished {
            turn_id,
            state,
            text: redact_secrets(&text),
        },
        other => other,
    }
}
