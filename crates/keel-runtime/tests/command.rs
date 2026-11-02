//! Process behavior that the in-process tests cannot cover: output volume,
//! timeouts and descendant cleanup. Python is used as a portable fixture and
//! skipped when it is not installed.

use std::time::{Duration, Instant};

use keel_core::{CancelToken, ErrorCode};
use keel_runtime::command::{self, CommandSpec};

fn python_available() -> bool {
    std::process::Command::new("python")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn a_large_output_is_truncated_and_bounded() {
    if !python_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let args = vec![
        "-c".to_owned(),
        "import sys; sys.stdout.write('x' * 200000)".to_owned(),
    ];

    let output = command::run(
        CommandSpec {
            program: "python",
            args: &args,
            cwd: dir.path(),
            timeout: Duration::from_secs(10),
        },
        &CancelToken::new(),
    )
    .await
    .unwrap();

    assert_eq!(output.exit_code, Some(0));
    assert!(output.truncated);
    assert!(output.output.contains("bytes omitted"));
    assert!(
        output.output.len() < 140 * 1024,
        "retained {} bytes",
        output.output.len()
    );
}

#[cfg(windows)]
#[tokio::test]
async fn a_windows_command_shim_is_resolved() {
    let installed = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| dir.join("npm.cmd").is_file()))
        .unwrap_or(false);
    if !installed {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let args = vec!["--version".to_owned()];

    let output = command::run(
        CommandSpec {
            program: "npm",
            args: &args,
            cwd: dir.path(),
            timeout: Duration::from_secs(60),
        },
        &CancelToken::new(),
    )
    .await
    .unwrap();

    assert_eq!(output.exit_code, Some(0), "{}", output.output);
}

#[tokio::test]
async fn a_timeout_returns_without_waiting_for_the_command() {
    if !python_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let args = vec!["-c".to_owned(), "import time; time.sleep(30)".to_owned()];
    let started = Instant::now();

    let error = command::run(
        CommandSpec {
            program: "python",
            args: &args,
            cwd: dir.path(),
            timeout: Duration::from_millis(300),
        },
        &CancelToken::new(),
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::Timeout);
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn a_timeout_takes_the_descendants_with_it() {
    if !python_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("spawn_child.py");
    std::fs::write(
        &script,
        "import subprocess, sys, time\n\
         child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])\n\
         with open(sys.argv[1], 'w') as handle:\n\
         \x20   handle.write(str(child.pid))\n\
         time.sleep(60)\n",
    )
    .unwrap();
    let pid_file = dir.path().join("child.pid");
    let args = vec![script.display().to_string(), pid_file.display().to_string()];

    let error = command::run(
        CommandSpec {
            program: "python",
            args: &args,
            cwd: dir.path(),
            timeout: Duration::from_secs(3),
        },
        &CancelToken::new(),
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::Timeout);
    let Some(pid) = descendant_pid(&pid_file).await else {
        panic!("the fixture did not write the descendant pid");
    };
    for _ in 0..30 {
        if !is_alive(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("descendant {pid} survived the timeout");
}

async fn descendant_pid(path: &std::path::Path) -> Option<u32> {
    for _ in 0..50 {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(pid) = text.trim().parse::<u32>() {
                return Some(pid);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    None
}

fn is_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        let filter = format!("PID eq {pid}");
        std::process::Command::new("tasklist")
            .args(["/FI", &filter, "/NH", "/FO", "CSV"])
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\"")))
            .unwrap_or(false)
    }
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}

#[tokio::test]
async fn a_command_that_keeps_printing_outlives_the_inactivity_window() {
    if !python_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let args = vec![
        "-c".to_owned(),
        "import time\nfor _ in range(10):\n    print('tick', flush=True)\n    time.sleep(0.1)"
            .to_owned(),
    ];

    let started = Instant::now();
    let output = command::run(
        CommandSpec {
            program: "python",
            args: &args,
            cwd: dir.path(),
            // Shorter than the command's total run time.
            timeout: Duration::from_millis(400),
        },
        &CancelToken::new(),
    )
    .await
    .unwrap();

    assert_eq!(output.exit_code, Some(0));
    assert!(
        started.elapsed() > Duration::from_millis(500),
        "an active command must not be cut at the inactivity window: {:?}",
        started.elapsed()
    );
}
