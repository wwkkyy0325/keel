use std::path::Path;
use std::sync::Arc;

use keel_core::{ArgValue, CancelToken, Tool, ToolArgs, ToolCallId, ToolCtx, TurnId};
use keel_runtime::{LocalTrashStore, WriteFileTool};

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
