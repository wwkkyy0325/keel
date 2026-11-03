//! Background commands: the process outlives the tool call and its result is
//! delivered at a safe point (ADR 0016).

use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use keel_core::ToolError;

use crate::command::{self, CommandSpec};

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
    fn start(&self, spec: &CommandSpec<'_>) -> Result<String, ToolError>;
    fn drain(&self) -> Vec<BackgroundResult>;
}

struct Running {
    id: u32,
    program: String,
    child: Child,
    started: Instant,
    output: PathBuf,
}

#[derive(Default)]
struct State {
    next_id: u32,
    tasks: Vec<Running>,
}

/// Local backend: one OS process per task, output in `outputs`.
pub struct LocalBackgroundTasks {
    state: Mutex<State>,
    outputs: PathBuf,
}

impl LocalBackgroundTasks {
    pub fn new(outputs: impl Into<PathBuf>) -> Self {
        Self {
            state: Mutex::new(State::default()),
            outputs: outputs.into(),
        }
    }

    /// How many commands are running right now.
    pub fn running(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .tasks
            .len()
    }
}

impl BackgroundTasks for LocalBackgroundTasks {
    fn start(&self, spec: &CommandSpec<'_>) -> Result<String, ToolError> {
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
            program: spec.program.to_owned(),
            child,
            started: Instant::now(),
            output: output.clone(),
        });
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
                    let _ = task.child.kill();
                    true
                }
                Ok(None) if task.started.elapsed() >= HARD_TIMEOUT => {
                    let _ = task.child.kill();
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
            for task in &mut state.tasks {
                let _ = task.child.kill();
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
        };
        let started = tasks.start(&spec).unwrap();
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
        };
        for _ in 0..LIMIT {
            tasks.start(&spec).unwrap();
        }
        let error = tasks.start(&spec).unwrap_err();
        assert!(
            error.message.contains("background limit"),
            "{}",
            error.message
        );
        assert_eq!(tasks.running(), LIMIT);
    }
}
