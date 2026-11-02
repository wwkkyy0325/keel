//! End-to-end tests for the composed views a frontend consumes.
//!
//! These run the real binary, so they cover argument parsing, the store and the
//! JSON contract together. A frontend test can use the same commands without
//! starting the UI.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn cli(arguments: &[&str], stdin: Option<&str>) -> Output {
    // The default state root is the user's home; isolate every test so it
    // never writes there. `--store` covers the database, not trash/outputs.
    let state = tempfile::tempdir().expect("state home");
    let mut command = Command::new(env!("CARGO_BIN_EXE_keel-cli"));
    command
        .current_dir(repository_root())
        .env("KEEL_HOME", state.path())
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped());
    let mut child = command.spawn().expect("run keel-cli");
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(input.as_bytes())
            .expect("write stdin");
    }
    let output = child.wait_with_output().expect("wait for keel-cli");
    drop(state);
    output
}

fn session_arguments<'a>(workspace: &'a str, store: &'a str) -> Vec<&'a str> {
    vec![
        "--profile",
        "variants/coding/profile.toml",
        "--workspace",
        workspace,
        "--store",
        store,
        "--no-todo",
        "--session",
    ]
}

fn seed_session(workspace: &Path, store: &Path, session: &str) {
    let mut arguments = session_arguments(workspace.to_str().unwrap(), store.to_str().unwrap());
    arguments.extend(["--session-id", session]);
    let output = cli(&arguments, Some("one\ntwo\n"));
    assert!(
        output.status.success(),
        "seed failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn the_tui_requires_an_interactive_terminal() {
    let output = cli(
        &["--tui", "--profile", "variants/companion/profile.toml"],
        None,
    );

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("interactive terminal"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_session_persists_every_turn_after_the_event_log_is_cleared() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("ws");
    std::fs::create_dir_all(&workspace).unwrap();
    let store = dir.path().join("keel.db");
    seed_session(&workspace, &store, "s");

    let conn = rusqlite::Connection::open(&store).unwrap();
    let turns: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM turns WHERE session_id = 's'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let turns_with_events: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT turn_seq) FROM events WHERE session_id = 's'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(turns, 2);
    assert_eq!(
        turns_with_events, 2,
        "the host clears memory only after the turn is saved"
    );
}

#[test]
fn views_are_emitted_by_the_cli_and_page_turns() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("ws");
    std::fs::create_dir_all(&workspace).unwrap();
    let store = dir.path().join("keel.db");
    seed_session(&workspace, &store, "s");
    let store_text = store.to_str().unwrap();

    let output = cli(&["--store", store_text, "--view", "sessions"], None);
    assert!(output.status.success());
    let sessions: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let list = sessions.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], "s");
    assert_eq!(list[0]["turns"], 2);

    let output = cli(
        &[
            "--store",
            store_text,
            "--view",
            "session",
            "--session-id",
            "s",
        ],
        None,
    );
    assert!(output.status.success());
    let session: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(session["session"]["id"], "s");
    assert_eq!(session["turns"].as_array().unwrap().len(), 2);

    let output = cli(
        &[
            "--store",
            store_text,
            "--view",
            "session",
            "--session-id",
            "s",
            "--limit",
            "1",
        ],
        None,
    );
    assert!(output.status.success());
    let page: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let turns = page["turns"].as_array().unwrap();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0]["seq"], 2);

    let output = cli(
        &[
            "--store",
            store_text,
            "--view",
            "session",
            "--session-id",
            "s",
            "--limit",
            "1",
            "--before",
            "2",
        ],
        None,
    );
    assert!(output.status.success());
    let page: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(page["turns"][0]["seq"], 1);

    let output = cli(
        &[
            "--store",
            store_text,
            "--view",
            "turn",
            "--session-id",
            "s",
            "--turn",
            "1",
        ],
        None,
    );
    assert!(output.status.success());
    let turn: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(turn["turn"]["seq"], 1);
    assert_eq!(turn["turn"]["answer_model"]["body"], turn["turn"]["answer"]);

    let output = cli(
        &[
            "--store",
            store_text,
            "--view",
            "memory",
            "--session-id",
            "s",
        ],
        None,
    );
    assert!(output.status.success());
    let memory: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(memory["facts"].is_array());
    assert!(memory["tasks"].is_array());

    let output = cli(
        &[
            "--store",
            store_text,
            "--view",
            "events",
            "--session-id",
            "s",
        ],
        None,
    );
    assert!(output.status.success());
    let events: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let events = events.as_array().unwrap();
    assert!(!events.is_empty(), "a stored turn persists its events");
    assert!(events.iter().all(|event| event["name"].is_string()));
    assert!(events.iter().any(|event| event["name"] == "turn_started"));
}

#[test]
fn fork_and_delete_run_through_the_cli() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("ws");
    std::fs::create_dir_all(&workspace).unwrap();
    let store = dir.path().join("keel.db");
    seed_session(&workspace, &store, "parent");
    let store_text = store.to_str().unwrap();
    let workspace_text = workspace.to_str().unwrap();

    let mut arguments = session_arguments(workspace_text, store_text);
    arguments.extend(["--session-id", "branch", "--fork-from", "parent:1"]);
    let output = cli(&arguments, Some("three\n"));
    assert!(
        output.status.success(),
        "fork failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = cli(
        &[
            "--store",
            store_text,
            "--view",
            "session",
            "--session-id",
            "branch",
        ],
        None,
    );
    assert!(output.status.success());
    let branch: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(branch["session"]["parent_session_id"], "parent");
    assert_eq!(branch["session"]["fork_turn_seq"], 1);
    assert_eq!(branch["turns"].as_array().unwrap().len(), 2);

    let output = cli(&["--store", store_text, "--delete-session", "branch"], None);
    assert!(output.status.success());
    let output = cli(&["--store", store_text, "--delete-session", "branch"], None);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not found"));
}

#[test]
fn a_missing_session_or_misused_flag_exits_non_zero() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("keel.db");
    let store_text = store.to_str().unwrap();
    // Opening the store creates the schema; no session exists.
    let output = cli(
        &[
            "--store",
            store_text,
            "--view",
            "session",
            "--session-id",
            "missing",
        ],
        None,
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not found"));

    let output = cli(
        &["--store", store_text, "--view", "sessions", "--limit", "2"],
        None,
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--view session"));
}

#[test]
fn plugin_check_json_is_machine_readable() {
    let dir = tempfile::tempdir().unwrap();
    let plugins = dir.path().join("plugins.toml");
    std::fs::write(
        &plugins,
        "[[servers]]\nname = \"broken\"\ncommand = \"definitely-not-a-real-command\"\n",
    )
    .unwrap();
    let output = cli(
        &[
            "--plugins-check",
            "--json",
            "--plugins",
            plugins.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(1));
    let statuses: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(statuses[0]["name"], "broken");
    assert_eq!(statuses[0]["tools"], 0);
    assert!(statuses[0]["error"].is_string());
}
