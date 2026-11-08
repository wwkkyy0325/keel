use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use keel_core::{
    AccessKind, ArgValue, CancelToken, ErrorCode, Tool, ToolArgs, ToolCallId, ToolCtx, TurnId,
};
use keel_runtime::{
    CodeSearchTool, ListDirTool, ListTrashTool, LocalBackgroundTasks, LocalTrashStore,
    ReadFileTool, RestoreFileTool, ShellTool, TrashStore, WriteFileTool,
};

fn ctx(root: &Path) -> ToolCtx {
    ToolCtx {
        turn_id: TurnId::new(1),
        call_id: ToolCallId::new(1),
        workspace_root: root.to_path_buf(),
        cancel: CancelToken::new(),
    }
}

fn args(path: &str, content: &str) -> ToolArgs {
    let mut args = ToolArgs::new();
    args.insert("path", ArgValue::String(path.to_owned()));
    args.insert("content", ArgValue::String(content.to_owned()));
    args
}

#[tokio::test]
async fn overwriting_backs_up_the_previous_version() {
    let dir = tempfile::tempdir().unwrap();
    let tool = WriteFileTool::new(Arc::new(LocalTrashStore::new(dir.path().to_path_buf())));

    let first = tool.call(args("notes.txt", "old"), &ctx(dir.path())).await;
    assert!(first.is_success());

    let second = tool.call(args("notes.txt", "new"), &ctx(dir.path())).await;
    assert!(second.is_success());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
        "new"
    );

    let trash = dir.path().join(".keel").join("trash");
    let mut files: Vec<_> = std::fs::read_dir(&trash)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.path().is_file())
        .collect();
    assert_eq!(files.len(), 1, "one data file");
    assert_eq!(
        std::fs::read_to_string(files.remove(0).path()).unwrap(),
        "old"
    );
    assert!(trash.join("meta").is_dir(), "the original path is recorded");
}

#[tokio::test]
async fn list_dir_reports_files_and_directories() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    let tool = ListDirTool::new();
    let mut args = ToolArgs::new();
    args.insert("path", ArgValue::String(".".to_owned()));

    let result = tool.call(args, &ctx(dir.path())).await;

    assert!(result.is_success());
    let content = result.content().unwrap();
    assert!(content.contains("f a.txt"));
    assert!(content.contains("d src"));

    let mut permission_args = ToolArgs::new();
    permission_args.insert("path", ArgValue::String(".".to_owned()));
    let request = &tool.permission_requests(&permission_args).unwrap()[0];
    assert_eq!(request.tool, "list_dir");
    assert_eq!(request.access, AccessKind::Read);
}

#[tokio::test]
async fn code_search_finds_matches_and_skips_build_directories() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::create_dir_all(dir.path().join("target")).unwrap();
    std::fs::write(
        dir.path().join("src").join("main.rs"),
        "fn main() {\n    println!(\"Hello World\");\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("target").join("generated.rs"),
        "hello world\n",
    )
    .unwrap();

    let tool = CodeSearchTool::new();
    let mut args = ToolArgs::new();
    args.insert("query", ArgValue::String("hello world".to_owned()));

    let result = tool.call(args, &ctx(dir.path())).await;

    assert!(result.is_success());
    let content = result.content().unwrap();
    assert!(content.contains("src/main.rs:2:"));
    assert!(!content.contains("target/generated.rs"));

    let mut permission_args = ToolArgs::new();
    permission_args.insert("query", ArgValue::String("hello".to_owned()));
    let request = &tool.permission_requests(&permission_args).unwrap()[0];
    assert_eq!(request.tool, "code_search");
    assert_eq!(request.access, AccessKind::Read);
}

#[tokio::test]
async fn read_file_reads_a_normal_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.txt"), "hello").unwrap();
    let tool = ReadFileTool::new();
    let mut args = ToolArgs::new();
    args.insert("path", ArgValue::String("notes.txt".to_owned()));

    let result = tool.call(args, &ctx(dir.path())).await;

    assert!(result.is_success());
    assert_eq!(result.content(), Some("hello"));
}

#[tokio::test]
async fn read_file_truncates_a_file_over_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("huge.log");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(9 * 1024 * 1024).unwrap();
    drop(file);

    let tool = ReadFileTool::new();
    let mut args = ToolArgs::new();
    args.insert("path", ArgValue::String("huge.log".to_owned()));

    let result = tool.call(args, &ctx(dir.path())).await;

    assert!(result.is_success());
    let content = result.content().unwrap();
    assert!(content.contains("[truncated:"), "{}", &content[..80]);
    assert!(content.contains("9437184"), "the marker names the size");
    assert!(content.len() <= 8 * 1024 * 1024 + 128);
}

#[tokio::test]
async fn read_file_returns_a_line_range_without_changing_its_bytes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("code.txt"),
        "one\r\ntwo\r\nthree\r\nfour\r\n",
    )
    .unwrap();
    let tool = ReadFileTool::new();
    let mut args = ToolArgs::new();
    args.insert("path", ArgValue::String("code.txt".to_owned()));
    args.insert("offset", ArgValue::Integer(2));
    args.insert("limit", ArgValue::Integer(2));

    let result = tool.call(args, &ctx(dir.path())).await;

    assert!(result.is_success());
    assert_eq!(result.content(), Some("two\r\nthree\r\n"));
}

#[tokio::test]
async fn read_file_reports_an_offset_past_the_end() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("code.txt"), "one\ntwo\nthree\nfour\n").unwrap();
    let tool = ReadFileTool::new();
    let mut args = ToolArgs::new();
    args.insert("path", ArgValue::String("code.txt".to_owned()));
    args.insert("offset", ArgValue::Integer(9));

    let result = tool.call(args, &ctx(dir.path())).await;

    assert!(result.is_success());
    let content = result.content().unwrap();
    assert!(content.contains("no lines at offset 9"), "{content}");
    assert!(content.contains("4 lines"), "{content}");
}

#[tokio::test]
async fn read_file_rejects_a_zero_offset() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("code.txt"), "one\n").unwrap();
    let tool = ReadFileTool::new();
    let mut args = ToolArgs::new();
    args.insert("path", ArgValue::String("code.txt".to_owned()));
    args.insert("offset", ArgValue::Integer(0));

    let result = tool.call(args, &ctx(dir.path())).await;

    assert_eq!(
        result.error().map(|error| error.code),
        Some(ErrorCode::InvalidArguments)
    );
}

#[tokio::test]
async fn code_search_can_be_case_sensitive_and_glob_filtered() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src").join("main.rs"), "Hello\nhello\n").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "Hello\n").unwrap();

    let tool = CodeSearchTool::new();
    let mut args = ToolArgs::new();
    args.insert("query", ArgValue::String("Hello".to_owned()));
    args.insert("case_sensitive", ArgValue::Boolean(true));
    args.insert("glob", ArgValue::String("*.rs".to_owned()));

    let result = tool.call(args, &ctx(dir.path())).await;

    assert!(result.is_success());
    let content = result.content().unwrap();
    assert!(content.contains("src/main.rs:1:"), "{content}");
    assert!(!content.contains("src/main.rs:2:"), "{content}");
    assert!(!content.contains("notes.txt"), "{content}");
}

#[tokio::test]
async fn code_search_rejects_a_non_boolean_case_flag() {
    let dir = tempfile::tempdir().unwrap();
    let tool = CodeSearchTool::new();
    let mut args = ToolArgs::new();
    args.insert("query", ArgValue::String("hello".to_owned()));
    args.insert("case_sensitive", ArgValue::String("true".to_owned()));

    let result = tool.call(args, &ctx(dir.path())).await;

    assert_eq!(
        result.error().map(|error| error.code),
        Some(ErrorCode::InvalidArguments)
    );
}

#[tokio::test]
async fn code_search_supports_regex() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src").join("main.rs"),
        "fn main() {\n    let value = 1;\n}\n",
    )
    .unwrap();
    let tool = CodeSearchTool::new();
    let mut args = ToolArgs::new();
    args.insert("query", ArgValue::String(r"fn\s+\w+".to_owned()));
    args.insert("regex", ArgValue::Boolean(true));

    let result = tool.call(args, &ctx(dir.path())).await;

    assert!(result.is_success());
    let content = result.content().unwrap();
    assert!(content.contains("src/main.rs:1:"), "{content}");
}

#[tokio::test]
async fn code_search_applies_the_case_flag_to_regex() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "HELLO\n").unwrap();
    let tool = CodeSearchTool::new();

    let mut insensitive = ToolArgs::new();
    insensitive.insert("query", ArgValue::String("hello".to_owned()));
    insensitive.insert("regex", ArgValue::Boolean(true));
    let result = tool.call(insensitive, &ctx(dir.path())).await;
    assert!(result.is_success());
    assert!(result.content().unwrap().contains("main.rs:1:"));

    let mut sensitive = ToolArgs::new();
    sensitive.insert("query", ArgValue::String("hello".to_owned()));
    sensitive.insert("regex", ArgValue::Boolean(true));
    sensitive.insert("case_sensitive", ArgValue::Boolean(true));
    let result = tool.call(sensitive, &ctx(dir.path())).await;
    assert!(result.is_success());
    assert_eq!(result.content(), Some("no matches"));
}

#[tokio::test]
async fn code_search_rejects_an_invalid_regex() {
    let dir = tempfile::tempdir().unwrap();
    let tool = CodeSearchTool::new();
    let mut args = ToolArgs::new();
    args.insert("query", ArgValue::String("(".to_owned()));
    args.insert("regex", ArgValue::Boolean(true));

    let result = tool.call(args, &ctx(dir.path())).await;

    assert_eq!(
        result.error().map(|error| error.code),
        Some(ErrorCode::InvalidArguments)
    );
}

#[tokio::test]
async fn code_search_context_includes_neighbors() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "one\ntwo\nthree\nfour\n").unwrap();
    let tool = CodeSearchTool::new();
    let mut args = ToolArgs::new();
    args.insert("query", ArgValue::String("three".to_owned()));
    args.insert("context", ArgValue::Integer(1));

    let result = tool.call(args, &ctx(dir.path())).await;

    assert!(result.is_success());
    let content = result.content().unwrap();
    assert!(content.contains("main.rs:2- two"), "{content}");
    assert!(content.contains("main.rs:3: three"), "{content}");
    assert!(content.contains("main.rs:4- four"), "{content}");
}

#[tokio::test]
async fn code_search_without_context_keeps_only_the_match() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "one\ntwo\nthree\n").unwrap();
    let tool = CodeSearchTool::new();
    let mut args = ToolArgs::new();
    args.insert("query", ArgValue::String("two".to_owned()));

    let result = tool.call(args, &ctx(dir.path())).await;

    assert!(result.is_success());
    let content = result.content().unwrap();
    assert!(content.contains("main.rs:2: two"), "{content}");
    assert!(!content.contains("main.rs:1-"), "{content}");
    assert!(!content.contains("main.rs:3-"), "{content}");
}

#[tokio::test]
async fn code_search_rejects_a_non_integer_context() {
    let dir = tempfile::tempdir().unwrap();
    let tool = CodeSearchTool::new();
    let mut args = ToolArgs::new();
    args.insert("query", ArgValue::String("two".to_owned()));
    args.insert("context", ArgValue::String("1".to_owned()));

    let result = tool.call(args, &ctx(dir.path())).await;

    assert_eq!(
        result.error().map(|error| error.code),
        Some(ErrorCode::InvalidArguments)
    );
}

#[tokio::test]
async fn shell_runs_allowlisted_programs_without_a_shell() {
    let dir = tempfile::tempdir().unwrap();
    let tool = ShellTool::new(vec!["rustc".to_owned()]);
    let mut args = ToolArgs::new();
    args.insert("program", ArgValue::String("rustc".to_owned()));
    args.insert("args", ArgValue::StringList(vec!["--version".to_owned()]));

    let result = tool.call(args.clone(), &ctx(dir.path())).await;

    assert!(result.is_success());
    let content = result.content().unwrap();
    assert!(content.contains("exit code: 0"));
    assert!(content.contains("rustc"));

    let request = &tool.permission_requests(&args).unwrap()[0];
    assert_eq!(request.tool, "shell");
    assert_eq!(request.access, AccessKind::Execute);
}

#[tokio::test]
async fn an_unknown_program_reaches_the_gate_instead_of_being_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let tool = ShellTool::new(vec!["rustc".to_owned()]);
    let mut args = ToolArgs::new();
    args.insert("program", ArgValue::String("not-allowed".to_owned()));

    let request = &tool.permission_requests(&args).unwrap()[0];
    let detail = request.detail.as_deref().unwrap_or_default();
    assert!(
        detail.contains("not in the standard program list"),
        "{detail}"
    );

    let result = tool.call(args, &ctx(dir.path())).await;
    assert_ne!(
        result.error().map(|error| error.code),
        Some(ErrorCode::InvalidArguments)
    );
}

#[tokio::test]
async fn a_standard_program_has_no_marker() {
    let tool = ShellTool::new(vec!["rustc".to_owned()]);
    let mut args = ToolArgs::new();
    args.insert("program", ArgValue::String("rustc".to_owned()));
    args.insert("args", ArgValue::StringList(vec!["--version".to_owned()]));

    let request = &tool.permission_requests(&args).unwrap()[0];
    let detail = request.detail.as_deref().unwrap_or_default();
    assert!(!detail.contains("not in the standard"), "{detail}");
}

#[tokio::test]
async fn restore_file_restores_the_newest_backup_and_keeps_the_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let trash: Arc<dyn TrashStore> = Arc::new(LocalTrashStore::new(dir.path().to_path_buf()));
    let writer = WriteFileTool::new(trash.clone());
    assert!(writer
        .call(args("notes.txt", "old"), &ctx(dir.path()))
        .await
        .is_success());
    assert!(writer
        .call(args("notes.txt", "new"), &ctx(dir.path()))
        .await
        .is_success());

    let mut restore_args = ToolArgs::new();
    restore_args.insert("path", ArgValue::String("notes.txt".to_owned()));
    let result = RestoreFileTool::new(trash.clone())
        .call(restore_args, &ctx(dir.path()))
        .await;

    assert!(result.is_success(), "{:?}", result.error());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
        "old"
    );
    let entries = trash.list(10).await.unwrap();
    assert!(entries
        .iter()
        .any(|entry| std::fs::read_to_string(&entry.stored).unwrap() == "new"));
}

#[tokio::test]
async fn restore_file_reports_a_missing_backup() {
    let dir = tempfile::tempdir().unwrap();
    let trash: Arc<dyn TrashStore> = Arc::new(LocalTrashStore::new(dir.path().to_path_buf()));
    let mut restore_args = ToolArgs::new();
    restore_args.insert("path", ArgValue::String("missing.txt".to_owned()));

    let result = RestoreFileTool::new(trash)
        .call(restore_args, &ctx(dir.path()))
        .await;

    assert_eq!(
        result.error().map(|error| error.code),
        Some(ErrorCode::NotFound)
    );
}

#[tokio::test]
async fn restore_file_rejects_an_entry_for_another_path() {
    let dir = tempfile::tempdir().unwrap();
    let trash: Arc<dyn TrashStore> = Arc::new(LocalTrashStore::new(dir.path().to_path_buf()));
    let writer = WriteFileTool::new(trash.clone());
    assert!(writer
        .call(args("a.txt", "a old"), &ctx(dir.path()))
        .await
        .is_success());
    assert!(writer
        .call(args("a.txt", "a new"), &ctx(dir.path()))
        .await
        .is_success());
    assert!(writer
        .call(args("b.txt", "b"), &ctx(dir.path()))
        .await
        .is_success());
    let entry = trash
        .list(10)
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.original.ends_with("a.txt"))
        .unwrap();

    let mut restore_args = ToolArgs::new();
    restore_args.insert("path", ArgValue::String("b.txt".to_owned()));
    restore_args.insert("entry", ArgValue::String(entry.id));
    let result = RestoreFileTool::new(trash)
        .call(restore_args, &ctx(dir.path()))
        .await;

    assert_eq!(
        result.error().map(|error| error.code),
        Some(ErrorCode::InvalidArguments)
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "b"
    );
}

#[tokio::test]
async fn list_trash_names_the_backups_and_their_paths() {
    let dir = tempfile::tempdir().unwrap();
    let trash: Arc<dyn TrashStore> = Arc::new(LocalTrashStore::new(dir.path().to_path_buf()));
    let writer = WriteFileTool::new(trash.clone());
    assert!(writer
        .call(args("notes.txt", "old"), &ctx(dir.path()))
        .await
        .is_success());
    assert!(writer
        .call(args("notes.txt", "new"), &ctx(dir.path()))
        .await
        .is_success());

    let result = ListTrashTool::new(trash)
        .call(ToolArgs::new(), &ctx(dir.path()))
        .await;

    assert!(result.is_success());
    let content = result.content().unwrap();
    assert!(content.contains("notes.txt"), "{content}");
}

#[tokio::test]
async fn shell_stops_at_its_own_timeout() {
    let python = std::process::Command::new("python")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !python {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let tool = ShellTool::new(vec!["python".to_owned()]).with_timeout(Duration::from_millis(300));
    let mut args = ToolArgs::new();
    args.insert("program", ArgValue::String("python".to_owned()));
    args.insert(
        "args",
        ArgValue::StringList(vec![
            "-c".to_owned(),
            "import time; time.sleep(30)".to_owned(),
        ]),
    );

    let started = Instant::now();
    let result = tool.call(args, &ctx(dir.path())).await;

    assert_eq!(
        result.error().map(|error| error.code),
        Some(ErrorCode::Timeout)
    );
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn failed_backup_aborts_the_write() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.txt"), "old").unwrap();
    std::fs::create_dir(dir.path().join(".keel")).unwrap();
    std::fs::write(dir.path().join(".keel").join("trash"), "blocker").unwrap();

    let tool = WriteFileTool::new(Arc::new(LocalTrashStore::new(dir.path().to_path_buf())));
    let result = tool.call(args("notes.txt", "new"), &ctx(dir.path())).await;

    assert!(!result.is_success());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
        "old"
    );
}

#[tokio::test]
async fn writing_creates_missing_parent_directories() {
    let dir = tempfile::tempdir().unwrap();
    let tool = WriteFileTool::new(Arc::new(LocalTrashStore::new(dir.path().to_path_buf())));

    let result = tool
        .call(
            args("tests/test_calc.py", "print('ok')\n"),
            &ctx(dir.path()),
        )
        .await;

    assert!(result.is_success(), "{result:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("tests").join("test_calc.py")).unwrap(),
        "print('ok')\n"
    );
}

#[tokio::test]
async fn shell_background_starts_a_command_and_returns_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let tasks = Arc::new(LocalBackgroundTasks::new(dir.path().join("outputs")));
    let tool = ShellTool::new(vec!["python".to_owned()]).with_background(tasks);
    let mut args = ToolArgs::new();
    args.insert("program", ArgValue::String("python".to_owned()));
    args.insert(
        "args",
        ArgValue::StringList(vec![
            "-c".to_owned(),
            "import time; time.sleep(0.2)".to_owned(),
        ]),
    );
    args.insert("background", ArgValue::String("true".to_owned()));

    let result = tool.call(args, &ctx(dir.path())).await;

    let content = result.content().unwrap_or_default();
    assert!(content.contains("started bg-1"), "{content}");
    assert!(content.contains("continues in the background"), "{content}");
}

#[tokio::test]
async fn shell_background_without_a_registry_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let tool = ShellTool::new(vec!["python".to_owned()]);
    let mut args = ToolArgs::new();
    args.insert("program", ArgValue::String("python".to_owned()));
    args.insert("background", ArgValue::String("true".to_owned()));

    let result = tool.call(args, &ctx(dir.path())).await;

    assert!(!result.is_success());
    assert!(result
        .error()
        .is_some_and(|error| error.message.contains("background execution")));
}
