//! Bounded per-session queue for user messages that arrive mid-turn.
//!
//! The queue is host-facing: a UI pushes text and the driver drains it at a
//! safe point. It is not an interaction and never creates a turn state.

use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// One queued user message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interjection {
    pub id: u64,
    pub text: String,
    pub arrived_ms: u64,
}

/// Result of trying to queue a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterjectionPush {
    Queued(u64),
    /// The same text is already waiting.
    Duplicate,
    /// The bounded queue is full.
    Full,
    /// Whitespace only.
    Empty,
}

pub struct InterjectionQueue {
    inner: Mutex<Inner>,
}

struct Inner {
    capacity: usize,
    next_id: u64,
    items: VecDeque<Interjection>,
    pending: HashSet<String>,
}

impl InterjectionQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                capacity: capacity.max(1),
                next_id: 1,
                items: VecDeque::new(),
                pending: HashSet::new(),
            }),
        }
    }

    pub fn push(&self, text: &str) -> InterjectionPush {
        let text = text.trim();
        if text.is_empty() {
            return InterjectionPush::Empty;
        }
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.pending.contains(text) {
            return InterjectionPush::Duplicate;
        }
        if inner.items.len() >= inner.capacity {
            return InterjectionPush::Full;
        }
        let id = inner.next_id;
        inner.next_id = inner.next_id.saturating_add(1);
        let arrived_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        inner.items.push_back(Interjection {
            id,
            text: text.to_owned(),
            arrived_ms,
        });
        inner.pending.insert(text.to_owned());
        InterjectionPush::Queued(id)
    }

    pub fn drain(&self) -> Vec<Interjection> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.pending.clear();
        inner.items.drain(..).collect()
    }

    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .items
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for InterjectionQueue {
    fn default() -> Self {
        Self::new(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_queue_is_bounded_and_drops_duplicates() {
        let queue = InterjectionQueue::new(2);

        assert_eq!(queue.push("  "), InterjectionPush::Empty);
        assert_eq!(queue.push("first"), InterjectionPush::Queued(1));
        assert_eq!(queue.push("first"), InterjectionPush::Duplicate);
        assert_eq!(queue.push("second"), InterjectionPush::Queued(2));
        assert_eq!(queue.push("third"), InterjectionPush::Full);
        assert_eq!(queue.len(), 2);

        let items = queue.drain();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "first");
        assert_eq!(items[1].text, "second");
        assert!(queue.is_empty());

        // After delivery the same text may be queued again.
        assert_eq!(queue.push("first"), InterjectionPush::Queued(3));
    }
}
