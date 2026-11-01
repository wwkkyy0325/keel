//! One structured command execution path for shell tools and verifiers.
//!
//! Programs and arguments are passed separately; there is no shell, no
//! interpolation and no command string.

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keel_core::{CancelToken, ToolError};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

/// Bytes kept from the start of one stream.
const HEAD_BYTES: usize = 48 * 1024;
/// Bytes kept from the end of one stream.
const TAIL_BYTES: usize = 16 * 1024;
/// How long to wait for pipe readers after the child exits. A descendant may
/// hold the pipe open; the capture is shared, so aborting loses nothing.
const READER_GRACE: Duration = Duration::from_secs(1);

/// Developer programs shared by the shell tool and command verifiers. Shells
/// are excluded so arguments cannot be re-interpreted.
pub fn coding_allowlist() -> Vec<String> {
    [
        "cargo", "rustc", "git", "python", "python3", "node", "npm", "npx", "pytest", "go", "make",
        "cmake", "ninja", "java", "javac", "dotnet", "pnpm", "yarn",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

/// Input to one bounded command execution.
pub struct CommandSpec<'a> {
    pub program: &'a str,
    pub args: &'a [String],
    pub cwd: &'a Path,
    pub timeout: Duration,
}

/// Captured command result.
#[derive(Debug)]
pub struct CommandOutput {
    pub exit_code: Option<i32>,
    pub output: String,
    pub truncated: bool,
}

/// Checks a program name against an allowlist. Paths and shell names must be
/// listed explicitly; `cargo.exe` matches `cargo`.
pub fn is_allowed(program: &str, allowlist: &[String]) -> bool {
    if program.contains('/') || program.contains('\\') {
        return false;
    }
    let name = program.to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    allowlist
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(name))
}

/// Runs one command without a shell and returns bounded output.
///
/// Both pipes are drained while the process runs. Memory stays bounded by
/// [`HEAD_BYTES`] plus [`TAIL_BYTES`] per stream; a command that prints
/// without end is discarded past those limits instead of being buffered.
pub async fn run(spec: CommandSpec<'_>, cancel: &CancelToken) -> Result<CommandOutput, ToolError> {
    if cancel.is_cancelled() {
        return Err(ToolError::cancelled());
    }

    let mut child = spawn(&spec)?;
    let mut tree = TreeGuard::new(child.id());
    let stdout_capture = Arc::new(Mutex::new(Capture::default()));
    let stderr_capture = Arc::new(Mutex::new(Capture::default()));
    let stdout_reader = child
        .stdout
        .take()
        .map(|stdout| tokio::spawn(capture_stream(stdout, stdout_capture.clone())));
    let stderr_reader = child
        .stderr
        .take()
        .map(|stderr| tokio::spawn(capture_stream(stderr, stderr_capture.clone())));

    let status = tokio::select! {
        _ = cancel.cancelled() => return Err(ToolError::cancelled()),
        result = tokio::time::timeout(spec.timeout, child.wait()) => {
            match result {
                Ok(Ok(status)) => status,
                Ok(Err(error)) => return Err(io_error(&error)),
                Err(_) => return Err(ToolError::timeout(spec.timeout)),
            }
        }
    };
    // The direct child exited on its own; descendants are not ours to kill.
    tree.disarm();
    settle(stdout_reader).await;
    settle(stderr_reader).await;

    let (stdout, stdout_truncated) = text_of(&stdout_capture);
    let (stderr, stderr_truncated) = text_of(&stderr_capture);
    let mut text = format!(
        "exit code: {}",
        status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "terminated".to_owned())
    );
    text.push_str("\nstdout:\n");
    text.push_str(&stdout);
    text.push_str("\nstderr:\n");
    text.push_str(&stderr);

    Ok(CommandOutput {
        exit_code: status.code(),
        output: text,
        truncated: stdout_truncated || stderr_truncated,
    })
}

fn build(program: &OsStr, spec: &CommandSpec<'_>) -> Command {
    let mut command = Command::new(program);
    command
        .args(spec.args)
        .current_dir(spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // A timed-out wrapper must not leave its descendants running.
        command.as_std_mut().process_group(0);
    }
    for (name, _) in std::env::vars() {
        if is_sensitive_env(&name) {
            command.env_remove(&name);
        }
    }
    command
}

fn spawn(spec: &CommandSpec<'_>) -> Result<Child, ToolError> {
    match build(OsStr::new(spec.program), spec).spawn() {
        Ok(child) => Ok(child),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // npm, npx, pnpm and yarn are `.cmd` shims on Windows. CreateProcess
            // does not resolve them from a bare name, so the allowlisted name
            // would never start. Rust handles the batch-file quoting.
            #[cfg(windows)]
            {
                if let Some(shim) = windows_shim(spec.program) {
                    return build(shim.as_os_str(), spec)
                        .spawn()
                        .map_err(|error| io_error(&error));
                }
            }
            Err(io_error(&error))
        }
        Err(error) => Err(io_error(&error)),
    }
}

#[cfg(windows)]
fn windows_shim(program: &str) -> Option<PathBuf> {
    if program.contains('/') || program.contains('\\') || Path::new(program).extension().is_some() {
        return None;
    }
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        for extension in ["cmd", "bat"] {
            let candidate = directory.join(format!("{program}.{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Bounded capture of one pipe: the head and the tail are kept, the middle is
/// counted and dropped.
#[derive(Debug, Default)]
struct Capture {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    total: u64,
}

impl Capture {
    fn push(&mut self, chunk: &[u8]) {
        self.total = self.total.saturating_add(chunk.len() as u64);
        let room = HEAD_BYTES.saturating_sub(self.head.len());
        let head_len = room.min(chunk.len());
        self.head.extend_from_slice(&chunk[..head_len]);
        if head_len < chunk.len() {
            self.tail.extend(&chunk[head_len..]);
            while self.tail.len() > TAIL_BYTES {
                self.tail.pop_front();
            }
        }
    }

    fn render(&self) -> (String, bool) {
        let retained = self.head.len() + self.tail.len();
        let omitted = self.total.saturating_sub(retained as u64);
        let mut text = String::from_utf8_lossy(&self.head).into_owned();
        if omitted > 0 {
            text.push_str(&format!("\n... {omitted} bytes omitted ...\n"));
            let tail: Vec<u8> = self.tail.iter().copied().collect();
            text.push_str(&String::from_utf8_lossy(&tail));
        }
        (text, omitted > 0)
    }
}

async fn capture_stream<R>(mut reader: R, capture: Arc<Mutex<Capture>>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let mut capture = capture
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                capture.push(&buffer[..read]);
            }
        }
    }
}

/// Waits for a pipe reader after the child exited. A descendant can hold the
/// pipe open; the capture is shared, so the task is aborted rather than
/// blocking the turn.
async fn settle(mut reader: Option<JoinHandle<()>>) {
    let Some(handle) = reader.as_mut() else {
        return;
    };
    if tokio::time::timeout(READER_GRACE, &mut *handle)
        .await
        .is_err()
    {
        handle.abort();
    }
}

fn text_of(capture: &Mutex<Capture>) -> (String, bool) {
    capture
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .render()
}

/// Kills the process tree when a turn is cancelled or a tool times out.
///
/// Dropping the child alone kills only the wrapper; a build or a package
/// manager leaves compiler and runtime processes behind, and those hold ports
/// and files after the turn is gone.
struct TreeGuard {
    pid: Option<u32>,
    armed: bool,
}

impl TreeGuard {
    fn new(pid: Option<u32>) -> Self {
        Self { pid, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TreeGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Some(pid) = self.pid else {
            return;
        };
        kill_tree(pid);
    }
}

fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        // The child leads its own process group, so the group id is its pid.
        let _ = std::process::Command::new("kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Environment variables a child process must not inherit. Hosts load model
/// keys from `.env`, and a shell call must not be able to print them.
pub fn is_sensitive_env(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.contains("KEY")
        || upper.contains("TOKEN")
        || upper.contains("SECRET")
        || upper.contains("PASSWORD")
        || upper.contains("CREDENTIAL")
        || upper.ends_with("_PAT")
}

fn io_error(error: &std::io::Error) -> ToolError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ToolError::not_found(error.to_string()),
        std::io::ErrorKind::PermissionDenied => ToolError::permission_denied(error.to_string()),
        _ => ToolError::io(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_environment_names_are_filtered() {
        assert!(is_sensitive_env("OPENAI_API_KEY"));
        assert!(is_sensitive_env("GITHUB_TOKEN"));
        assert!(is_sensitive_env("AWS_SECRET_ACCESS_KEY"));
        assert!(is_sensitive_env("SERVICE_PASSWORD"));
        assert!(is_sensitive_env("GITHUB_PAT"));
        assert!(!is_sensitive_env("PATH"));
        assert!(!is_sensitive_env("HOME"));
        assert!(!is_sensitive_env("CARGO_HOME"));
    }

    #[test]
    fn a_capture_keeps_a_bounded_head_and_tail() {
        let mut capture = Capture::default();
        let chunk = vec![b'a'; 8192];
        for _ in 0..2048 {
            capture.push(&chunk);
        }
        capture.push(b"THE END");

        let (text, truncated) = capture.render();

        assert!(truncated);
        assert!(text.starts_with("aaaa"), "the head is kept");
        assert!(text.ends_with("THE END"), "the tail is kept");
        assert!(text.contains("bytes omitted"));
        assert!(
            text.len() <= HEAD_BYTES + TAIL_BYTES + 128,
            "retained {} bytes",
            text.len()
        );
    }

    #[test]
    fn a_short_capture_is_unchanged() {
        let mut capture = Capture::default();
        capture.push(b"hello");
        capture.push(b" world");

        let (text, truncated) = capture.render();

        assert!(!truncated);
        assert_eq!(text, "hello world");
    }

    #[test]
    fn allowlist_rejects_paths_and_shells() {
        let allowed = coding_allowlist();
        assert!(is_allowed("cargo", &allowed));
        assert!(is_allowed("cargo.exe", &allowed));
        assert!(!is_allowed("bash", &allowed));
        assert!(!is_allowed("./cargo", &allowed));
        assert!(!is_allowed("C:\\tools\\cargo.exe", &allowed));
    }
}
