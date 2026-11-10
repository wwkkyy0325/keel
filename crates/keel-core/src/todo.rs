//! Typed task state for a session.
//!
//! The list is kernel-owned so it can be pinned in every context. The model
//! edits it through the `todo` tool; it never owns a free-form plan document.
//! Host-seeded items are immutable and, when locked, the model can only move
//! their status.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Maximum number of items in one list.
pub const TODO_MAX_ITEMS: usize = 100;
/// Maximum characters in one item.
pub const TODO_MAX_TEXT_CHARS: usize = 200;

/// Lifecycle of one task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl TodoStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    fn marker(self) -> &'static str {
        match self {
            Self::Pending => "[ ]",
            Self::InProgress => "[>]",
            Self::Completed => "[x]",
            Self::Failed => "[!]",
        }
    }

    fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

/// Lifecycle of the whole list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TodoState {
    #[default]
    Active,
    Completed,
    Failed,
    Abandoned,
}

impl TodoState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Abandoned => "abandoned",
        }
    }

    pub fn is_closed(self) -> bool {
        self != Self::Active
    }
}

/// Who created the item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TodoSource {
    Host,
    Model,
}

impl TodoSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Model => "model",
        }
    }
}

/// One task with a stable id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: u32,
    pub text: String,
    pub status: TodoStatus,
    pub source: TodoSource,
}

/// One session's task list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoList {
    pub items: Vec<TodoItem>,
    pub revision: u32,
    pub state: TodoState,
    /// A locked list keeps host items fixed; model edits still move statuses.
    pub locked: bool,
    next_id: u32,
}

/// A rejected todo mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TodoError {
    TooManyItems,
    EmptyText,
    TextTooLong,
    NotFound,
    HostItem,
    Locked,
    MultipleInProgress,
    Closed,
    /// The list already closed with a different outcome.
    AlreadyClosed(TodoState),
    NotComplete,
    InvalidState,
    /// A locked host plan only advances the current item.
    OutOfOrder,
}

impl fmt::Display for TodoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyItems => write!(f, "todo list is full"),
            Self::EmptyText => write!(f, "todo text must not be empty"),
            Self::TextTooLong => write!(
                f,
                "todo text must be at most {TODO_MAX_TEXT_CHARS} characters"
            ),
            Self::NotFound => write!(f, "todo item not found"),
            Self::HostItem => write!(f, "host items cannot be removed"),
            Self::Locked => write!(f, "todo list is locked by the host"),
            Self::MultipleInProgress => {
                write!(f, "only one item can be in progress")
            }
            Self::Closed => write!(f, "todo list is closed"),
            Self::AlreadyClosed(state) => {
                write!(f, "todo list is already {}", state.as_str())
            }
            Self::NotComplete => write!(f, "not every item is completed"),
            Self::InvalidState => write!(f, "invalid todo state transition"),
            Self::OutOfOrder => write!(f, "a locked host plan only advances the current item"),
        }
    }
}

impl TodoList {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a fresh list after the previous one closed. The id sequence
    /// stays monotonic, so event consumers never see two items with the same
    /// id. A host lock is left alone: the host owns that decision.
    pub fn reset(&mut self) {
        self.items.clear();
        self.state = TodoState::Active;
        self.revision = self.revision.saturating_add(1);
    }

    /// Adds an immutable host-seeded task.
    pub fn add_host(&mut self, text: &str) -> Result<u32, TodoError> {
        self.push(text, TodoSource::Host)
    }

    /// Adds a model-authored task, unless the host locked the list.
    pub fn add_model(&mut self, text: &str) -> Result<u32, TodoError> {
        if self.locked {
            return Err(TodoError::Locked);
        }
        self.push(text, TodoSource::Model)
    }

    fn push(&mut self, text: &str, source: TodoSource) -> Result<u32, TodoError> {
        if self.state.is_closed() {
            return Err(TodoError::Closed);
        }
        let text = text.trim();
        if text.is_empty() {
            return Err(TodoError::EmptyText);
        }
        if text.chars().count() > TODO_MAX_TEXT_CHARS {
            return Err(TodoError::TextTooLong);
        }
        if self.items.len() >= TODO_MAX_ITEMS {
            return Err(TodoError::TooManyItems);
        }
        self.next_id = self.next_id.saturating_add(1);
        let id = self.next_id;
        self.items.push(TodoItem {
            id,
            text: text.to_owned(),
            status: TodoStatus::Pending,
            source,
        });
        self.revision = self.revision.saturating_add(1);
        Ok(id)
    }

    /// Moves one item's status. Host and model items both allow this.
    pub fn update_status(&mut self, id: u32, status: TodoStatus) -> Result<(), TodoError> {
        if self.state.is_closed() {
            return Err(TodoError::Closed);
        }
        let index = self
            .items
            .iter()
            .position(|item| item.id == id)
            .ok_or(TodoError::NotFound)?;
        if self.locked {
            let current = self.current().map(|item| item.id);
            if current != Some(id) {
                return Err(TodoError::OutOfOrder);
            }
        }
        if status == TodoStatus::InProgress
            && self
                .items
                .iter()
                .any(|item| item.id != id && item.status == TodoStatus::InProgress)
        {
            return Err(TodoError::MultipleInProgress);
        }
        self.items[index].status = status;
        self.revision = self.revision.saturating_add(1);
        self.refresh_state();
        Ok(())
    }

    /// Closes the list explicitly. `Completed` requires every item to be done.
    ///
    /// Closing into the state the list already holds is a no-op: the list
    /// closes itself once every item is terminal, so a model that asks for the
    /// outcome it can already see must not be told it failed.
    pub fn close(&mut self, state: TodoState) -> Result<(), TodoError> {
        if state == TodoState::Active {
            return Err(TodoError::InvalidState);
        }
        if self.state == state {
            return Ok(());
        }
        if self.state.is_closed() {
            return Err(TodoError::AlreadyClosed(self.state));
        }
        if state == TodoState::Completed
            && (self.items.is_empty()
                || self
                    .items
                    .iter()
                    .any(|item| item.status != TodoStatus::Completed))
        {
            return Err(TodoError::NotComplete);
        }
        self.state = state;
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    fn refresh_state(&mut self) {
        if self.items.is_empty() {
            return;
        }
        if self.items.iter().all(|item| item.status.is_terminal()) {
            self.state = if self
                .items
                .iter()
                .any(|item| item.status == TodoStatus::Failed)
            {
                TodoState::Failed
            } else {
                TodoState::Completed
            };
        }
    }

    /// Removes a model-authored item. Host items and locked lists reject it.
    pub fn remove(&mut self, id: u32) -> Result<(), TodoError> {
        if self.state.is_closed() {
            return Err(TodoError::Closed);
        }
        if self.locked {
            return Err(TodoError::Locked);
        }
        let index = self
            .items
            .iter()
            .position(|item| item.id == id)
            .ok_or(TodoError::NotFound)?;
        if self.items[index].source == TodoSource::Host {
            return Err(TodoError::HostItem);
        }
        self.items.remove(index);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Freezes the list after host seeding.
    pub fn lock(&mut self) {
        self.locked = true;
    }

    pub fn in_progress(&self) -> Option<&TodoItem> {
        self.items
            .iter()
            .find(|item| item.status == TodoStatus::InProgress)
    }

    /// The task the model should work on next.
    ///
    /// An explicit `in_progress` wins; otherwise the first pending item is
    /// current. This lets the model mark only completion and saves one tool
    /// round per task.
    pub fn current(&self) -> Option<&TodoItem> {
        if let Some(item) = self.in_progress() {
            return Some(item);
        }
        if self.state.is_closed() {
            return None;
        }
        self.items
            .iter()
            .find(|item| item.status == TodoStatus::Pending)
    }

    pub fn completed(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .count()
    }

    pub fn failed(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.status == TodoStatus::Failed)
            .count()
    }

    /// Renders the pinned task block. Closed lists collapse to one summary
    /// line plus any failed items, so a finished plan stops costing context.
    pub fn render(&self) -> String {
        let total = self.items.len();
        match self.state {
            TodoState::Active => {
                let mut text = String::from("[tasks]\n");
                if let Some(current) = self.current() {
                    text.push_str(&format!("current: #{} {}\n", current.id, current.text));
                }
                for item in &self.items {
                    text.push_str(&format!(
                        "#{} {} {}\n",
                        item.id,
                        item.status.marker(),
                        item.text
                    ));
                }
                text
            }
            TodoState::Completed => {
                format!("[tasks] completed: {}/{total} items\n", self.completed())
            }
            TodoState::Failed => {
                let mut text = format!(
                    "[tasks] failed: completed={} failed={} total={total}\n",
                    self.completed(),
                    self.failed()
                );
                for item in &self.items {
                    if item.status == TodoStatus::Failed {
                        text.push_str(&format!("#{} [!] {}\n", item.id, item.text));
                    }
                }
                text
            }
            TodoState::Abandoned => format!(
                "[tasks] abandoned: completed={} total={total}\n",
                self.completed()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_items_get_ids_and_render() {
        let mut todos = TodoList::new();
        assert_eq!(todos.add_host("chapter 1").unwrap(), 1);
        assert_eq!(todos.add_host("chapter 2").unwrap(), 2);
        assert_eq!(todos.items[0].source, TodoSource::Host);
        assert!(todos.render().contains("#2 [ ] chapter 2"));
        assert_eq!(todos.revision, 2);
    }

    #[test]
    fn only_one_item_can_be_in_progress() {
        let mut todos = TodoList::new();
        let first = todos.add_host("chapter 1").unwrap();
        let second = todos.add_host("chapter 2").unwrap();
        todos.update_status(first, TodoStatus::InProgress).unwrap();
        assert_eq!(
            todos.update_status(second, TodoStatus::InProgress),
            Err(TodoError::MultipleInProgress)
        );
        todos.update_status(first, TodoStatus::Completed).unwrap();
        todos.update_status(second, TodoStatus::InProgress).unwrap();
        assert_eq!(todos.in_progress().map(|item| item.id), Some(second));
    }

    #[test]
    fn a_locked_list_keeps_host_tasks_fixed() {
        let mut todos = TodoList::new();
        let id = todos.add_host("chapter 1").unwrap();
        todos.lock();
        assert_eq!(todos.add_model("gap task"), Err(TodoError::Locked));
        assert_eq!(todos.remove(id), Err(TodoError::Locked));
        todos.update_status(id, TodoStatus::Completed).unwrap();
        assert_eq!(todos.completed(), 1);
    }

    #[test]
    fn a_locked_plan_rejects_a_later_completion() {
        let mut todos = TodoList::new();
        let first = todos.add_host("chapter 1").unwrap();
        let second = todos.add_host("chapter 2").unwrap();
        todos.lock();

        assert_eq!(
            todos.update_status(second, TodoStatus::Completed),
            Err(TodoError::OutOfOrder)
        );
        assert_eq!(todos.current().map(|item| item.id), Some(first));
        todos.update_status(first, TodoStatus::Completed).unwrap();
        assert_eq!(todos.current().map(|item| item.id), Some(second));
        todos.update_status(second, TodoStatus::Completed).unwrap();
        assert_eq!(todos.state, TodoState::Completed);
    }

    #[test]
    fn a_locked_plan_rejects_a_later_in_progress() {
        let mut todos = TodoList::new();
        let first = todos.add_host("chapter 1").unwrap();
        let second = todos.add_host("chapter 2").unwrap();
        todos.lock();

        assert_eq!(
            todos.update_status(second, TodoStatus::InProgress),
            Err(TodoError::OutOfOrder)
        );
        todos.update_status(first, TodoStatus::InProgress).unwrap();
        assert_eq!(todos.current().map(|item| item.id), Some(first));
    }

    #[test]
    fn the_first_pending_item_is_current_until_completed() {
        let mut todos = TodoList::new();
        let first = todos.add_host("one").unwrap();
        let second = todos.add_host("two").unwrap();
        assert_eq!(todos.current().map(|item| item.id), Some(first));
        todos.update_status(first, TodoStatus::Completed).unwrap();
        assert_eq!(todos.current().map(|item| item.id), Some(second));
        assert!(todos.render().contains("current: #2 two"));
    }

    #[test]
    fn text_is_bounded() {
        let mut todos = TodoList::new();
        assert_eq!(todos.add_host(""), Err(TodoError::EmptyText));
        assert_eq!(
            todos.add_host(&"x".repeat(TODO_MAX_TEXT_CHARS + 1)),
            Err(TodoError::TextTooLong)
        );
    }

    #[test]
    fn completing_every_item_closes_the_list() {
        let mut todos = TodoList::new();
        let first = todos.add_host("one").unwrap();
        let second = todos.add_host("two").unwrap();
        todos.update_status(first, TodoStatus::Completed).unwrap();
        assert_eq!(todos.state, TodoState::Active);
        todos.update_status(second, TodoStatus::Completed).unwrap();
        assert_eq!(todos.state, TodoState::Completed);
        assert_eq!(todos.render(), "[tasks] completed: 2/2 items\n");
        assert_eq!(
            todos.update_status(first, TodoStatus::Pending),
            Err(TodoError::Closed)
        );
    }

    #[test]
    fn a_failed_item_closes_as_failed() {
        let mut todos = TodoList::new();
        let first = todos.add_host("one").unwrap();
        let second = todos.add_host("two").unwrap();
        todos.update_status(first, TodoStatus::Failed).unwrap();
        todos.update_status(second, TodoStatus::Completed).unwrap();
        assert_eq!(todos.state, TodoState::Failed);
        assert!(todos.render().contains("[tasks] failed"));
        assert!(todos.render().contains("#1 [!] one"));
    }

    #[test]
    fn close_requires_every_item_completed() {
        let mut todos = TodoList::new();
        todos.add_host("one").unwrap();
        assert_eq!(
            todos.close(TodoState::Completed),
            Err(TodoError::NotComplete)
        );
        todos.close(TodoState::Abandoned).unwrap();
        assert_eq!(todos.state, TodoState::Abandoned);
        assert_eq!(todos.add_host("two"), Err(TodoError::Closed));
    }

    #[test]
    fn closing_into_the_current_outcome_is_a_no_op() {
        let mut todos = TodoList::new();
        let id = todos.add_host("one").unwrap();
        todos.update_status(id, TodoStatus::Completed).unwrap();
        assert_eq!(todos.state, TodoState::Completed);
        assert_eq!(todos.close(TodoState::Completed), Ok(()));
        assert_eq!(
            todos.close(TodoState::Abandoned),
            Err(TodoError::AlreadyClosed(TodoState::Completed))
        );
        assert_eq!(
            TodoError::AlreadyClosed(TodoState::Completed).to_string(),
            "todo list is already completed"
        );
    }

    #[test]
    fn an_explicit_close_is_also_idempotent() {
        let mut todos = TodoList::new();
        todos.add_host("one").unwrap();
        todos.close(TodoState::Abandoned).unwrap();
        assert_eq!(todos.close(TodoState::Abandoned), Ok(()));
        assert_eq!(
            todos.close(TodoState::Failed),
            Err(TodoError::AlreadyClosed(TodoState::Abandoned))
        );
    }
}
