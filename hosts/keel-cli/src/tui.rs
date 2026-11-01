//! Minimal terminal UI for the reference host.
//!
//! The state reducer and the renderer are separate from the async loop so the
//! frontend can be tested with `TestBackend` and synthetic key events, without
//! a terminal. The loop only wires channels: model deltas, kernel events,
//! interactions, turns and keys.

use std::io;
use std::io::IsTerminal;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use keel_core::{
    CancelToken, Event, InteractionKind, InteractionPort, InteractionRequest, InteractionResponse,
    KernelError, TodoList, TokenUsage,
};
use keel_runtime::{EventLog, RuntimeConfig, TurnDriver, TurnOutcome};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block as FrameBlock, Clear, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use tokio::sync::{mpsc, oneshot};
use unicode_width::UnicodeWidthChar;

use crate::store::Store;
use crate::terminal::parse_response;
use crate::HostModel;

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
    pub driver: TurnDriver<HostModel>,
    pub config: RuntimeConfig,
    pub events: Arc<EventLog>,
    pub todos: Option<Arc<Mutex<TodoList>>>,
    pub store: Option<Store>,
    pub session_id: String,
    pub interactions: mpsc::Receiver<InteractionJob>,
    pub ui: mpsc::Receiver<UiEvent>,
}

struct TurnDone {
    input: String,
    outcome: TurnOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Submit(String),
    Slash(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BlockKind {
    User,
    Assistant,
    Tool {
        name: String,
        ok: Option<bool>,
        duration_ms: u64,
    },
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
    profile: String,
    model: String,
    session_id: String,
    permission: String,
    blocks: Vec<Block>,
    input: Vec<char>,
    cursor: usize,
    history: Vec<String>,
    history_pos: Option<usize>,
    scroll: usize,
    follow: bool,
    running: bool,
    cancel: Option<CancelToken>,
    pending: Option<InteractionJob>,
    status: String,
    usage: UsageTotal,
    last_seq: u64,
    streaming: Option<usize>,
    should_quit: bool,
}

impl App {
    fn new(profile: String, model: String, session_id: String, permission: String) -> Self {
        Self {
            profile,
            model,
            session_id,
            permission,
            blocks: vec![Block {
                kind: BlockKind::Notice,
                text: "type a request; /help lists commands".to_owned(),
            }],
            input: Vec::new(),
            cursor: 0,
            history: Vec::new(),
            history_pos: None,
            scroll: 0,
            follow: true,
            running: false,
            cancel: None,
            pending: None,
            status: "ready".to_owned(),
            usage: UsageTotal::default(),
            last_seq: 0,
            streaming: None,
            should_quit: false,
        }
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

    fn start_turn(&mut self, input: String) {
        self.blocks.push(Block {
            kind: BlockKind::User,
            text: input,
        });
        self.blocks.push(Block {
            kind: BlockKind::Assistant,
            text: String::new(),
        });
        self.streaming = Some(self.blocks.len() - 1);
        self.running = true;
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
        if let Some(index) = self.streaming.take() {
            let final_text = &done.outcome.answer.body;
            if !final_text.is_empty() {
                if let Some(block) = self.blocks.get_mut(index) {
                    block.text = final_text.clone();
                }
            } else if self
                .blocks
                .get(index)
                .is_some_and(|block| block.text.is_empty())
            {
                self.blocks.remove(index);
            }
        }
        if let Some(error) = &done.outcome.error {
            self.error(error.to_string());
        }
        if !done.outcome.answer.changes.is_empty() {
            let paths: Vec<&str> = done
                .outcome
                .answer
                .changes
                .iter()
                .map(|change| change.path.as_str())
                .collect();
            self.notice(format!("changed: {}", paths.join(", ")));
        }
        self.usage.add(&done.outcome.usage);
        self.running = false;
        self.cancel = None;
        self.status = "ready".to_owned();
    }

    fn open_interaction(&mut self, job: InteractionJob) {
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
                Event::ToolStarted { tool, .. } => {
                    self.blocks.push(Block {
                        kind: BlockKind::Tool {
                            name: tool.clone(),
                            ok: None,
                            duration_ms: 0,
                        },
                        text: String::new(),
                    });
                    self.follow = true;
                }
                Event::ToolFinished {
                    tool,
                    ok,
                    duration_ms,
                    ..
                } => {
                    if let Some(block) = self.blocks.iter_mut().rev().find(|block| {
                        matches!(&block.kind, BlockKind::Tool { name, ok: None, .. } if name.as_str() == tool.as_str())
                    }) {
                        block.kind = BlockKind::Tool {
                            name: tool.clone(),
                            ok: Some(*ok),
                            duration_ms: *duration_ms,
                        };
                    }
                }
                _ => {}
            }
        }
    }

    fn cancel_turn(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
            self.notice("cancelling…");
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Command> {
        if key.kind != KeyEventKind::Press {
            return None;
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
                    None
                }
                _ => None,
            };
        }
        match key.code {
            KeyCode::Enter => {
                if self.running {
                    self.notice("a turn is running; Ctrl-C cancels it");
                    return None;
                }
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
                self.follow = false;
                self.scroll = self.scroll.saturating_sub(10);
                None
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(10);
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
        let permission = job.request.kind == InteractionKind::PermissionApproval;
        let response = match key.code {
            KeyCode::Esc => Some(InteractionResponse::Deferred),
            KeyCode::Char('d') | KeyCode::Char('1') => Some(parse_response(&job.request, "deny")),
            KeyCode::Char('a') | KeyCode::Char('2') => {
                Some(parse_response(&job.request, "allow_once"))
            }
            KeyCode::Char('s') | KeyCode::Char('3') => {
                Some(parse_response(&job.request, "allow_scope"))
            }
            KeyCode::Char('f') | KeyCode::Char('4') => {
                Some(parse_response(&job.request, "full_auto"))
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
            Some(response) => {
                self.notice(format!("interaction: {}", describe(&response)));
                self.answer(job, response);
            }
            None => self.pending = Some(job),
        }
    }

    fn transcript(&self, width: usize) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for block in &self.blocks {
            let (prefix, text, style) = match &block.kind {
                BlockKind::User => ("> ", block.text.clone(), Style::default().fg(Color::Cyan)),
                BlockKind::Assistant => ("", block.text.clone(), Style::default()),
                BlockKind::Tool {
                    name,
                    ok,
                    duration_ms,
                } => {
                    let text = match ok {
                        None => format!("● {name} …"),
                        Some(true) => format!("● {name} ok {duration_ms} ms"),
                        Some(false) => format!("● {name} failed {duration_ms} ms"),
                    };
                    let color = if matches!(ok, Some(false)) {
                        Color::Red
                    } else {
                        Color::DarkGray
                    };
                    ("", text, Style::default().fg(color))
                }
                BlockKind::Notice => ("· ", block.text.clone(), Style::default().fg(Color::Yellow)),
                BlockKind::Error => ("! ", block.text.clone(), Style::default().fg(Color::Red)),
            };
            for line in wrap_text(&format!("{prefix}{text}"), width) {
                lines.push(Line::from(Span::styled(line, style)));
            }
            lines.push(Line::from(""));
        }
        lines
    }
}

fn describe(response: &InteractionResponse) -> &'static str {
    match response {
        InteractionResponse::Approved { .. } => "approved",
        InteractionResponse::Rejected { .. } => "denied",
        InteractionResponse::Deferred => "deferred",
        InteractionResponse::Cancelled => "cancelled",
        InteractionResponse::Selected { .. } => "selected",
        InteractionResponse::Freeform { .. } => "answered",
        InteractionResponse::TimedOut => "timed out",
    }
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
        host.config.profile_name.clone(),
        host.config.model_route.model.clone(),
        host.session_id.clone(),
        format!("{:?}", host.config.permission_level).to_lowercase(),
    );
    let driver = Arc::new(host.driver);
    let (key_tx, mut key_rx) = mpsc::channel(64);
    let stop = Arc::new(AtomicBool::new(false));
    spawn_key_reader(key_tx, stop.clone());
    let (turn_tx, mut turn_rx) = mpsc::channel::<TurnDone>(4);
    let mut interactions = host.interactions;
    let mut ui = host.ui;
    let mut store = host.store;
    let mut ticks = tokio::time::interval(Duration::from_millis(80));

    while !app.should_quit {
        terminal.draw(|frame| draw(&app, frame)).map_err(io_error)?;
        tokio::select! {
            Some(key) = key_rx.recv() => {
                let CrosstermEvent::Key(key) = key else { continue };
                match app.handle_key(key) {
                    Some(Command::Submit(input)) => start_turn(&mut app, driver.clone(), turn_tx.clone(), input),
                    Some(Command::Slash(command)) => run_slash(&mut app, &command, store.as_ref()),
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

fn start_turn(
    app: &mut App,
    driver: Arc<TurnDriver<HostModel>>,
    tx: mpsc::Sender<TurnDone>,
    input: String,
) {
    let cancel = CancelToken::new();
    app.cancel = Some(cancel.clone());
    app.start_turn(input.clone());
    tokio::spawn(async move {
        let outcome = driver.run(&input, cancel).await;
        let _ = tx.send(TurnDone { input, outcome }).await;
    });
}

fn run_slash(app: &mut App, command: &str, store: Option<&Store>) {
    match command.trim() {
        "/help" => app.notice("commands: /help /clear /cost /sessions /cancel /quit"),
        "/quit" | "/exit" => app.should_quit = true,
        "/clear" => app.blocks.clear(),
        "/cost" => app.notice(format!(
            "tokens: input {} output {} cached {}",
            app.usage.input, app.usage.output, app.usage.cached
        )),
        "/cancel" => app.cancel_turn(),
        "/sessions" => match store {
            Some(store) => match crate::view::sessions(store.connection()) {
                Ok(sessions) if sessions.is_empty() => app.notice("no sessions"),
                Ok(sessions) => {
                    for session in sessions {
                        app.notice(format!(
                            "{}  {} turns  {}",
                            session.id,
                            session.turns,
                            session.last_turn_state.unwrap_or_else(|| "-".to_owned())
                        ));
                    }
                }
                Err(error) => app.error(format!("cannot list sessions: {error}")),
            },
            None => app.notice("no store configured; pass --store"),
        },
        other => app.notice(format!("unknown command `{other}`; try /help")),
    }
}

fn draw(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(area);

    let header = format!(
        " Keel · {} · {} · session {} · {} ",
        app.profile, app.model, app.session_id, app.permission
    );
    frame.render_widget(
        Paragraph::new(header).style(Style::default().add_modifier(Modifier::BOLD)),
        chunks[0],
    );

    let lines = app.transcript(chunks[1].width as usize);
    let visible = chunks[1].height as usize;
    let offset = if app.follow {
        lines.len().saturating_sub(visible)
    } else {
        app.scroll.min(lines.len().saturating_sub(visible))
    };
    frame.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), chunks[1]);

    let before: String = app.input[..app.cursor.min(app.input.len())]
        .iter()
        .collect();
    let after: String = app.input[app.cursor.min(app.input.len())..]
        .iter()
        .collect();
    let input = format!("> {before}▏{after}");
    frame.render_widget(
        Paragraph::new(input)
            .wrap(Wrap { trim: false })
            .block(FrameBlock::bordered().title(" message ")),
        chunks[2],
    );

    let status = if app.running {
        " ● working · Ctrl-C cancel · PgUp/PgDn scroll "
    } else {
        " ● ready · Enter send · Ctrl-C quit · /help "
    };
    frame.render_widget(
        Paragraph::new(status).style(Style::default().fg(if app.running {
            Color::Yellow
        } else {
            Color::DarkGray
        })),
        chunks[3],
    );

    if let Some(job) = &app.pending {
        draw_modal(job, frame, area);
    }
}

fn draw_modal(job: &InteractionJob, frame: &mut Frame, area: Rect) {
    let width = area.width.clamp(20, 70);
    let height = area.height.clamp(6, 14);
    let modal = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, modal);
    let title = if job.request.kind == InteractionKind::PermissionApproval {
        " permission ".to_owned()
    } else {
        format!(" {} ", job.request.kind.as_str())
    };
    let mut lines = vec![Line::from(job.request.prompt.clone()), Line::from("")];
    if job.request.kind == InteractionKind::PermissionApproval {
        lines.push(Line::from(" d  Deny"));
        lines.push(Line::from(" a  Allow once"));
        lines.push(Line::from(" s  Allow this path for the session"));
        lines.push(Line::from(" f  Allow and switch to full auto"));
    } else {
        for (index, option) in job.request.options.iter().enumerate() {
            lines.push(Line::from(format!(" {}. {}", index + 1, option.label)));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(" Esc  defer"));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(FrameBlock::bordered().title(title)),
        modal,
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
            "coding".to_owned(),
            "mock".to_owned(),
            "s".to_owned(),
            "workspace_auto".to_owned(),
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
    fn tool_events_become_one_line_blocks() {
        let mut app = app();
        let events = EventLog::new();
        events.append(Event::ToolStarted {
            turn_id: TurnId::new(1),
            call_id: keel_core::ToolCallId::new(1),
            tool: "shell".to_owned(),
            risk: keel_core::RiskLevel::Dangerous,
        });
        events.append(Event::ToolFinished {
            turn_id: TurnId::new(1),
            call_id: keel_core::ToolCallId::new(1),
            tool: "shell".to_owned(),
            ok: true,
            error: None,
            duration_ms: 12,
        });

        app.drain_events(&events);

        assert_eq!(
            app.blocks.last().unwrap().kind,
            BlockKind::Tool {
                name: "shell".to_owned(),
                ok: Some(true),
                duration_ms: 12
            }
        );
    }

    #[test]
    fn permission_keys_map_to_the_same_scopes_as_the_line_ui() {
        let mut app = app();
        let (reply, mut answer) = oneshot::channel();
        app.open_interaction(InteractionJob {
            request: InteractionRequest::new(
                TurnId::new(1),
                InteractionKind::PermissionApproval,
                "Allow shell?",
            )
            .with_options(vec![
                InteractionOption::new("deny", "Deny"),
                InteractionOption::new("allow_once", "Allow once"),
                InteractionOption::new("allow_scope", "Allow this path"),
                InteractionOption::new("full_auto", "Full auto"),
            ]),
            reply,
        });

        app.handle_key(key(KeyCode::Char('a')));

        assert!(app.pending.is_none());
        assert!(matches!(
            answer.try_recv(),
            Ok(InteractionResponse::Approved {
                scope: keel_core::GrantScope::Once
            })
        ));
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
        assert!(rendered.contains("Keel · coding · mock · session s"));
        assert!(rendered.contains("> ▏"));
        assert!(rendered.contains("Enter send"));
    }

    #[test]
    fn a_streamed_answer_is_rendered_in_the_transcript() {
        let mut app = app();
        app.start_turn("say hello".to_owned());
        app.push_delta("hello from the model");
        let rendered = frame(&app, 80, 24);
        golden("tui-streaming.txt", &rendered);
        assert!(rendered.contains("> say hello"));
        assert!(rendered.contains("hello from the model"));
        assert!(rendered.contains("working"));
    }

    #[test]
    fn the_permission_modal_lists_every_scope() {
        let mut app = app();
        let (reply, _answer) = oneshot::channel();
        app.open_interaction(InteractionJob {
            request: InteractionRequest::new(
                TurnId::new(1),
                InteractionKind::PermissionApproval,
                "Allow `shell` to execute `cargo`?",
            )
            .with_options(vec![
                InteractionOption::new("deny", "Deny"),
                InteractionOption::new("allow_once", "Allow once"),
                InteractionOption::new("allow_scope", "Allow this path"),
                InteractionOption::new("full_auto", "Full auto"),
            ]),
            reply,
        });

        let rendered = frame(&app, 80, 24);
        golden("tui-modal.txt", &rendered);
        assert!(rendered.contains("Allow `shell`"));
        assert!(rendered.contains("Allow once"));
        assert!(rendered.contains("full auto"));
    }
}
