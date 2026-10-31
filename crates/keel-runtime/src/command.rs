//! One structured command execution path for shell tools and verifiers.
//!
//! Programs and arguments are passed separately; there is no shell, no
//! interpolation and no command string.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use keel_core::{CancelToken, ToolError};
use tokio::process::Command;

const MAX_COMMAND_OUTPUT_BYTES: usize = 64 * 1024;

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

/// Runs one command without a shell and returns combined output.
pub async fn run(spec: CommandSpec<'_>, cancel: &CancelToken) -> Result<CommandOutput, ToolError> {
    if cancel.is_cancelled() {
        return Err(ToolError::cancelled());
    }

    let mut command = Command::new(spec.program);
    command
        .args(spec.args)
        .current_dir(spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (name, _) in std::env::vars() {
        if is_sensitive_env(&name) {
            command.env_remove(&name);
        }
    }

    let output = tokio::select! {
        _ = cancel.cancelled() => return Err(ToolError::cancelled()),
        result = tokio::time::timeout(spec.timeout, command.output()) => {
            match result {
                Ok(Ok(output)) => output,
                Ok(Err(error)) => return Err(io_error(&error)),
                Err(_) => return Err(ToolError::timeout(spec.timeout)),
            }
        }
    };

    let mut text = String::new();
    text.push_str(&format!(
        "exit code: {}",
        output
            .status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "terminated".to_owned())
    ));
    text.push_str("\nstdout:\n");
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str("\nstderr:\n");
    text.push_str(&String::from_utf8_lossy(&output.stderr));

    let mut truncated = false;
    if text.len() > MAX_COMMAND_OUTPUT_BYTES {
        let boundary = char_boundary(&text, MAX_COMMAND_OUTPUT_BYTES);
        text.truncate(boundary);
        text.push_str("\n... output truncated");
        truncated = true;
    }

    Ok(CommandOutput {
        exit_code: output.status.code(),
        output: text,
        truncated,
    })
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

fn char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
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
    fn allowlist_rejects_paths_and_shells() {
        let allowed = coding_allowlist();
        assert!(is_allowed("cargo", &allowed));
        assert!(is_allowed("cargo.exe", &allowed));
        assert!(!is_allowed("bash", &allowed));
        assert!(!is_allowed("./cargo", &allowed));
        assert!(!is_allowed("C:\\tools\\cargo.exe", &allowed));
    }
}
