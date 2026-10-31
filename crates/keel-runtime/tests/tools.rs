use std::path::Path;
use std::sync::Arc;

use keel_core::{
    AccessKind, ArgValue, CancelToken, ErrorCode, Tool, ToolArgs, ToolCallId, ToolCtx, TurnId,
};
use keel_runtime::{CodeSearchTool, ListDirTool, LocalTrashStore, ShellTool, WriteFileTool};

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
    let mut entries = std::fs::read_dir(&trash).unwrap();
    let entry = entries.next().unwrap().unwrap();
    assert!(entries.next().is_none());
    assert_eq!(std::fs::read_to_string(entry.path()).unwrap(), "old");
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
async fn shell_rejects_programs_outside_the_allowlist() {
    let dir = tempfile::tempdir().unwrap();
    let tool = ShellTool::new(vec!["rustc".to_owned()]);
    let mut args = ToolArgs::new();
    args.insert("program", ArgValue::String("not-allowed".to_owned()));

    let result = tool.call(args, &ctx(dir.path())).await;

    assert!(!result.is_success());
    assert_eq!(
        result.error().map(|error| error.code),
        Some(ErrorCode::InvalidArguments)
    );
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
