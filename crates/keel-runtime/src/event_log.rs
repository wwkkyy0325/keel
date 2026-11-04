use std::sync::{Mutex, MutexGuard};

use keel_core::{redact_secrets, Event, TurnId};

/// One event with its append sequence number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRecord {
    pub seq: u64,
    pub event: Event,
}

/// In-memory event log.
///
/// The kernel only appends while a turn runs. A host that persists the stream
/// may call [`EventLog::clear`] between turns, so a long-lived `--session`
/// process does not keep every turn's events for its whole lifetime.
/// Sequence numbers keep increasing across a clear.
#[derive(Debug, Default)]
pub struct EventLog {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    entries: Vec<EventRecord>,
    next_seq: u64,
}

impl EventLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&self, event: Event) -> u64 {
        let mut inner = self.inner();
        inner.next_seq = inner.next_seq.saturating_add(1);
        let seq = inner.next_seq;
        inner.entries.push(EventRecord {
            seq,
            event: redact_event(event),
        });
        seq
    }

    pub fn len(&self) -> usize {
        self.inner().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner().entries.is_empty()
    }

    pub fn snapshot(&self) -> Vec<EventRecord> {
        self.inner().entries.clone()
    }

    pub fn for_turn(&self, turn_id: TurnId) -> Vec<EventRecord> {
        self.inner()
            .entries
            .iter()
            .filter(|record| record.event.turn_id() == turn_id)
            .cloned()
            .collect()
    }

    /// Drops the records a host has already persisted. The sequence counter
    /// is kept so event numbers stay monotonic within the process.
    pub fn clear(&self) {
        self.inner().entries.clear();
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The event log is an audit record and may be exported. Redact detected
/// secrets before they reach disk; the host warns but does not block input.
impl keel_core::EventSink for EventLog {
    fn emit(&self, event: Event) {
        self.append(event);
    }
}

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
        Event::InterjectionQueued { turn_id, id, text } => Event::InterjectionQueued {
            turn_id,
            id,
            text: redact_secrets(&text),
        },
        Event::InterjectionDelivered { turn_id, id, text } => Event::InterjectionDelivered {
            turn_id,
            id,
            text: redact_secrets(&text),
        },
        Event::InterjectionDropped {
            turn_id,
            id,
            text,
            reason,
        } => Event::InterjectionDropped {
            turn_id,
            id,
            text: redact_secrets(&text),
            reason,
        },
        Event::ModelFailed {
            turn_id,
            provider,
            model,
            code,
            message,
            duration_ms,
        } => Event::ModelFailed {
            turn_id,
            provider,
            model,
            code,
            message: redact_secrets(&message),
            duration_ms,
        },
        other => other,
    }
}
