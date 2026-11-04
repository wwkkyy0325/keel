//! Minimal terminal UI for the reference host.
//!
//! The state reducer and the renderer are separate from the async loop so the
//! frontend can be tested with `TestBackend` and synthetic key events, without
//! a terminal. The loop only wires channels: model deltas, kernel events,
//! interactions, turns and keys.

use std::cell::Cell;
use std::collections::HashSet;
use std::io;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use keel_core::{
    AccessKind, CancelToken, Event, GrantScope, InteractionKind, InteractionPort,
    InteractionRequest, InteractionResponse, KernelError, PermissionLevel, TodoList, TodoState,
    TodoStatus, TokenUsage, TurnState,
};
use keel_runtime::{EventLog, InterjectionPush, RootScope, RuntimeConfig, TurnDriver, TurnOutcome};

use crate::ui_text::Text;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block as FrameBlock, BorderType, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    Wrap,
};
use ratatui::{Frame, Terminal};
use tokio::sync::{mpsc, oneshot};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::store::Store;
use crate::terminal::parse_response;
use crate::HostModel;

/// The UI mode is session-scoped; the kernel still keeps a TTL so a forgotten
/// full-auto grant expires even if the UI never toggles it back.
const MODE_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Text the model produced for the current turn, forwarded while it streams.
#[derive(Debug, Clone)]
pub enum UiEvent {
    Text(String),
}

/// One pending kernel interaction, routed to the UI loop.
pub struct InteractionJob {
    pub request: InteractionRequest,
    pub reply: oneshot::Sender<InteractionResponse>,
}

/// Interaction port backed by the TUI. The kernel turn waits on a oneshot
/// while the UI shows a modal.
pub struct TuiInteractionPort {
    tx: mpsc::Sender<InteractionJob>,
}

pub fn interaction_pair() -> (TuiInteractionPort, mpsc::Receiver<InteractionJob>) {
    let (tx, rx) = mpsc::channel(8);
    (TuiInteractionPort { tx }, rx)
}

#[async_trait]
impl InteractionPort for TuiInteractionPort {
    async fn request(
        &self,
        request: InteractionRequest,
        cancel: &CancelToken,
    ) -> InteractionResponse {
        if request.kind != InteractionKind::PermissionApproval && request.options.is_empty() {
            return InteractionResponse::Deferred;
        }
        let (reply, answer) = oneshot::channel();
        if self
            .tx
            .send(InteractionJob { request, reply })
            .await
            .is_err()
        {
            return InteractionResponse::Deferred;
        }
        tokio::select! {
            _ = cancel.cancelled() => InteractionResponse::Cancelled,
            result = answer => result.unwrap_or(InteractionResponse::Deferred),
        }
    }
}

/// Everything the UI needs from the host, moved in for the session.
pub struct TuiHost {
    /// Resolved UI language; chrome text comes from `ui_text`.
    pub text: Text,
    pub driver: TurnDriver<HostModel>,
    pub config: RuntimeConfig,
    pub events: Arc<EventLog>,
    pub todos: Option<Arc<Mutex<TodoList>>>,
    pub store: Option<Store>,
    pub session_id: String,
    /// Primary workspace; the status line shows its base name.
    pub workspace: PathBuf,
    /// Every scope, primary first, labeled with its access.
    pub roots: Vec<String>,
    /// Job or `--input` text placed in the prompt; never submitted.
    pub prefill: Option<String>,
    pub interactions: mpsc::Receiver<InteractionJob>,
    pub ui: mpsc::Receiver<UiEvent>,
}

struct TurnDone {
    input: String,
    outcome: TurnOutcome,
    /// Task items still open when the turn ended.
    remaining: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Submit(String),
    Slash(String),
    /// Queue a user message for the running turn.
    Interject(String),
    /// Tab or Shift+Tab: switch between workspace-auto and full-auto.
    ToggleMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BlockKind {
    User,
    /// Text streamed while the turn runs; replaced by `Result` when it ends.
    Assistant,
    Tool {
        name: String,
        ok: Option<bool>,
        duration_ms: u64,
        /// Bounded, redacted call arguments for the `in` line.
        args: String,
    },
    /// The accumulated answer, the only content kept after a turn.
    Result,
    /// Aggregate fields shown under the answer.
    Summary,
    Notice,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Block {
    kind: BlockKind,
    text: String,
}

#[derive(Debug, Default, Clone, Copy)]
struct UsageTotal {
    input: u64,
    output: u64,
    cached: u64,
}

impl UsageTotal {
    fn add(&mut self, usage: &TokenUsage) {
        self.input += u64::from(usage.input_tokens);
        self.output += u64::from(usage.output_tokens);
        self.cached += u64::from(usage.cached_input_tokens);
    }
}

struct App {
    text: Text,
    /// Primary workspace, used to resolve a relative `/add-dir`.
    root: PathBuf,
    /// Base name of the primary workspace, shown in the status line.
    workspace: String,
    /// Scopes beyond the primary one.
    extra_roots: usize,
    permission: String,
    /// Every scope, primary first, labeled with its access.
    roots: Vec<String>,
    blocks: Vec<Block>,
    input: Vec<char>,
    cursor: usize,
    history: Vec<String>,
    history_pos: Option<usize>,
    scroll: usize,
    follow: bool,
    /// Blocks hidden after their turn finished; `Ctrl-O` reveals them.
    hidden: HashSet<usize>,
    show_process: bool,
    content_lines: Cell<usize>,
    viewport_lines: Cell<usize>,
    /// Width of the last draw, so a detail toggle can measure the transcript.
    last_width: Cell<usize>,
    running: bool,
    /// When the running turn started; the status line shows its elapsed time.
    started: Option<Instant>,
    /// Open and total task items, refreshed from the shared list.
    remaining: usize,
    total_tasks: usize,
    cancel: Option<CancelToken>,
    pending: Option<InteractionJob>,
    pending_choice: usize,
    full_auto: bool,
    base_full_auto: bool,
    status: String,
    usage: UsageTotal,
    last_seq: u64,
    streaming: Option<usize>,
    /// First block of the running turn's process; everything from here is
    /// dropped when the turn finishes.
    turn_content_start: Option<usize>,
    turn_tools: u32,
    /// Tool calls the gate refused during the running turn.
    turn_blocked: u32,
    should_quit: bool,
}

impl App {
    fn new(text: Text, root: PathBuf, permission: String, roots: Vec<String>) -> Self {
        let workspace = root
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| root.display().to_string());
        Self {
            text,
            root,
            extra_roots: roots.len().saturating_sub(1),
            workspace,
            permission,
            roots,
            blocks: vec![Block {
                kind: BlockKind::Notice,
                text: text.empty_hint().to_owned(),
            }],
            input: Vec::new(),
            cursor: 0,
            history: Vec::new(),
            history_pos: None,
            scroll: 0,
            follow: true,
            hidden: HashSet::new(),
            show_process: false,
            content_lines: Cell::new(0),
            viewport_lines: Cell::new(1),
            last_width: Cell::new(80),
            running: false,
            started: None,
            remaining: 0,
            total_tasks: 0,
            cancel: None,
            pending: None,
            pending_choice: 0,
            full_auto: false,
            base_full_auto: false,
            status: "ready".to_owned(),
            usage: UsageTotal::default(),
            last_seq: 0,
            streaming: None,
            turn_content_start: None,
            turn_tools: 0,
            turn_blocked: 0,
            should_quit: false,
        }
    }

    /// Places text in the prompt without starting a turn.
    fn prefill(&mut self, text: &str) {
        self.input = text.chars().collect();
        self.cursor = self.input.len();
    }

    /// Records a session scope returned by the permission engine and keeps the
    /// status line's `+N` in sync.
    fn record_root(&mut self, scope: RootScope) {
        let label = format!("{} [{}]", scope.path.display(), scope.access.as_str());
        if !self.roots.contains(&label) {
            self.roots.push(label);
        }
        self.extra_roots = self.roots.len().saturating_sub(1);
        let path = scope.path.display().to_string();
        self.notice(self.text.root_added(&path, scope.access.as_str()));
    }

    fn notice(&mut self, text: impl Into<String>) {
        self.blocks.push(Block {
            kind: BlockKind::Notice,
            text: text.into(),
        });
        self.follow = true;
    }

    fn error(&mut self, text: impl Into<String>) {
        self.blocks.push(Block {
            kind: BlockKind::Error,
            text: text.into(),
        });
        self.follow = true;
    }

    /// Shows or hides the process of finished turns. Revealing scrolls to the
    /// last turn's first hidden block: without that, the view stays at the
    /// bottom and the toggle looks like a no-op.
    fn toggle_details(&mut self) {
        self.show_process = !self.show_process;
        self.follow = true;
        if !self.show_process {
            return;
        }
        let Some(target) = self.last_turn_hidden_start() else {
            return;
        };
        let width = self.last_width.get().max(1);
        let (_, offsets) = self.transcript_at(width);
        if let Some((_, line)) = offsets.iter().find(|(index, _)| *index == target) {
            self.follow = false;
            self.scroll = line.saturating_sub(1);
        }
    }

    /// The first hidden block after the most recent user message.
    fn last_turn_hidden_start(&self) -> Option<usize> {
        let last_user = self
            .blocks
            .iter()
            .rposition(|block| matches!(block.kind, BlockKind::User))?;
        self.hidden
            .iter()
            .copied()
            .filter(|index| *index > last_user)
            .min()
    }

    fn start_turn(&mut self, input: String) {
        self.blocks.push(Block {
            kind: BlockKind::User,
            text: input,
        });
        self.turn_content_start = Some(self.blocks.len());
        self.turn_tools = 0;
        self.turn_blocked = 0;
        self.blocks.push(Block {
            kind: BlockKind::Assistant,
            text: String::new(),
        });
        self.streaming = Some(self.blocks.len() - 1);
        self.running = true;
        self.started = Some(Instant::now());
        self.status = "working".to_owned();
        self.follow = true;
    }

    fn push_delta(&mut self, text: &str) {
        let Some(index) = self.streaming else {
            return;
        };
        if let Some(block) = self.blocks.get_mut(index) {
            block.text.push_str(text);
            self.follow = true;
        }
    }

    fn finish_turn(&mut self, done: &TurnDone) {
        let error = done.outcome.error.as_ref().map(|error| error.to_string());
        self.collapse(
            &done.outcome.answer.body,
            error.as_deref(),
            &done.outcome.usage,
            done.outcome.duration_ms,
            done.outcome.answer.changes.len(),
        );
        self.add_end_notice(
            done.outcome.state,
            done.outcome.answer.missing,
            error.is_some(),
        );
        for text in &done.outcome.late_interjections {
            self.notice(self.text.interjection_late(text));
        }
        if let Some(hint) = continuation_hint(self.text, done.outcome.state, done.remaining) {
            self.notice(hint);
            if self.input.is_empty() {
                self.prefill("Continue the task list");
            }
        }
        if let Some(kind) = done.outcome.secret_warning {
            self.notice(self.text.secret_notice(kind.as_str()));
        }
    }

    /// One line for a turn that did not end in a plain answer. The kernel
    /// already types the state and flags a missing answer; the UI must not
    /// leave the user guessing why the screen is empty.
    fn add_end_notice(&mut self, state: TurnState, missing: bool, has_error: bool) {
        match state {
            TurnState::Cancelled => self.notice(self.text.turn_cancelled()),
            TurnState::Error => {
                if !has_error {
                    self.notice(self.text.turn_failed());
                }
            }
            TurnState::Done | TurnState::Converge => {
                if missing {
                    self.notice(self.text.empty_answer());
                }
            }
            other => self.notice(self.text.turn_ended(&other.to_string())),
        }
    }

    /// Replaces the turn's process with its result: the final answer and one
    /// line of aggregate fields. Tool lines, intermediate text and notices are
    /// dropped; the user message stays.
    fn collapse(
        &mut self,
        answer: &str,
        error: Option<&str>,
        usage: &TokenUsage,
        duration_ms: u64,
        changes: usize,
    ) {
        if let Some(start) = self.turn_content_start.take() {
            for index in start..self.blocks.len() {
                self.hidden.insert(index);
            }
        }
        self.streaming = None;
        let answer = answer.trim();
        if !answer.is_empty() {
            self.blocks.push(Block {
                kind: BlockKind::Result,
                text: answer.to_owned(),
            });
        }
        if let Some(error) = error {
            self.blocks.push(Block {
                kind: BlockKind::Error,
                text: error.to_owned(),
            });
        }
        let mut fields = Vec::new();
        fields.extend(self.text.summary_tools(self.turn_tools as usize));
        fields.extend(self.text.summary_files(changes));
        fields.extend(self.text.summary_blocked(self.turn_blocked as usize));
        fields.push(self.text.summary_tokens(
            u64::from(usage.input_tokens),
            u64::from(usage.output_tokens),
        ));
        if usage.cached_input_tokens > 0 {
            fields.push(
                self.text
                    .summary_cached(u64::from(usage.cached_input_tokens)),
            );
        }
        fields.push(format!("{duration_ms} ms"));
        self.blocks.push(Block {
            kind: BlockKind::Summary,
            text: fields.join(" · "),
        });
        self.turn_tools = 0;
        self.turn_blocked = 0;
        self.usage.add(usage);
        self.running = false;
        self.started = None;
        self.cancel = None;
        self.status = "ready".to_owned();
        self.follow = true;
    }

    fn open_interaction(&mut self, job: InteractionJob) {
        self.pending_choice = job
            .request
            .options
            .iter()
            .position(|option| option.id == "allow_once")
            .unwrap_or(0);
        if let Some(previous) = self.pending.replace(job) {
            let _ = previous.reply.send(InteractionResponse::Deferred);
        }
        self.follow = true;
    }

    fn answer(&mut self, job: InteractionJob, response: InteractionResponse) {
        let _ = job.reply.send(response);
    }

    fn drain_events(&mut self, events: &EventLog) {
        for record in events.snapshot() {
            if record.seq <= self.last_seq {
                continue;
            }
            self.last_seq = record.seq;
            match &record.event {
                Event::ToolStarted {
                    tool, args_preview, ..
                } => {
                    self.turn_tools = self.turn_tools.saturating_add(1);
                    let block = Block {
                        kind: BlockKind::Tool {
                            name: tool.clone(),
                            ok: None,
                            duration_ms: 0,
                            args: args_preview.clone().unwrap_or_default(),
                        },
                        text: String::new(),
                    };
                    match self.streaming {
                        // No text yet: the tool belongs above the answer.
                        Some(index)
                            if self
                                .blocks
                                .get(index)
                                .is_some_and(|block| block.text.is_empty()) =>
                        {
                            self.blocks.insert(index, block);
                            self.streaming = Some(index + 1);
                        }
                        // Text already streamed: keep it above, start a new
                        // assistant block after the tool for what follows.
                        Some(_) => {
                            self.blocks.push(block);
                            self.blocks.push(Block {
                                kind: BlockKind::Assistant,
                                text: String::new(),
                            });
                            self.streaming = Some(self.blocks.len() - 1);
                        }
                        None => self.blocks.push(block),
                    }
                    self.follow = true;
                }
                Event::ToolFinished {
                    tool,
                    ok,
                    duration_ms,
                    output_preview,
                    ..
                } => {
                    if let Some(block) = self.blocks.iter_mut().rev().find(|block| {
                        matches!(&block.kind, BlockKind::Tool { name, ok: None, .. } if name.as_str() == tool.as_str())
                    }) {
                        let args = match &block.kind {
                            BlockKind::Tool { args, .. } => args.clone(),
                            _ => String::new(),
                        };
                        block.kind = BlockKind::Tool {
                            name: tool.clone(),
                            ok: Some(*ok),
                            duration_ms: *duration_ms,
                            args,
                        };
                        block.text = output_preview.clone().unwrap_or_default();
                    }
                }
                Event::PermissionDecided { decision, .. } if decision == "deny" => {
                    self.turn_blocked = self.turn_blocked.saturating_add(1);
                }
                _ => {}
            }
        }
    }

    fn cancel_turn(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
            self.notice(self.text.cancelling());
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Command> {
        if key.kind != KeyEventKind::Press {
            return None;
        }
        // Tab and Shift+Tab switch the session mode, even while a prompt is
        // open; the prompt itself is navigated with the arrow keys.
        if key.code == KeyCode::BackTab {
            return Some(Command::ToggleMode);
        }
        if self.pending.is_some() {
            self.modal_key(key);
            return None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('c') => {
                    if self.running {
                        self.cancel_turn();
                    } else {
                        self.should_quit = true;
                    }
                    None
                }
                KeyCode::Char('d') => {
                    self.should_quit = true;
                    None
                }
                KeyCode::Char('l') => {
                    self.blocks.clear();
                    self.hidden.clear();
                    None
                }
                KeyCode::Char('o') => {
                    self.toggle_details();
                    None
                }
                _ => None,
            };
        }
        match key.code {
            KeyCode::Enter => {
                let text: String = self.input.iter().collect();
                if text.trim().is_empty() {
                    return None;
                }
                self.history.push(text.clone());
                self.history_pos = None;
                self.input.clear();
                self.cursor = 0;
                if text.starts_with('/') {
                    Some(Command::Slash(text))
                } else if self.running {
                    Some(Command::Interject(text))
                } else {
                    Some(Command::Submit(text))
                }
            }
            KeyCode::Char(character) => {
                self.input.insert(self.cursor, character);
                self.cursor += 1;
                None
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.input.remove(self.cursor);
                }
                None
            }
            KeyCode::Delete => {
                if self.cursor < self.input.len() {
                    self.input.remove(self.cursor);
                }
                None
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                None
            }
            KeyCode::Right => {
                self.cursor = (self.cursor + 1).min(self.input.len());
                None
            }
            KeyCode::Home => {
                self.cursor = 0;
                None
            }
            KeyCode::End => {
                self.cursor = self.input.len();
                None
            }
            KeyCode::Up => {
                self.history_up();
                None
            }
            KeyCode::Down => {
                self.history_down();
                None
            }
            KeyCode::PageUp => {
                self.page_up();
                None
            }
            KeyCode::PageDown => {
                self.page_down();
                None
            }
            _ => None,
        }
    }

    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let position = match self.history_pos {
            Some(position) => position.saturating_sub(1),
            None => self.history.len() - 1,
        };
        self.history_pos = Some(position);
        self.input = self.history[position].chars().collect();
        self.cursor = self.input.len();
    }

    fn page_up(&mut self) {
        let max = self
            .content_lines
            .get()
            .saturating_sub(self.viewport_lines.get());
        let current = if self.follow {
            max
        } else {
            self.scroll.min(max)
        };
        self.scroll = current.saturating_sub(10);
        self.follow = false;
    }

    fn page_down(&mut self) {
        let max = self
            .content_lines
            .get()
            .saturating_sub(self.viewport_lines.get());
        let current = if self.follow {
            max
        } else {
            self.scroll.min(max)
        };
        self.scroll = (current + 10).min(max);
        if self.scroll >= max {
            self.follow = true;
        }
    }

    fn history_down(&mut self) {
        let Some(position) = self.history_pos else {
            return;
        };
        if position + 1 >= self.history.len() {
            self.history_pos = None;
            self.input.clear();
            self.cursor = 0;
            return;
        }
        let next = position + 1;
        self.history_pos = Some(next);
        self.input = self.history[next].chars().collect();
        self.cursor = self.input.len();
    }

    fn modal_key(&mut self, key: KeyEvent) {
        let Some(job) = self.pending.take() else {
            return;
        };
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            let _ = job.reply.send(InteractionResponse::Cancelled);
            self.cancel_turn();
            return;
        }
        let count = job.request.options.len();
        match key.code {
            KeyCode::Up if count > 0 => {
                self.pending_choice = (self.pending_choice + count - 1) % count;
                self.pending = Some(job);
                return;
            }
            KeyCode::Down if count > 0 => {
                self.pending_choice = (self.pending_choice + 1) % count;
                self.pending = Some(job);
                return;
            }
            KeyCode::Enter if count > 0 => {
                let option = &job.request.options[self.pending_choice.min(count - 1)];
                let response = if job.request.kind == InteractionKind::PermissionApproval {
                    permission_response(&job.request, &option.id)
                } else {
                    InteractionResponse::Selected {
                        option: option.id.clone(),
                    }
                };
                self.apply_response(job, response);
                return;
            }
            KeyCode::Esc => {
                self.apply_response(job, InteractionResponse::Deferred);
                return;
            }
            _ => {}
        }
        let permission = job.request.kind == InteractionKind::PermissionApproval;
        let response = match key.code {
            KeyCode::Char('d') | KeyCode::Char('1') if permission => {
                Some(parse_response(&job.request, "deny"))
            }
            KeyCode::Char('a') | KeyCode::Char('2') if permission => {
                Some(parse_response(&job.request, "allow_once"))
            }
            KeyCode::Char('s') | KeyCode::Char('3') if permission => {
                Some(parse_response(&job.request, "allow_scope"))
            }
            KeyCode::Char('f') | KeyCode::Char('4') if permission => {
                Some(permission_response(&job.request, "full_auto"))
            }
            KeyCode::Char(character) if !permission => character
                .to_digit(10)
                .and_then(|index| usize::try_from(index).ok())
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| job.request.options.get(index))
                .map(|option| InteractionResponse::Selected {
                    option: option.id.clone(),
                }),
            _ => None,
        };
        match response {
            Some(response) => self.apply_response(job, response),
            None => self.pending = Some(job),
        }
    }

    fn apply_response(&mut self, job: InteractionJob, response: InteractionResponse) {
        if matches!(
            &response,
            InteractionResponse::Approved {
                scope: GrantScope::FullAutoSession { .. }
            }
        ) {
            self.full_auto = true;
            self.permission = "full_auto".to_owned();
        }
        self.answer(job, response);
    }

    fn transcript(&self, width: usize) -> Vec<Line<'static>> {
        self.transcript_at(width).0
    }

    /// The transcript and the first line of every visible block, so a toggle
    /// can scroll to the block it just revealed.
    fn transcript_at(&self, width: usize) -> (Vec<Line<'static>>, Vec<(usize, usize)>) {
        let visible: Vec<(usize, &Block)> = self
            .blocks
            .iter()
            .enumerate()
            .filter(|(index, _)| self.show_process || !self.hidden.contains(index))
            .collect();
        let mut lines = Vec::new();
        let mut offsets = Vec::with_capacity(visible.len());
        for (position, (index, block)) in visible.iter().enumerate() {
            offsets.push((*index, lines.len()));
            match &block.kind {
                BlockKind::Tool {
                    name,
                    ok,
                    duration_ms,
                    args,
                } => {
                    let (glyph, color) = match ok {
                        None => ("…", Color::Yellow),
                        Some(true) => ("✓", Color::DarkGray),
                        Some(false) => ("✗", Color::Red),
                    };
                    let timing = match ok {
                        Some(_) => format!("  {duration_ms} ms"),
                        None => String::new(),
                    };
                    push_wrapped(
                        &mut lines,
                        &format!("  {glyph} {name}{timing}"),
                        Style::default().fg(color),
                        width,
                    );
                    if !args.is_empty() {
                        push_wrapped(
                            &mut lines,
                            &format!("    in   {args}"),
                            Style::default().fg(Color::DarkGray),
                            width,
                        );
                    }
                    if !block.text.is_empty() {
                        let output: Vec<&str> = block.text.lines().collect();
                        let (head, tail) = if self.show_process || output.len() <= EXCERPT_LINES {
                            (output.len(), 0)
                        } else {
                            (EXCERPT_HEAD, EXCERPT_TAIL)
                        };
                        let out_style = Style::default().fg(if *ok == Some(false) {
                            Color::Red
                        } else {
                            Color::DarkGray
                        });
                        for (line_index, line) in output.iter().take(head).enumerate() {
                            let label = if line_index == 0 {
                                "    out  "
                            } else {
                                "         "
                            };
                            push_wrapped(&mut lines, &format!("{label}{line}"), out_style, width);
                        }
                        if tail > 0 {
                            let hidden = output.len() - head - tail;
                            push_wrapped(
                                &mut lines,
                                &format!("         … +{hidden} lines (Ctrl-O)"),
                                Style::default().fg(Color::DarkGray),
                                width,
                            );
                            for line in &output[output.len() - tail..] {
                                push_wrapped(
                                    &mut lines,
                                    &format!("         {line}"),
                                    out_style,
                                    width,
                                );
                            }
                        }
                    }
                }
                _ => {
                    let (prefix, text, style) = match &block.kind {
                        BlockKind::User => {
                            ("› ", block.text.clone(), Style::default().fg(Color::Cyan))
                        }
                        BlockKind::Assistant | BlockKind::Result => {
                            ("", block.text.clone(), Style::default().fg(Color::White))
                        }
                        BlockKind::Summary => (
                            "  ⎿ ",
                            block.text.clone(),
                            Style::default().fg(Color::DarkGray),
                        ),
                        BlockKind::Notice => (
                            "  · ",
                            block.text.clone(),
                            Style::default().fg(Color::Yellow),
                        ),
                        BlockKind::Error => {
                            ("  ! ", block.text.clone(), Style::default().fg(Color::Red))
                        }
                        BlockKind::Tool { .. } => unreachable!(),
                    };
                    if matches!(&block.kind, BlockKind::Assistant | BlockKind::Result) {
                        push_rich(&mut lines, &block.text, style, width);
                    } else {
                        let wrapped = wrap_text(&format!("{prefix}{text}"), width);
                        if matches!(&block.kind, BlockKind::User)
                            && !self.show_process
                            && wrapped.len() > USER_EXCERPT_LINES
                        {
                            for line in wrapped.iter().take(USER_EXCERPT_HEAD) {
                                lines.push(Line::from(Span::styled(line.clone(), style)));
                            }
                            let hidden = wrapped.len() - USER_EXCERPT_HEAD;
                            lines.push(Line::from(Span::styled(
                                format!("  … +{hidden} lines (Ctrl-O)"),
                                Style::default().fg(Color::DarkGray),
                            )));
                        } else {
                            for line in wrapped {
                                lines.push(Line::from(Span::styled(line, style)));
                            }
                        }
                    }
                }
            }
            let next_is_user = visible
                .get(position + 1)
                .is_some_and(|(_, next)| matches!(&next.kind, BlockKind::User));
            if matches!(&block.kind, BlockKind::User) || next_is_user {
                lines.push(Line::from(""));
            }
        }
        (lines, offsets)
    }
}

/// Lines of tool output shown while a turn runs: the head and tail, with the
/// middle elided. `Ctrl-O` shows the whole preview.
const EXCERPT_HEAD: usize = 2;
const EXCERPT_TAIL: usize = 2;
const EXCERPT_LINES: usize = EXCERPT_HEAD + EXCERPT_TAIL + 1;

/// A sent user message longer than this collapses to its head after the turn;
/// `Ctrl-O` shows the whole message.
const USER_EXCERPT_HEAD: usize = 6;
const USER_EXCERPT_LINES: usize = USER_EXCERPT_HEAD + 2;

fn push_wrapped(lines: &mut Vec<Line<'static>>, text: &str, style: Style, width: usize) {
    for line in wrap_text(text, width) {
        lines.push(Line::from(Span::styled(line, style)));
    }
}

/// Renders assistant text with the few markers a terminal can carry cheaply:
/// fenced code blocks become a box with the language on the top border,
/// tables become aligned columns, `inline code` is cyan, `**bold**` is bold,
/// headings are bold, quotes and rules get a rule. Anything else is left as
/// written.
fn push_rich(lines: &mut Vec<Line<'static>>, text: &str, base: Style, width: usize) {
    let all: Vec<&str> = text.lines().collect();
    let mut index = 0;
    let mut code_lang: Option<String> = None;
    while index < all.len() {
        let raw = all[index];
        let trimmed = raw.trim_start();
        if trimmed.starts_with("```") {
            if code_lang.is_some() {
                code_lang = None;
                push_code_border(lines, width, "", false);
            } else {
                let tag = trimmed.trim_start_matches("```").trim().to_owned();
                push_code_border(lines, width, &tag, true);
                code_lang = Some(tag);
            }
            index += 1;
            continue;
        }
        if code_lang.is_some() {
            push_code_line(lines, raw, width);
            index += 1;
            continue;
        }
        if raw.contains('|') && is_table_separator(all.get(index + 1).copied().unwrap_or("")) {
            index = push_table(lines, &all, index, base);
            continue;
        }
        if is_rule(raw) {
            lines.push(Line::from(Span::styled(
                format!("  {}", "─".repeat(width.saturating_sub(2))),
                Style::default().fg(Color::DarkGray),
            )));
            index += 1;
            continue;
        }
        let indent = raw.len() - trimmed.len();
        if let Some(rest) = trimmed.strip_prefix("> ") {
            push_wrapped(
                lines,
                &format!("{}  │ {rest}", " ".repeat(indent)),
                Style::default().fg(Color::DarkGray),
                width,
            );
            index += 1;
            continue;
        }
        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            push_styled(
                lines,
                &format!("{}• {rest}", " ".repeat(indent)),
                base,
                width,
            );
            index += 1;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('#') {
            push_styled(
                lines,
                &format!("{}{}", " ".repeat(indent), rest.trim_start()),
                base.add_modifier(Modifier::BOLD),
                width,
            );
            index += 1;
            continue;
        }
        push_styled(lines, raw, base, width);
        index += 1;
    }
}

fn push_code_border(lines: &mut Vec<Line<'static>>, width: usize, lang: &str, top: bool) {
    let inner = width.saturating_sub(2).max(1);
    let style = Style::default().fg(Color::DarkGray);
    let text = if top {
        if lang.is_empty() {
            format!("╭{}╮", "─".repeat(inner))
        } else {
            let label = format!("─ {lang} ");
            let pad = inner.saturating_sub(label.chars().count());
            format!("╭{label}{}╮", "─".repeat(pad))
        }
    } else {
        format!("╰{}╯", "─".repeat(inner))
    };
    lines.push(Line::from(Span::styled(text, style)));
}

fn push_code_line(lines: &mut Vec<Line<'static>>, raw: &str, width: usize) {
    let content_width = width.saturating_sub(4).max(1);
    let border = Style::default().fg(Color::DarkGray);
    let code = Style::default().fg(Color::White);
    for line in wrap_text(raw, content_width) {
        let pad = content_width.saturating_sub(line.width());
        lines.push(Line::from(vec![
            Span::styled("│ ".to_owned(), border),
            Span::styled(line, code),
            Span::styled(format!("{} │", " ".repeat(pad)), border),
        ]));
    }
}

fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') || !trimmed.ends_with('|') {
        return false;
    }
    let cells: Vec<&str> = trimmed.trim_matches('|').split('|').collect();
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let cell = cell.trim();
            !cell.is_empty() && cell.chars().all(|character| matches!(character, '-' | ':'))
        })
}

fn table_cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_owned())
        .collect()
}

fn push_table(lines: &mut Vec<Line<'static>>, all: &[&str], start: usize, base: Style) -> usize {
    let header = table_cells(all[start]);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut index = start + 2;
    while index < all.len() {
        let line = all[index];
        if !line.contains('|') || line.trim().is_empty() {
            break;
        }
        rows.push(table_cells(line));
        index += 1;
    }
    let columns = header
        .len()
        .max(rows.iter().map(Vec::len).max().unwrap_or(0));
    let mut widths = vec![0usize; columns];
    for row in std::iter::once(&header).chain(rows.iter()) {
        for (column, cell) in row.iter().enumerate() {
            let measured = inline_spans(cell, base)
                .iter()
                .map(|span| span.content.width())
                .sum::<usize>();
            widths[column] = widths[column].max(measured);
        }
    }
    push_table_row(lines, &header, &widths, base.add_modifier(Modifier::BOLD));
    let rule: Vec<String> = widths.iter().map(|width| "─".repeat(*width)).collect();
    lines.push(Line::from(Span::styled(
        format!("  {}", rule.join("  ")),
        Style::default().fg(Color::DarkGray),
    )));
    for row in &rows {
        push_table_row(lines, row, &widths, base);
    }
    index
}

fn push_table_row(lines: &mut Vec<Line<'static>>, row: &[String], widths: &[usize], style: Style) {
    let mut spans = vec![Span::raw("  ")];
    for (column, width) in widths.iter().enumerate() {
        if column > 0 {
            spans.push(Span::raw("  "));
        }
        let cell = row.get(column).map(String::as_str).unwrap_or("");
        let mut cell_spans = inline_spans(cell, style);
        let measured: usize = cell_spans.iter().map(|span| span.content.width()).sum();
        cell_spans.push(Span::raw(" ".repeat(width.saturating_sub(measured))));
        spans.extend(cell_spans);
    }
    lines.push(Line::from(spans));
}

fn is_rule(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.chars().count() < 3 {
        return false;
    }
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    matches!(first, '-' | '*' | '_') && trimmed.chars().all(|character| character == first)
}

fn push_styled(lines: &mut Vec<Line<'static>>, text: &str, style: Style, width: usize) {
    let spans = inline_spans(text, style);
    let measured: usize = spans
        .iter()
        .map(|span| {
            span.content
                .chars()
                .map(|character| character.width().unwrap_or(0))
                .sum::<usize>()
        })
        .sum();
    if measured <= width {
        lines.push(Line::from(spans));
        return;
    }
    let plain: String = spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    for line in wrap_text(&plain, width) {
        lines.push(Line::from(Span::styled(line, style)));
    }
}

fn inline_spans(text: &str, base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, segment) in text.split('`').enumerate() {
        let code = index % 2 == 1;
        for (part_index, part) in segment.split("**").enumerate() {
            if part.is_empty() {
                continue;
            }
            let mut style = base;
            if code {
                style = style.fg(Color::Cyan);
            }
            if part_index % 2 == 1 {
                style = style.add_modifier(Modifier::BOLD);
            }
            spans.push(Span::styled(part.to_owned(), style));
        }
    }
    spans
}

/// Wrapped input lines and the line the cursor sits on.
fn input_view(app: &App, width: usize) -> (Vec<String>, usize) {
    let cursor = app.cursor.min(app.input.len());
    let before: String = app.input[..cursor].iter().collect();
    let after: String = app.input[cursor..].iter().collect();
    let lines = wrap_text(&format!("> {before}▏{after}"), width);
    let cursor_line = wrap_text(&format!("> {before}▏"), width)
        .len()
        .saturating_sub(1)
        .min(lines.len().saturating_sub(1));
    (lines, cursor_line)
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for raw in text.split('\n') {
        let mut line = String::new();
        let mut line_width = 0usize;
        for character in raw.chars() {
            let character_width = character.width().unwrap_or(0);
            if line_width + character_width > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                line_width = 0;
            }
            line.push(character);
            line_width += character_width;
        }
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self, KernelError> {
        enable_raw_mode().map_err(io_error)?;
        execute!(io::stdout(), EnterAlternateScreen).map_err(io_error)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

fn io_error(error: io::Error) -> KernelError {
    KernelError::Io {
        message: error.to_string(),
    }
}

fn spawn_key_reader(tx: mpsc::Sender<CrosstermEvent>, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match crossterm::event::poll(Duration::from_millis(100)) {
                Ok(true) => match crossterm::event::read() {
                    Ok(event) => {
                        if tx.blocking_send(event).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                },
                Ok(false) => {}
                Err(_) => break,
            }
        }
    });
}

/// Runs the interactive session. `driver` is shared with a turn task; the UI
/// owns the terminal, the store and the transcript.
pub async fn run(host: TuiHost) -> Result<ExitCode, KernelError> {
    if !io::stdin().is_terminal() {
        return Err(KernelError::InvalidConfig {
            message: "`--tui` needs an interactive terminal".to_owned(),
        });
    }
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout())).map_err(io_error)?;
    let mut app = App::new(
        host.text,
        host.workspace.clone(),
        host.config.permission_level.as_str().to_owned(),
        host.roots.clone(),
    );
    app.base_full_auto = host.config.permission_level == PermissionLevel::FullAuto;
    app.full_auto = app.base_full_auto;
    if let Some(prefill) = host.prefill {
        app.prefill(&prefill);
    }
    let driver = Arc::new(host.driver);
    let (key_tx, mut key_rx) = mpsc::channel(64);
    let stop = Arc::new(AtomicBool::new(false));
    spawn_key_reader(key_tx, stop.clone());
    let (turn_tx, mut turn_rx) = mpsc::channel::<TurnDone>(4);
    let mut interactions = host.interactions;
    let mut ui = host.ui;
    let mut store = host.store;
    let todos = host.todos.clone();
    let mut ticks = tokio::time::interval(Duration::from_millis(80));

    while !app.should_quit {
        let (open, total) = task_counts(todos.as_ref());
        app.remaining = open;
        app.total_tasks = total;
        terminal.draw(|frame| draw(&app, frame)).map_err(io_error)?;
        tokio::select! {
            Some(key) = key_rx.recv() => {
                let CrosstermEvent::Key(key) = key else { continue };
                match app.handle_key(key) {

                    Some(Command::Submit(input)) => start_turn(
                        &mut app,
                        driver.clone(),
                        turn_tx.clone(),
                        todos.clone(),
                        input,
                    ),
                    Some(Command::Slash(command)) => {
                        run_slash(&mut app, &command, store.as_ref(), Some(&driver))
                    }
                    Some(Command::Interject(text)) => match driver.interject(&text) {
                        InterjectionPush::Queued(_) => app.notice(app.text.interjection_queued()),
                        InterjectionPush::Duplicate => {
                            app.notice(app.text.interjection_duplicate())
                        }
                        InterjectionPush::Full => app.notice(app.text.interjection_full()),
                        InterjectionPush::Empty => {}
                    },
                    Some(Command::ToggleMode) => toggle_mode(&mut app, &driver),
                    None => {}
                }
            }
            Some(job) = interactions.recv() => app.open_interaction(job),
            Some(event) = ui.recv() => {
                let UiEvent::Text(text) = event;
                app.push_delta(&text);
            }
            Some(done) = turn_rx.recv() => {
                app.drain_events(&host.events);
                app.finish_turn(&done);
                if let Some(store) = store.as_mut() {
                    crate::save_turn(
                        store,
                        &host.session_id,
                        &driver,
                        host.todos.as_ref(),
                        &done.input,
                        &done.outcome,
                        &host.events,
                    )?;
                }
                host.events.clear();
            }
            _ = ticks.tick() => app.drain_events(&host.events),
        }
    }
    stop.store(true, Ordering::Relaxed);
    Ok(ExitCode::SUCCESS)
}

/// The UI mode lasts for the session, so its full-auto option uses the same
/// long TTL as the Tab switch; other scopes keep the line UI's defaults.
fn permission_response(request: &InteractionRequest, id: &str) -> InteractionResponse {
    if id == "full_auto" {
        InteractionResponse::Approved {
            scope: GrantScope::FullAutoSession { ttl: MODE_TTL },
        }
    } else {
        parse_response(request, id)
    }
}

fn toggle_mode(app: &mut App, driver: &TurnDriver<HostModel>) {
    if app.base_full_auto {
        return;
    }
    if app.full_auto {
        driver.revoke_permission_grants();
        app.full_auto = false;
        app.permission = "workspace_auto".to_owned();
    } else {
        driver.escalate_full_auto(MODE_TTL);
        app.full_auto = true;
        app.permission = "full_auto".to_owned();
    }
}

fn start_turn(
    app: &mut App,
    driver: Arc<TurnDriver<HostModel>>,
    tx: mpsc::Sender<TurnDone>,
    todos: Option<Arc<Mutex<TodoList>>>,
    input: String,
) {
    let cancel = CancelToken::new();
    app.cancel = Some(cancel.clone());
    app.start_turn(input.clone());
    tokio::spawn(async move {
        let outcome = driver.run(&input, cancel).await;
        let remaining = open_tasks(todos.as_ref());
        let _ = tx
            .send(TurnDone {
                input,
                outcome,
                remaining,
            })
            .await;
    });
}

/// Open and total task items for the status line.
fn task_counts(todos: Option<&Arc<Mutex<TodoList>>>) -> (usize, usize) {
    let Some(handle) = todos else { return (0, 0) };
    let list = handle
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let total = list.items.len();
    let open = list
        .items
        .iter()
        .filter(|item| matches!(item.status, TodoStatus::Pending | TodoStatus::InProgress))
        .count();
    (open, total)
}

/// Task items the model has not closed, used only to offer the next turn.
fn open_tasks(todos: Option<&Arc<Mutex<TodoList>>>) -> usize {
    let Some(handle) = todos else { return 0 };
    let list = handle
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if list.state != TodoState::Active {
        return 0;
    }
    list.items
        .iter()
        .filter(|item| matches!(item.status, TodoStatus::Pending | TodoStatus::InProgress))
        .count()
}

/// The hint shown when a finished turn left work on the list.
fn continuation_hint(text: Text, state: TurnState, remaining: usize) -> Option<String> {
    if remaining == 0 {
        return None;
    }
    matches!(state, TurnState::Done | TurnState::Converge)
        .then(|| text.continuation_hint(remaining))
}

/// `/add-dir <path> [ro|rw]`; a relative path is resolved against the
/// workspace, not the process working directory.
fn parse_add_dir(command: &str) -> Option<(String, AccessKind)> {
    let rest = command.trim().strip_prefix("/add-dir")?.trim();
    if rest.is_empty() {
        return None;
    }
    let (path, access) = match rest.rsplit_once(' ') {
        Some((path, "ro")) => (path.trim(), AccessKind::Read),
        Some((path, "rw")) => (path.trim(), AccessKind::Write),
        _ => (rest, AccessKind::Write),
    };
    (!path.is_empty()).then(|| (path.to_owned(), access))
}

fn add_dir(app: &mut App, driver: &TurnDriver<HostModel>, command: &str) {
    let Some((path, access)) = parse_add_dir(command) else {
        app.notice(app.text.add_dir_usage());
        return;
    };
    let requested = PathBuf::from(&path);
    let resolved = if requested.is_absolute() {
        requested
    } else {
        app.root.join(requested)
    };
    match driver.add_root(&resolved, access) {
        Ok(scope) => app.record_root(scope),
        Err(error) => app.error(app.text.add_dir_failed(&error.to_string())),
    }
}

fn run_slash(
    app: &mut App,
    command: &str,
    store: Option<&Store>,
    driver: Option<&TurnDriver<HostModel>>,
) {
    match command.trim() {
        "/help" => app.notice(app.text.help()),
        "/details" => app.toggle_details(),
        cmd if cmd == "/add-dir" || cmd.starts_with("/add-dir ") => match driver {
            Some(driver) => add_dir(app, driver, cmd),
            None => app.notice(app.text.add_dir_unavailable()),
        },
        "/quit" | "/exit" => app.should_quit = true,
        "/clear" => app.blocks.clear(),
        "/cost" => app.notice(
            app.text
                .cost(app.usage.input, app.usage.output, app.usage.cached),
        ),
        "/cancel" => app.cancel_turn(),
        "/roots" => {
            if app.roots.is_empty() {
                app.notice(app.text.no_roots());
            } else {
                let roots = app.roots.clone();
                for root in roots {
                    app.notice(format!("root {root}"));
                }
            }
        }
        "/sessions" => match store {
            Some(store) => match crate::view::sessions(store.connection()) {
                Ok(sessions) if sessions.is_empty() => app.notice(app.text.no_sessions()),
                Ok(sessions) => {
                    for session in sessions {
                        app.notice(app.text.session_line(
                            &session.id,
                            session.turns,
                            &session.last_turn_state.unwrap_or_else(|| "-".to_owned()),
                        ));
                    }
                }
                Err(error) => app.error(format!("cannot list sessions: {error}")),
            },
            None => app.notice(app.text.no_store()),
        },
        other => app.notice(format!("unknown command `{other}`; try /help")),
    }
}

/// Content lines shown in the input box before it starts scrolling.
const INPUT_MAX_LINES: usize = 6;

fn draw(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let prompt_height = app.pending.as_ref().map_or(0, |job| {
        let text_width = area.width.saturating_sub(2).max(1) as usize;
        let prompt_lines = wrap_text(&job.request.prompt, text_width).len().max(1);
        (prompt_lines + job.request.options.len() + 3).min(14) as u16
    });
    let input_width = area.width.saturating_sub(2).max(1) as usize;
    let (input_lines, cursor_line) = input_view(app, input_width);
    let visible_input = input_lines.len().min(INPUT_MAX_LINES);
    let input_start = if input_lines.len() > visible_input {
        cursor_line
            .saturating_sub(visible_input.saturating_sub(1))
            .min(input_lines.len() - visible_input)
    } else {
        0
    };
    let input_height = ((visible_input + 2) as u16)
        .min(area.height.saturating_sub(prompt_height).saturating_sub(4))
        .max(3);
    let chunks = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(prompt_height),
        Constraint::Length(input_height),
        Constraint::Length(1),
    ])
    .split(area);

    app.last_width.set(chunks[0].width as usize);
    let lines = app.transcript(chunks[0].width as usize);
    let content = lines.len();
    let visible = chunks[0].height as usize;
    app.content_lines.set(content);
    app.viewport_lines.set(visible);
    let offset = if app.follow {
        content.saturating_sub(visible)
    } else {
        app.scroll.min(content.saturating_sub(visible))
    };
    frame.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), chunks[0]);
    if content > visible {
        let mut state = ScrollbarState::new(content).position(offset);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            chunks[0],
            &mut state,
        );
    }

    if let Some(job) = &app.pending {
        draw_prompt(job, app.pending_choice, frame, chunks[1], app.text);
    }

    let input_text = input_lines[input_start..input_start + visible_input].join("\n");
    frame.render_widget(
        Paragraph::new(input_text).block(FrameBlock::bordered().border_type(BorderType::Rounded)),
        chunks[2],
    );

    let state = if app.running {
        app.text.status_working()
    } else {
        app.text.status_ready()
    };
    let mut left = format!(" ● {state}");
    left.push_str(" · ");
    left.push_str(&app.workspace);
    if app.extra_roots > 0 {
        left.push_str(&format!(" +{}", app.extra_roots));
    }
    if app.remaining > 0 && app.total_tasks > 0 {
        left.push_str(" · ");
        left.push_str(&app.text.task_status(app.remaining, app.total_tasks));
    }
    if let Some(started) = app.started {
        left.push_str(&format!(" · {}s", started.elapsed().as_secs()));
    }
    let mut right = app.text.mode_label(&app.permission);
    if app.running {
        right.push_str(" · Ctrl-C");
    } else if !app.hidden.is_empty() {
        right.push_str(" · Ctrl-O · Shift+Tab");
    } else {
        right.push_str(" · Shift+Tab");
    }
    let footer = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(chunks[3]);
    let status_style = Style::default().fg(if app.running {
        Color::Yellow
    } else {
        Color::Green
    });
    frame.render_widget(Paragraph::new(left).style(status_style), footer[0]);
    frame.render_widget(
        Paragraph::new(right)
            .alignment(Alignment::Right)
            .style(Style::default().fg(Color::DarkGray)),
        footer[1],
    );
}

/// Inline approval above the input, so the conversation does not disappear
/// behind a modal.
fn draw_prompt(job: &InteractionJob, selected: usize, frame: &mut Frame, area: Rect, text: Text) {
    let title = if job.request.kind == InteractionKind::PermissionApproval {
        text.permission_title().to_owned()
    } else {
        format!(" {} ", job.request.kind.as_str())
    };
    let mut lines = Vec::new();
    for line in job.request.prompt.lines() {
        lines.push(Line::from(line.to_owned()));
    }
    for (index, option) in job.request.options.iter().enumerate() {
        let active = index == selected;
        let style = if active {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let marker = if active { "› " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(marker.to_owned(), style),
            Span::styled(text.permission_label(&option.id, &option.label), style),
        ]));
    }
    lines.push(Line::from(text.permission_hint()));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(Color::Yellow))
            .block(FrameBlock::bordered().title(title)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use keel_core::{InteractionOption, TurnId};
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(character: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
    }

    fn app() -> App {
        App::new(
            Text::new(crate::ui_text::Lang::En),
            PathBuf::from("C:/keel"),
            "workspace_auto".to_owned(),
            Vec::new(),
        )
    }

    fn frame(app: &App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(app, frame)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn golden(name: &str, actual: &str) {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name);
        if !path.exists() {
            std::fs::write(&path, format!("{actual}\n")).unwrap();
        }
        let expected = std::fs::read_to_string(&path).unwrap();
        assert_eq!(format!("{actual}\n"), expected);
    }

    #[test]
    fn typing_and_deleting_keeps_char_boundaries() {
        let mut app = app();
        for character in "中文a".chars() {
            app.handle_key(key(KeyCode::Char(character)));
        }
        assert_eq!(app.input.iter().collect::<String>(), "中文a");

        app.handle_key(key(KeyCode::Backspace));
        assert_eq!(app.input.iter().collect::<String>(), "中文");
        app.handle_key(key(KeyCode::Backspace));
        assert_eq!(app.input.iter().collect::<String>(), "中");
        app.handle_key(key(KeyCode::Delete));
        assert_eq!(app.input.iter().collect::<String>(), "中");
    }

    #[test]
    fn enter_submits_and_slash_stays_local() {
        let mut app = app();
        for character in "hello".chars() {
            app.handle_key(key(KeyCode::Char(character)));
        }
        assert_eq!(
            app.handle_key(key(KeyCode::Enter)),
            Some(Command::Submit("hello".to_owned()))
        );
        assert!(app.input.is_empty());

        for character in "/help".chars() {
            app.handle_key(key(KeyCode::Char(character)));
        }
        assert_eq!(
            app.handle_key(key(KeyCode::Enter)),
            Some(Command::Slash("/help".to_owned()))
        );
    }

    #[test]
    fn slash_roots_lists_every_scope() {
        let mut app = App::new(
            Text::new(crate::ui_text::Lang::En),
            PathBuf::from("C:/ws"),
            "workspace_auto".to_owned(),
            vec!["C:/ws [write]".to_owned(), "D:/Archive [read]".to_owned()],
        );

        run_slash(&mut app, "/roots", None, None);

        let notices: Vec<String> = app
            .blocks
            .iter()
            .filter(|block| matches!(&block.kind, BlockKind::Notice))
            .map(|block| block.text.clone())
            .collect();
        assert!(notices.contains(&"root C:/ws [write]".to_owned()));
        assert!(notices.contains(&"root D:/Archive [read]".to_owned()));
    }

    #[test]
    fn history_walks_back_and_forth() {
        let mut app = app();
        for text in ["one", "two"] {
            app.input = text.chars().collect();
            app.cursor = app.input.len();
            app.handle_key(key(KeyCode::Enter));
        }
        app.handle_key(key(KeyCode::Up));
        assert_eq!(app.input.iter().collect::<String>(), "two");
        app.handle_key(key(KeyCode::Up));
        assert_eq!(app.input.iter().collect::<String>(), "one");
        app.handle_key(key(KeyCode::Down));
        assert_eq!(app.input.iter().collect::<String>(), "two");
        app.handle_key(key(KeyCode::Down));
        assert!(app.input.is_empty());
    }

    #[test]
    fn ctrl_c_cancels_a_turn_and_quits_when_idle() {
        let mut app = app();
        app.running = true;
        app.cancel = Some(CancelToken::new());
        app.handle_key(ctrl('c'));
        assert!(app.cancel.as_ref().unwrap().is_cancelled());
        assert!(!app.should_quit);

        app.running = false;
        app.handle_key(ctrl('c'));
        assert!(app.should_quit);
    }

    #[test]
    fn deltas_stream_into_the_assistant_block() {
        let mut app = app();
        app.start_turn("hello".to_owned());
        app.push_delta("wor");
        app.push_delta("ld");

        assert_eq!(app.blocks.last().unwrap().text, "world");
    }

    #[test]
    fn enter_while_running_queues_an_interjection() {
        let mut app = app();
        app.running = true;
        app.input = "stay in the workspace".chars().collect();
        app.cursor = app.input.len();

        assert_eq!(
            app.handle_key(key(KeyCode::Enter)),
            Some(Command::Interject("stay in the workspace".to_owned()))
        );
        assert!(app.input.is_empty());
        assert_eq!(
            app.history.last().map(String::as_str),
            Some("stay in the workspace")
        );
    }

    #[test]
    fn a_slash_command_still_works_while_a_turn_runs() {
        let mut app = app();
        app.running = true;
        app.input = "/cancel".chars().collect();
        app.cursor = app.input.len();

        assert_eq!(
            app.handle_key(key(KeyCode::Enter)),
            Some(Command::Slash("/cancel".to_owned()))
        );
    }

    #[test]
    fn prefill_fills_the_prompt_without_starting_a_turn() {
        let mut app = app();
        let blocks = app.blocks.len();

        app.prefill("tidy the downloads");

        assert_eq!(app.input.iter().collect::<String>(), "tidy the downloads");
        assert_eq!(app.cursor, app.input.len());
        assert_eq!(app.blocks.len(), blocks);
        assert!(!app.running);
    }

    #[test]
    fn a_prefilled_prompt_is_visible_in_the_input_box() {
        let mut app = app();
        app.prefill("tidy the downloads");

        let rendered = frame(&app, 80, 24);

        assert!(rendered.contains("tidy the downloads"), "{rendered}");
        assert!(!rendered.contains("⏺ shell"), "{rendered}");
    }

    #[test]
    fn tool_events_become_one_line_blocks() {
        let mut app = app();
        let events = EventLog::new();
        events.append(Event::ToolStarted {
            turn_id: TurnId::new(1),
            call_id: keel_core::ToolCallId::new(1),
            tool: "shell".to_owned(),
            risk: keel_core::RiskLevel::Dangerous,
            args_preview: Some("program=python".to_owned()),
        });
        events.append(Event::ToolFinished {
            turn_id: TurnId::new(1),
            call_id: keel_core::ToolCallId::new(1),
            tool: "shell".to_owned(),
            ok: true,
            error: None,
            duration_ms: 12,
            output_preview: Some("all tests passed".to_owned()),
        });

        app.drain_events(&events);

        let block = app.blocks.last().unwrap();
        assert_eq!(
            block.kind,
            BlockKind::Tool {
                name: "shell".to_owned(),
                ok: Some(true),
                duration_ms: 12,
                args: "program=python".to_owned(),
            }
        );
        assert_eq!(block.text, "all tests passed");
    }

    #[test]
    fn a_running_tool_shows_a_short_output_excerpt() {
        let mut app = app();
        app.start_turn("run the tests".to_owned());
        app.blocks.push(Block {
            kind: BlockKind::Tool {
                name: "shell".to_owned(),
                ok: Some(false),
                duration_ms: 5,
                args: String::new(),
            },
            text: "one\ntwo\nthree\nfour\nfive\nsix".to_owned(),
        });

        let running = transcript_text(&app, 80);
        assert!(running.contains("one"), "{running}");
        assert!(running.contains("six"), "{running}");
        assert!(!running.contains("three"), "{running}");
        assert!(running.contains("+2 lines"), "{running}");

        app.show_process = true;
        let expanded = transcript_text(&app, 80);
        assert!(expanded.contains("three"), "{expanded}");
        assert!(!expanded.contains("+2 lines"), "{expanded}");
    }

    fn usage() -> TokenUsage {
        TokenUsage {
            input_tokens: 15,
            output_tokens: 5,
            cached_input_tokens: 0,
        }
    }

    #[test]
    fn a_finished_turn_hides_its_process() {
        let mut app = app();
        app.start_turn("say hello".to_owned());
        app.push_delta("partial");
        app.blocks.push(Block {
            kind: BlockKind::Tool {
                name: "echo".to_owned(),
                ok: Some(true),
                duration_ms: 1,
                args: String::new(),
            },
            text: "command output".to_owned(),
        });
        app.turn_tools = 1;

        app.collapse("final answer", None, &usage(), 12, 1);

        assert!(app
            .blocks
            .iter()
            .any(|block| matches!(&block.kind, BlockKind::Result) && block.text == "final answer"));
        assert!(app.blocks.iter().any(
            |block| matches!(&block.kind, BlockKind::Summary) && block.text.contains("1 tool")
        ));
        let collapsed = transcript_text(&app, 80);
        assert!(!collapsed.contains("partial"), "{collapsed}");
        assert!(!collapsed.contains("echo"), "{collapsed}");
        assert!(!collapsed.contains("command output"), "{collapsed}");

        app.show_process = true;
        let expanded = transcript_text(&app, 80);
        assert!(expanded.contains("partial"), "{expanded}");
        assert!(expanded.contains("echo"), "{expanded}");
        assert!(expanded.contains("command output"), "{expanded}");
    }

    #[test]
    fn a_blocked_tool_is_counted_in_the_summary() {
        let mut app = app();
        let events = EventLog::new();
        app.start_turn("edit the config".to_owned());
        events.append(Event::PermissionDecided {
            turn_id: TurnId::new(1),
            call_id: keel_core::ToolCallId::new(1),
            tool: "read_file".to_owned(),
            access: keel_core::AccessKind::Read,
            path: Some(".env".to_owned()),
            decision: "deny".to_owned(),
            reason: Some("the path matches a secret pattern".to_owned()),
        });
        app.drain_events(&events);

        app.collapse("done", None, &usage(), 10, 0);

        let summary = app
            .blocks
            .iter()
            .find_map(|block| match &block.kind {
                BlockKind::Summary => Some(block.text.as_str()),
                _ => None,
            })
            .expect("a summary line");
        assert!(summary.contains("1 blocked"), "{summary}");
    }

    fn transcript_text(app: &App, width: usize) -> String {
        app.transcript(width)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn page_up_starts_at_the_bottom_and_page_down_returns_to_it() {
        let mut app = app();
        app.content_lines.set(100);
        app.viewport_lines.set(20);

        app.page_up();
        assert_eq!(app.scroll, 70);
        assert!(!app.follow);

        app.page_up();
        assert_eq!(app.scroll, 60);

        app.page_down();
        assert_eq!(app.scroll, 70);
        assert!(!app.follow);

        app.page_down();
        assert_eq!(app.scroll, 80);
        assert!(app.follow);
    }

    fn permission_job(prompt: &str) -> (InteractionJob, oneshot::Receiver<InteractionResponse>) {
        let (reply, answer) = oneshot::channel();
        let job = InteractionJob {
            request: InteractionRequest::new(
                TurnId::new(1),
                InteractionKind::PermissionApproval,
                prompt,
            )
            .with_options(vec![
                InteractionOption::new("deny", "Deny"),
                InteractionOption::new("allow_once", "Allow once"),
                InteractionOption::new("allow_scope", "Allow this path"),
                InteractionOption::new("full_auto", "Full auto"),
            ]),
            reply,
        };
        (job, answer)
    }

    #[test]
    fn permission_keys_map_to_the_same_scopes_as_the_line_ui() {
        let mut app = app();
        let (job, mut answer) = permission_job("Allow shell?");
        app.open_interaction(job);

        app.handle_key(key(KeyCode::Char('a')));

        assert!(app.pending.is_none());
        assert!(matches!(
            answer.try_recv(),
            Ok(InteractionResponse::Approved {
                scope: GrantScope::Once
            })
        ));
    }

    #[test]
    fn arrows_select_a_scope_and_enter_confirms_it() {
        let mut app = app();
        let (job, mut answer) = permission_job("Allow shell?");
        app.open_interaction(job);
        assert_eq!(app.pending_choice, 1, "allow once is the default");

        app.handle_key(key(KeyCode::Down));
        app.handle_key(key(KeyCode::Enter));

        assert!(app.pending.is_none());
        assert!(matches!(
            answer.try_recv(),
            Ok(InteractionResponse::Approved {
                scope: GrantScope::PathPrefix { .. }
            })
        ));
    }

    #[test]
    fn shift_tab_requests_a_mode_toggle() {
        let mut app = app();
        assert_eq!(
            app.handle_key(key(KeyCode::BackTab)),
            Some(Command::ToggleMode)
        );
        assert_eq!(app.handle_key(key(KeyCode::Tab)), None);
    }

    #[test]
    fn ctrl_o_toggles_process_details() {
        let mut app = app();
        assert!(!app.show_process);
        assert_eq!(app.handle_key(ctrl('o')), None);
        assert!(app.show_process);
        app.handle_key(ctrl('o'));
        assert!(!app.show_process);
    }

    #[test]
    fn a_code_block_is_rendered_with_a_bar_and_no_fences() {
        let mut app = app();
        app.blocks.push(Block {
            kind: BlockKind::Result,
            text: "before\n```rust\nlet x = 1;\n```\nafter".to_owned(),
        });
        let rendered = frame(&app, 80, 20);
        assert!(rendered.contains("╭─ rust "), "{rendered}");
        assert!(rendered.contains("│ let x = 1;"), "{rendered}");
        assert!(rendered.contains("╰"), "{rendered}");
        assert!(!rendered.contains("```"), "{rendered}");
        assert!(rendered.contains("before"), "{rendered}");
    }

    #[test]
    fn a_table_becomes_aligned_columns() {
        let mut app = app();
        app.blocks.push(Block {
            kind: BlockKind::Result,
            text: "| method | path |\n| --- | --- |\n| GET | /health |\n| POST | /items |"
                .to_owned(),
        });
        let rendered = frame(&app, 80, 20);
        assert!(rendered.contains("method"), "{rendered}");
        assert!(rendered.contains("POST"), "{rendered}");
        assert!(rendered.contains("────"), "{rendered}");
        assert!(!rendered.contains('|'), "{rendered}");
    }

    #[test]
    fn inline_code_markers_are_removed() {
        let mut app = app();
        app.blocks.push(Block {
            kind: BlockKind::Result,
            text: "run `cargo test` now".to_owned(),
        });
        let rendered = frame(&app, 80, 20);
        assert!(rendered.contains("run cargo test now"), "{rendered}");
        assert!(!rendered.contains('`'), "{rendered}");
    }

    #[test]
    fn tool_lines_separate_input_and_output() {
        let mut app = app();
        app.start_turn("go".to_owned());
        app.blocks.push(Block {
            kind: BlockKind::Tool {
                name: "shell".to_owned(),
                ok: Some(true),
                duration_ms: 12,
                args: "program=python".to_owned(),
            },
            text: "exit code: 0".to_owned(),
        });
        app.collapse("done", None, &TokenUsage::default(), 20, 0);
        app.handle_key(ctrl('o'));

        let rendered = frame(&app, 80, 24);
        assert!(rendered.contains("✓ shell  12 ms"), "{rendered}");
        assert!(rendered.contains("in   program=python"), "{rendered}");
        assert!(rendered.contains("out  exit code: 0"), "{rendered}");
    }

    #[test]
    fn the_input_box_grows_with_a_pasted_message() {
        let mut app = app();
        app.input = "line1\nline2\nline3\nline4".chars().collect();
        app.cursor = app.input.len();
        let rendered = frame(&app, 60, 20);
        for line in ["line1", "line2", "line3", "line4"] {
            assert!(rendered.contains(line), "{rendered}");
        }
    }

    #[test]
    fn a_long_sent_message_collapses_until_details_are_shown() {
        let mut app = app();
        let long: String = (1..=12).map(|n| format!("line{n}\n")).collect();
        app.start_turn(long);
        app.collapse("done", None, &TokenUsage::default(), 10, 0);

        let hidden = frame(&app, 80, 30);
        assert!(hidden.contains("line1"), "{hidden}");
        assert!(!hidden.contains("line12"), "{hidden}");
        assert!(hidden.contains("Ctrl-O"), "{hidden}");

        app.handle_key(ctrl('o'));
        let shown = frame(&app, 80, 30);
        assert!(shown.contains("line12"), "{shown}");
    }

    #[test]
    fn add_dir_parses_access_and_defaults_to_write() {
        assert_eq!(
            parse_add_dir("/add-dir C:/notes"),
            Some(("C:/notes".to_owned(), AccessKind::Write))
        );
        assert_eq!(
            parse_add_dir("/add-dir C:/notes ro"),
            Some(("C:/notes".to_owned(), AccessKind::Read))
        );
        assert_eq!(
            parse_add_dir("/add-dir C:/my notes ro"),
            Some(("C:/my notes".to_owned(), AccessKind::Read))
        );
        assert!(parse_add_dir("/add-dir").is_none());
        assert!(parse_add_dir("/add-dir   ").is_none());
    }

    #[test]
    fn a_recorded_root_updates_the_status_line() {
        let mut app = App::new(
            Text::new(crate::ui_text::Lang::En),
            PathBuf::from("C:/keel"),
            "workspace_auto".to_owned(),
            vec!["C:/keel [write]".to_owned()],
        );
        app.record_root(RootScope {
            path: PathBuf::from("D:/notes"),
            access: AccessKind::Read,
        });
        assert_eq!(app.extra_roots, 1);
        assert!(app
            .roots
            .iter()
            .any(|line| line.contains("D:/notes") && line.contains("read")));
        let rendered = frame(&app, 80, 12);
        assert!(rendered.contains("keel +1"), "{rendered}");
    }

    #[test]
    fn the_status_line_names_the_workspace_and_extra_roots() {
        let app = App::new(
            Text::new(crate::ui_text::Lang::En),
            PathBuf::from("C:/keel"),
            "workspace_auto".to_owned(),
            vec!["C:/keel".to_owned(), "D:/notes".to_owned()],
        );
        let rendered = frame(&app, 80, 12);
        assert!(rendered.contains("keel +1"), "{rendered}");
    }

    #[test]
    fn the_chinese_screen_renders_translated_chrome() {
        let app = App::new(
            Text::new(crate::ui_text::Lang::Zh),
            PathBuf::from("C:/keel"),
            "workspace_auto".to_owned(),
            Vec::new(),
        );
        // The test buffer lists each wide cell separately, so compare with
        // spaces removed.
        let plain: String = frame(&app, 80, 12).replace(' ', "");
        assert!(plain.contains("输入任务"), "{plain}");
        assert!(plain.contains("就绪"), "{plain}");
    }

    #[test]
    fn ctrl_o_reveals_the_last_turn_process() {
        let mut app = app();
        app.start_turn("go".to_owned());
        app.blocks.push(Block {
            kind: BlockKind::Tool {
                name: "list_dir".to_owned(),
                ok: Some(true),
                duration_ms: 3,
                args: String::new(),
            },
            text: String::new(),
        });
        app.collapse("done", None, &TokenUsage::default(), 10, 0);
        let hidden = frame(&app, 60, 20);
        assert!(!hidden.contains("list_dir"), "{hidden}");

        app.handle_key(ctrl('o'));
        let shown = frame(&app, 60, 20);
        assert!(shown.contains("list_dir"), "{shown}");
    }

    #[test]
    fn a_cancelled_turn_says_so() {
        let mut app = app();
        app.add_end_notice(TurnState::Cancelled, true, false);
        let last = app.blocks.last().expect("notice");
        assert_eq!(last.kind, BlockKind::Notice);
        assert!(last.text.contains("cancelled"), "{}", last.text);
    }

    #[test]
    fn a_missing_answer_is_explained() {
        let mut app = app();
        app.add_end_notice(TurnState::Done, true, false);
        let last = app.blocks.last().expect("notice");
        assert!(last.text.contains("without an answer"), "{}", last.text);
    }

    #[test]
    fn the_details_command_toggles_process_details() {
        let mut app = app();
        assert!(!app.show_process);
        run_slash(&mut app, "/details", None, None);
        assert!(app.show_process);
        run_slash(&mut app, "/details", None, None);
        assert!(!app.show_process);
    }

    #[test]
    fn wrapping_respects_cjk_width() {
        let lines = wrap_text("中文中文中文", 4);
        assert_eq!(lines, vec!["中文", "中文", "中文"]);
    }

    #[test]
    fn the_empty_screen_has_a_header_input_and_status() {
        let rendered = frame(&app(), 80, 24);
        golden("tui-empty.txt", &rendered);
        assert!(rendered.contains("● ready"), "{rendered}");
        assert!(rendered.contains("Workspace · Shift+Tab"), "{rendered}");
        assert!(rendered.contains("> ▏"), "{rendered}");
    }

    #[test]
    fn a_streamed_answer_is_rendered_in_the_transcript() {
        let mut app = app();
        app.start_turn("say hello".to_owned());
        app.push_delta("hello from the model");
        let rendered = frame(&app, 80, 24);
        golden("tui-streaming.txt", &rendered);
        assert!(rendered.contains("› say hello"));
        assert!(rendered.contains("hello from the model"));
        assert!(rendered.contains("working"));
    }

    #[test]
    fn a_finished_turn_renders_only_the_result_and_its_fields() {
        let mut app = app();
        app.start_turn("say hello".to_owned());
        app.push_delta("partial");
        app.blocks.push(Block {
            kind: BlockKind::Tool {
                name: "echo".to_owned(),
                ok: Some(true),
                duration_ms: 1,
                args: String::new(),
            },
            text: String::new(),
        });
        app.turn_tools = 1;
        app.collapse("final answer", None, &usage(), 12, 1);

        let rendered = frame(&app, 80, 24);
        golden("tui-result.txt", &rendered);
        assert!(rendered.contains("final answer"));
        assert!(rendered.contains("1 tool · 1 file"));
        assert!(!rendered.contains("partial"));
        assert!(!rendered.contains("echo"));
    }

    #[test]
    fn the_permission_prompt_is_inline() {
        let mut app = app();
        let (job, _answer) =
            permission_job("Allow `shell` to execute `cargo`?\ncommand: cargo test");
        app.open_interaction(job);

        let rendered = frame(&app, 80, 24);
        golden("tui-prompt.txt", &rendered);
        assert!(rendered.contains("Allow `shell`"));
        assert!(rendered
            .lines()
            .any(|line| line.contains("command: cargo test")));
        assert!(rendered.contains("› Allow once"), "the default is marked");
        assert!(rendered.contains("Full auto"));
        assert!(rendered.contains("↑↓ select"));
    }

    #[test]
    fn a_finished_turn_offers_the_next_one_when_tasks_remain() {
        let text = Text::new(crate::ui_text::Lang::En);
        assert_eq!(
            continuation_hint(text, TurnState::Done, 2),
            Some("2 task(s) remain; press Enter to continue".to_owned())
        );
        assert!(continuation_hint(text, TurnState::Done, 0).is_none());
        assert!(continuation_hint(text, TurnState::Error, 2).is_none());
        assert!(continuation_hint(text, TurnState::Cancelled, 2).is_none());
    }
}
