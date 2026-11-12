//! Background commands: the process outlives the tool call and its result is
//! delivered at a safe point (ADR 0016).

use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use keel_core::{Event, ToolError, TurnId};

use crate::command::{self, CommandSpec};
use crate::event_log::EventLog;

/// Commands that may run at once per registry.
const LIMIT: usize = 4;
/// A background command is killed after this long, whatever it prints.
const HARD_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);
const TAIL_LINES: usize = 40;
const TAIL_CHARS: usize = 4000;

/// One finished background command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundResult {
    pub id: String,
    pub program: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub output_path: PathBuf,
    /// Last lines of the output, bounded.
    pub tail: String,
}

/// The kernel's view: start a command and drain the ones that finished.
pub trait BackgroundTasks: Send + Sync {
    /// Starts a command and returns the handle line shown to the model.
    fn start(&self, turn_id: TurnId, spec: &CommandSpec<'_>) -> Result<String, ToolError>;
    fn drain(&self) -> Vec<BackgroundResult>;
}

struct Running {
    id: u32,
    pid: u32,
    turn_id: TurnId,
    program: String,
    child: Child,
    started: Instant,
    output: PathBuf,
    max_duration: Option<Duration>,
}

#[derive(Default)]
struct State {
    next_id: u32,
    tasks: Vec<Running>,
}

/// One background command as a host reads it without draining the result.
#[derive(Clone, Debug)]
pub struct BackgroundSnapshot {
    pub id: String,
    pub program: String,
    pub pid: u32,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub output_path: String,
}

/// Local backend: one OS process per task, output in `outputs`.
pub struct LocalBackgroundTasks {
    state: Mutex<State>,
    outputs: PathBuf,
    events: Option<Arc<EventLog>>,
}

/// Highest `bg-N.log` handle already on disk, or 0.
fn highest_handle(dir: &Path) -> u32 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            let number = name.strip_prefix("bg-")?.strip_suffix(".log")?;
            number.parse::<u32>().ok()
        })
        .max()
        .unwrap_or(0)
}

impl LocalBackgroundTasks {
    pub fn new(outputs: impl Into<PathBuf>) -> Self {
        let outputs = outputs.into();
        // Handles restart at bg-1 with a new registry. Continue after the
        // logs already in the directory, so a new task does not append to an
        // older handle's log and two sessions never share one.
        let next_id = highest_handle(&outputs);
        Self {
            state: Mutex::new(State {
                next_id,
                tasks: Vec::new(),
            }),
            outputs,
            events: None,
        }
    }

    /// Records every start in the turn's event log. A start is otherwise only
    /// visible in a tool preview, which is not persisted.
    pub fn with_events(mut self, events: Arc<EventLog>) -> Self {
        self.events = Some(events);
        self
    }

    /// How many commands are running right now.
    pub fn running(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .tasks
            .len()
    }

    /// Kills a running command by its `bg-N` handle. The task stays in the
    /// registry so `drain` still reports how it ended, and the next snapshot
    /// shows it as no longer running. Returns false when the handle is
    /// unknown or the command had already exited.
    pub fn stop(&self, id: &str) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(task) = state
            .tasks
            .iter_mut()
            .find(|task| format!("bg-{}", task.id) == id)
        else {
            return false;
        };
        if matches!(task.child.try_wait(), Ok(Some(_))) {
            return false;
        }
        command::kill_tree(task.pid);
        true
    }

    /// Reports running and finished-but-not-drained commands without
    /// consuming them. The driver's `drain` still owns the result.
    pub fn snapshot(&self) -> Vec<BackgroundSnapshot> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state
            .tasks
            .iter_mut()
            .map(|task| {
                let (running, exit_code) = match task.child.try_wait() {
                    Ok(Some(status)) => (false, status.code()),
                    _ => (true, None),
                };
                BackgroundSnapshot {
                    id: format!("bg-{}", task.id),
                    program: task.program.clone(),
                    pid: task.pid,
                    running,
                    exit_code,
                    duration_ms: task.started.elapsed().as_millis() as u64,
                    output_path: task.output.display().to_string(),
                }
            })
            .collect()
    }
}

impl BackgroundTasks for LocalBackgroundTasks {
    fn start(&self, turn_id: TurnId, spec: &CommandSpec<'_>) -> Result<String, ToolError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.tasks.len() >= LIMIT {
            return Err(ToolError::invalid_arguments(format!(
                "background limit reached: {LIMIT} commands are already running"
            )));
        }
        std::fs::create_dir_all(&self.outputs).map_err(|error| ToolError::io(error.to_string()))?;
        let next_id = state.next_id.saturating_add(1);
        state.next_id = next_id;
        let id = format!("bg-{next_id}");
        let output = self.outputs.join(format!("{id}.log"));
        let child = command::spawn_background(spec, &output)?;
        let pid = child.id();
        state.tasks.push(Running {
            id: next_id,
            pid,
            turn_id,
            program: spec.program.to_owned(),
            child,
            started: Instant::now(),
            output: output.clone(),
            max_duration: spec.max_duration,
        });
        if let Some(events) = &self.events {
            events.append(Event::BackgroundStarted {
                turn_id,
                id: id.clone(),
                program: spec.program.to_owned(),
                pid,
            });
        }
        Ok(format!(
            "started {id} (pid {pid}); output: {}",
            output.display()
        ))
    }

    fn drain(&self) -> Vec<BackgroundResult> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut running = Vec::with_capacity(state.tasks.len());
        let mut finished = Vec::new();
        for mut task in std::mem::take(&mut state.tasks) {
            let status = task.child.try_wait();
            let done = match &status {
                Ok(Some(_)) => true,
                Err(_) => {
                    command::kill_tree(task.pid);
                    let _ = task.child.wait();
                    true
                }
                Ok(None)
                    if task
                        .max_duration
                        .is_some_and(|max| task.started.elapsed() >= max) =>
                {
                    command::kill_tree(task.pid);
                    let _ = task.child.wait();
                    true
                }
                Ok(None) if task.started.elapsed() >= HARD_TIMEOUT => {
                    command::kill_tree(task.pid);
                    let _ = task.child.wait();
                    true
                }
                Ok(None) => false,
            };
            if done {
                let exit_code = match status {
                    Ok(Some(status)) => status.code(),
                    _ => None,
                };
                finished.push(BackgroundResult {
                    id: format!("bg-{}", task.id),
                    program: task.program,
                    exit_code,
                    duration_ms: task.started.elapsed().as_millis() as u64,
                    tail: read_tail(&task.output),
                    output_path: task.output,
                });
            } else {
                running.push(task);
            }
        }
        state.tasks = running;
        finished
    }
}

impl Drop for LocalBackgroundTasks {
    fn drop(&mut self) {
        if let Ok(state) = self.state.get_mut() {
            // A shell command spawns children; killing only the leader leaves
            // the grandchildren running. `kill_tree` matches the foreground
            // path: a process group on Unix, `taskkill /T` on Windows.
            // No `wait()`: shutdown must not block on an unkillable process.
            let events = self.events.clone();
            for task in &mut state.tasks {
                command::kill_tree(task.pid);
                if let Some(events) = &events {
                    events.append(Event::BackgroundResult {
                        turn_id: task.turn_id,
                        id: format!("bg-{}", task.id),
                        program: task.program.clone(),
                        output_path: task.output.display().to_string(),
                        exit_code: None,
                        duration_ms: task.started.elapsed().as_millis() as u64,
                    });
                }
            }
        }
    }
}

fn read_tail(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().rev().take(TAIL_LINES).collect();
    let joined = lines.into_iter().rev().collect::<Vec<_>>().join("\n");
    let total = joined.chars().count();
    if total > TAIL_CHARS {
        joined.chars().skip(total - TAIL_CHARS).collect()
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn python_available() -> bool {
        std::process::Command::new("python")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn a_background_command_reports_its_output_and_exit() {
        if !python_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let tasks = LocalBackgroundTasks::new(dir.path().join("outputs"));
        let args = vec!["-c".to_owned(), "print('done')".to_owned()];
        let spec = CommandSpec {
            program: "python",
            args: &args,
            cwd: dir.path(),
            timeout: Duration::from_secs(5),
            max_duration: None,
        };
        let started = tasks.start(TurnId::new(1), &spec).unwrap();
        assert!(started.contains("bg-1"), "{started}");

        let mut results = Vec::new();
        for _ in 0..100 {
            results = tasks.drain();
            if !results.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].exit_code, Some(0));
        assert!(results[0].tail.contains("done"), "{}", results[0].tail);
        assert!(results[0].output_path.exists());
    }

    #[test]
    fn a_running_command_can_be_stopped() {
        if !python_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let tasks = LocalBackgroundTasks::new(dir.path().join("outputs"));
        let args = vec!["-c".to_owned(), "import time; time.sleep(30)".to_owned()];
        let spec = CommandSpec {
            program: "python",
            args: &args,
            cwd: dir.path(),
            timeout: Duration::from_secs(30),
            max_duration: None,
        };
        tasks.start(TurnId::new(1), &spec).unwrap();
        assert_eq!(tasks.running(), 1);

        assert!(tasks.stop("bg-1"));
        assert!(!tasks.stop("bg-9"), "an unknown handle is refused");

        let mut results = Vec::new();
        for _ in 0..100 {
            results = tasks.drain();
            if !results.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(results.len(), 1);
        assert_ne!(results[0].exit_code, Some(0));
        assert_eq!(tasks.running(), 0);
    }

    #[test]
    fn a_new_registry_continues_after_existing_handles() {
        if !python_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let outputs = dir.path().join("outputs");
        std::fs::create_dir_all(&outputs).unwrap();
        std::fs::write(outputs.join("bg-7.log"), "an earlier session").unwrap();
        let tasks = LocalBackgroundTasks::new(outputs);
        let args = vec!["-c".to_owned(), "print('new')".to_owned()];
        let spec = CommandSpec {
            program: "python",
            args: &args,
            cwd: dir.path(),
            timeout: Duration::from_secs(5),
            max_duration: None,
        };

        let started = tasks.start(TurnId::new(1), &spec).unwrap();

        assert!(started.contains("bg-8"), "{started}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("outputs/bg-7.log")).unwrap(),
            "an earlier session",
            "the older log must not be appended to"
        );
    }

    #[test]
    fn the_background_limit_is_enforced() {
        if !python_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let tasks = LocalBackgroundTasks::new(dir.path().join("outputs"));
        let args = vec!["-c".to_owned(), "import time; time.sleep(5)".to_owned()];
        let spec = CommandSpec {
            program: "python",
            args: &args,
            cwd: dir.path(),
            timeout: Duration::from_secs(5),
            max_duration: None,
        };
        for _ in 0..LIMIT {
            tasks.start(TurnId::new(1), &spec).unwrap();
        }
        let error = tasks.start(TurnId::new(1), &spec).unwrap_err();
        assert!(
            error.message.contains("background limit"),
            "{}",
            error.message
        );
        assert_eq!(tasks.running(), LIMIT);
    }

    #[test]
    fn dropping_the_registry_kills_the_process_tree() {
        if !python_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker.txt");
        let tasks = LocalBackgroundTasks::new(dir.path().join("outputs"));
        // The leader spawns a grandchild that would write the marker after
        // two seconds. Killing only the leader leaves the grandchild alive.
        let inner =
            "import time; time.sleep(2); open(__import__('sys').argv[1], 'w').write('alive')";
        let leader = format!(
            "import subprocess, sys; subprocess.run([sys.executable, '-c', {inner:?}, sys.argv[1]])"
        );
        let args = vec![
            "-c".to_owned(),
            leader,
            marker.to_string_lossy().into_owned(),
        ];
        let spec = CommandSpec {
            program: "python",
            args: &args,
            cwd: dir.path(),
            timeout: Duration::from_secs(5),
            max_duration: None,
        };
        tasks.start(TurnId::new(1), &spec).unwrap();
        // Let the leader spawn its child before the registry goes away.
        std::thread::sleep(Duration::from_millis(300));
        drop(tasks);
        std::thread::sleep(Duration::from_secs(3));
        assert!(
            !marker.exists(),
            "the grandchild survived the registry drop"
        );
    }

    #[test]
    fn a_background_start_is_audited() {
        if !python_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let events = Arc::new(EventLog::new());
        let tasks =
            LocalBackgroundTasks::new(dir.path().join("outputs")).with_events(events.clone());
        let args = vec!["-c".to_owned(), "import time; time.sleep(5)".to_owned()];
        let spec = CommandSpec {
            program: "python",
            args: &args,
            cwd: dir.path(),
            timeout: Duration::from_secs(5),
            max_duration: None,
        };
        tasks.start(TurnId::new(7), &spec).unwrap();

        let started = events
            .snapshot()
            .into_iter()
            .find_map(|record| match record.event {
                Event::BackgroundStarted {
                    turn_id,
                    id,
                    program,
                    pid,
                } => Some((turn_id, id, program, pid)),
                _ => None,
            })
            .expect("background_started");
        assert_eq!(started.0, TurnId::new(7));
        assert_eq!(started.1, "bg-1");
        assert_eq!(started.2, "python");
        assert!(started.3 > 0);
    }
}
