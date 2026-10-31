use crate::verification::VerificationReport;

/// One artifact the kernel observed this turn change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: String,
}

/// One item the turn left unresolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingItem {
    pub description: String,
}

/// The user-visible result of a turn, edited as the turn progresses.
///
/// The body is the latest text-only assistant round; it is replaced, never
/// concatenated, so narration is not mistaken for the answer. The structured
/// fields accumulate: changes come from tool calls the kernel saw, pending
/// items from deferred work, and verification from the kernel verifier.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Answer {
    /// Number of body revisions in this turn.
    pub revision: u32,
    pub body: String,
    pub changes: Vec<Change>,
    pub pending: Vec<PendingItem>,
    pub verification: Option<VerificationReport>,
    /// Raw wrap-up report text, kept without parsing.
    pub notes: Option<String>,
    /// True when the turn ended without any body text.
    pub missing: bool,
}

impl Answer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the body with a new revision. Empty text is ignored.
    pub fn set_body(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        self.body = text.to_owned();
        self.revision = self.revision.saturating_add(1);
        self.missing = false;
    }

    /// Records a changed artifact once per path.
    pub fn record_change(&mut self, path: impl Into<String>) {
        let path = path.into();
        if self.changes.iter().any(|change| change.path == path) {
            return;
        }
        self.changes.push(Change { path });
    }

    /// Records an unresolved item once per description.
    pub fn record_pending(&mut self, description: impl Into<String>) {
        let description = description.into();
        if self
            .pending
            .iter()
            .any(|item| item.description == description)
        {
            return;
        }
        self.pending.push(PendingItem { description });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_is_replaced_not_concatenated() {
        let mut answer = Answer::new();
        answer.set_body("first");
        answer.set_body("second");

        assert_eq!(answer.body, "second");
        assert_eq!(answer.revision, 2);
    }

    #[test]
    fn empty_text_does_not_create_a_revision() {
        let mut answer = Answer::new();
        answer.set_body("  ");

        assert!(answer.body.is_empty());
        assert_eq!(answer.revision, 0);
        assert!(!answer.missing);
    }

    #[test]
    fn changes_are_recorded_once_per_path() {
        let mut answer = Answer::new();
        answer.record_change("src/lib.rs");
        answer.record_change("src/lib.rs");
        answer.record_change("src/main.rs");

        assert_eq!(answer.changes.len(), 2);
        assert_eq!(answer.changes[0].path, "src/lib.rs");
    }

    #[test]
    fn pending_items_are_recorded_once() {
        let mut answer = Answer::new();
        answer.record_pending("interaction 3 deferred");
        answer.record_pending("interaction 3 deferred");

        assert_eq!(answer.pending.len(), 1);
    }

    #[test]
    fn setting_a_body_clears_the_missing_flag() {
        let mut answer = Answer {
            missing: true,
            ..Answer::default()
        };
        answer.set_body("done");

        assert!(!answer.missing);
    }
}
