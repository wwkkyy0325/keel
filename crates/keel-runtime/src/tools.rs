use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use keel_core::{
    resolve_path, AccessKind, ArgValue, ParamKind, ParamSpec, PermissionRequest, RiskLevel,
    TodoError, TodoList, TodoState, TodoStatus, Tool, ToolArgs, ToolCtx, ToolError, ToolResult,
    ToolSchema,
};
use tokio::io::AsyncReadExt;

use crate::background::BackgroundTasks;
use crate::command::{self, CommandSpec};
use crate::trash::TrashStore;

/// Returns the argument text unchanged. Used to exercise the tool loop.
pub struct EchoTool;

impl EchoTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EchoTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for EchoTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "echo".to_owned(),
            description: "Return the provided text unchanged.".to_owned(),
            risk: RiskLevel::ReadOnly,
            params: vec![ParamSpec {
                name: "text".to_owned(),
                kind: ParamKind::String,
                required: true,
            }],
        }
    }

    fn permission_requests(&self, _args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError> {
        Ok(Vec::new())
    }

    async fn call(&self, args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        if ctx.cancel.is_cancelled() {
            return ToolResult::failure(ctx.call_id, "echo", ToolError::cancelled());
        }
        match args.require_str("text") {
            Ok(text) => ToolResult::success(ctx.call_id, "echo", text),
            Err(error) => ToolResult::failure(ctx.call_id, "echo", error),
        }
    }
}

/// Files above this size are refused instead of being read into memory.
const MAX_READ_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// Reads a UTF-8 text file from the workspace.
pub struct ReadFileTool;

impl ReadFileTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReadFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "read_file".to_owned(),
            description: "Read a UTF-8 text file from the workspace.".to_owned(),
            risk: RiskLevel::ReadOnly,
            params: vec![ParamSpec {
                name: "path".to_owned(),
                kind: ParamKind::String,
                required: true,
            }],
        }
    }

    fn permission_requests(&self, args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError> {
        let path = args.require_str("path")?;
        Ok(vec![PermissionRequest {
            tool: "read_file".to_owned(),
            access: AccessKind::Read,
            path: Some(PathBuf::from(path)),
            detail: None,
        }])
    }

    async fn call(&self, args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        if ctx.cancel.is_cancelled() {
            return ToolResult::failure(ctx.call_id, "read_file", ToolError::cancelled());
        }
        let path = match args.require_str("path") {
            Ok(path) => path,
            Err(error) => return ToolResult::failure(ctx.call_id, "read_file", error),
        };
        let resolved = resolve_path(&ctx.workspace_root, Path::new(path));
        // Bounded read: a log or archive larger than the limit yields its head
        // and a marker instead of an error, so the model can still work with
        // it without reading gigabytes into memory.
        let len = tokio::fs::metadata(&resolved)
            .await
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let truncated = len > MAX_READ_FILE_BYTES;
        let mut file = match tokio::fs::File::open(&resolved).await {
            Ok(file) => file,
            Err(error) => {
                return ToolResult::failure(ctx.call_id, "read_file", io_error(&error));
            }
        };
        let mut buffer = Vec::with_capacity(len.min(MAX_READ_FILE_BYTES) as usize);
        if let Err(error) = (&mut file)
            .take(MAX_READ_FILE_BYTES)
            .read_to_end(&mut buffer)
            .await
        {
            return ToolResult::failure(ctx.call_id, "read_file", io_error(&error));
        }
        let mut content = match String::from_utf8(buffer) {
            Ok(text) => text,
            Err(error) if truncated => {
                // The cut may land inside a character; keep the valid prefix.
                let valid = error.utf8_error().valid_up_to();
                String::from_utf8_lossy(&error.as_bytes()[..valid]).into_owned()
            }
            Err(_) => {
                return ToolResult::failure(
                    ctx.call_id,
                    "read_file",
                    ToolError::io(format!("`{path}` is not valid UTF-8")),
                );
            }
        };
        if truncated {
            content.push_str(&format!(
                "\n\n[truncated: the file is {len} bytes; showing the first \
                 {MAX_READ_FILE_BYTES} bytes]"
            ));
        }
        ToolResult::success(ctx.call_id, "read_file", content)
    }
}

/// Creates a directory and any missing parents.
pub struct MakeDirTool;

impl MakeDirTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MakeDirTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for MakeDirTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "make_dir".to_owned(),
            description: "Create a directory and any missing parents.".to_owned(),
            risk: RiskLevel::Mutating,
            params: vec![ParamSpec {
                name: "path".to_owned(),
                kind: ParamKind::String,
                required: true,
            }],
        }
    }

    fn permission_requests(&self, args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError> {
        let path = args.require_str("path")?;
        Ok(vec![PermissionRequest {
            tool: "make_dir".to_owned(),
            access: AccessKind::Write,
            path: Some(PathBuf::from(path)),
            detail: None,
        }])
    }

    async fn call(&self, args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        if ctx.cancel.is_cancelled() {
            return ToolResult::failure(ctx.call_id, "make_dir", ToolError::cancelled());
        }
        let path = match args.require_str("path") {
            Ok(path) => path,
            Err(error) => return ToolResult::failure(ctx.call_id, "make_dir", error),
        };
        let resolved = resolve_path(&ctx.workspace_root, Path::new(path));

        match tokio::fs::metadata(&resolved).await {
            Ok(metadata) if metadata.is_dir() => ToolResult::success(
                ctx.call_id,
                "make_dir",
                format!("{} already exists", resolved.display()),
            ),
            Ok(_) => ToolResult::failure(
                ctx.call_id,
                "make_dir",
                ToolError::invalid_arguments(format!(
                    "`{}` exists and is not a directory",
                    resolved.display()
                )),
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match tokio::fs::create_dir_all(&resolved).await {
                    Ok(()) => ToolResult::success(
                        ctx.call_id,
                        "make_dir",
                        format!("created {}", resolved.display()),
                    ),
                    Err(error) => ToolResult::failure(ctx.call_id, "make_dir", io_error(&error)),
                }
            }
            Err(error) => ToolResult::failure(ctx.call_id, "make_dir", io_error(&error)),
        }
    }
}

/// Deletes one regular file after copying it into the trash.
pub struct DeleteFileTool {
    trash: Arc<dyn TrashStore>,
}

impl DeleteFileTool {
    pub fn new(trash: Arc<dyn TrashStore>) -> Self {
        Self { trash }
    }
}

#[async_trait]
impl Tool for DeleteFileTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "delete_file".to_owned(),
            description: "Delete one regular file; it is moved into the trash first.".to_owned(),
            risk: RiskLevel::Mutating,
            params: vec![ParamSpec {
                name: "path".to_owned(),
                kind: ParamKind::String,
                required: true,
            }],
        }
    }

    fn permission_requests(&self, args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError> {
        let path = args.require_str("path")?;
        Ok(vec![PermissionRequest {
            tool: "delete_file".to_owned(),
            access: AccessKind::Write,
            path: Some(PathBuf::from(path)),
            detail: None,
        }])
    }

    async fn call(&self, args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        if ctx.cancel.is_cancelled() {
            return ToolResult::failure(ctx.call_id, "delete_file", ToolError::cancelled());
        }
        let path = match args.require_str("path") {
            Ok(path) => path,
            Err(error) => return ToolResult::failure(ctx.call_id, "delete_file", error),
        };
        let resolved = resolve_path(&ctx.workspace_root, Path::new(path));

        let metadata = match tokio::fs::symlink_metadata(&resolved).await {
            Ok(metadata) => metadata,
            Err(error) => return ToolResult::failure(ctx.call_id, "delete_file", io_error(&error)),
        };
        if metadata.file_type().is_symlink() {
            return ToolResult::failure(
                ctx.call_id,
                "delete_file",
                ToolError::invalid_arguments(format!(
                    "`{}` is a symlink; delete its target explicitly",
                    resolved.display()
                )),
            );
        }
        if metadata.is_dir() {
            return ToolResult::failure(
                ctx.call_id,
                "delete_file",
                ToolError::invalid_arguments(format!(
                    "`{}` is a directory; delete_file removes regular files only",
                    resolved.display()
                )),
            );
        }

        match self.trash.discard(&resolved).await {
            Ok(entry) => ToolResult::success(
                ctx.call_id,
                "delete_file",
                format!("deleted {} (trash entry {})", resolved.display(), entry.id),
            ),
            Err(error) => ToolResult::failure(ctx.call_id, "delete_file", error),
        }
    }
}

/// Moves one file or directory on the same volume.
pub struct MoveFileTool {
    trash: Arc<dyn TrashStore>,
}

impl MoveFileTool {
    pub fn new(trash: Arc<dyn TrashStore>) -> Self {
        Self { trash }
    }
}

#[async_trait]
impl Tool for MoveFileTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "move_file".to_owned(),
            description: "Move a file or directory; an existing destination file is backed up \
                          first. Moves stay on one volume."
                .to_owned(),
            risk: RiskLevel::Mutating,
            params: vec![
                ParamSpec {
                    name: "from".to_owned(),
                    kind: ParamKind::String,
                    required: true,
                },
                ParamSpec {
                    name: "to".to_owned(),
                    kind: ParamKind::String,
                    required: true,
                },
            ],
        }
    }

    fn permission_requests(&self, args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError> {
        let from = args.require_str("from")?;
        let to = args.require_str("to")?;
        Ok(vec![
            PermissionRequest {
                tool: "move_file".to_owned(),
                access: AccessKind::Write,
                path: Some(PathBuf::from(from)),
                detail: Some("source".to_owned()),
            },
            PermissionRequest {
                tool: "move_file".to_owned(),
                access: AccessKind::Write,
                path: Some(PathBuf::from(to)),
                detail: Some("destination".to_owned()),
            },
        ])
    }

    async fn call(&self, args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        if ctx.cancel.is_cancelled() {
            return ToolResult::failure(ctx.call_id, "move_file", ToolError::cancelled());
        }
        let from = match args.require_str("from") {
            Ok(path) => path,
            Err(error) => return ToolResult::failure(ctx.call_id, "move_file", error),
        };
        let to = match args.require_str("to") {
            Ok(path) => path,
            Err(error) => return ToolResult::failure(ctx.call_id, "move_file", error),
        };
        let source = resolve_path(&ctx.workspace_root, Path::new(from));
        let destination = resolve_path(&ctx.workspace_root, Path::new(to));

        let source_meta = match tokio::fs::symlink_metadata(&source).await {
            Ok(metadata) => metadata,
            Err(error) => return ToolResult::failure(ctx.call_id, "move_file", io_error(&error)),
        };
        if source_meta.file_type().is_symlink() {
            return ToolResult::failure(
                ctx.call_id,
                "move_file",
                ToolError::invalid_arguments(format!(
                    "`{}` is a symlink; move its target explicitly",
                    source.display()
                )),
            );
        }

        let backup = match tokio::fs::symlink_metadata(&destination).await {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return ToolResult::failure(
                    ctx.call_id,
                    "move_file",
                    ToolError::invalid_arguments(format!(
                        "`{}` is a symlink; choose another destination",
                        destination.display()
                    )),
                )
            }
            Ok(metadata) if metadata.is_dir() => {
                return ToolResult::failure(
                    ctx.call_id,
                    "move_file",
                    ToolError::invalid_arguments(format!(
                        "`{}` is a directory; refusing to overwrite it",
                        destination.display()
                    )),
                )
            }
            Ok(_) => match self.trash.backup(&destination).await {
                Ok(entry) => Some(entry),
                Err(error) => return ToolResult::failure(ctx.call_id, "move_file", error),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return ToolResult::failure(ctx.call_id, "move_file", io_error(&error));
            }
        };

        if let Some(parent) = destination.parent() {
            if let Err(error) = tokio::fs::create_dir_all(parent).await {
                return ToolResult::failure(ctx.call_id, "move_file", io_error(&error));
            }
        }

        match tokio::fs::rename(&source, &destination).await {
            Ok(()) => {
                let note = backup
                    .map(|entry| format!(" (previous destination in trash as {})", entry.id))
                    .unwrap_or_default();
                ToolResult::success(
                    ctx.call_id,
                    "move_file",
                    format!(
                        "moved {} to {}{note}",
                        source.display(),
                        destination.display()
                    ),
                )
            }
            Err(error) => ToolResult::failure(ctx.call_id, "move_file", io_error(&error)),
        }
    }
}

/// Lists one directory without following symlinks.
pub struct ListDirTool;

impl ListDirTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ListDirTool {
    fn default() -> Self {
        Self::new()
    }
}

const DEFAULT_DIR_ENTRIES: i64 = 200;
const MAX_DIR_ENTRIES: i64 = 1000;
const MAX_DIR_OUTPUT_BYTES: usize = 32 * 1024;
const DEFAULT_SEARCH_RESULTS: i64 = 100;
const MAX_SEARCH_RESULTS: i64 = 500;
const MAX_SEARCH_FILE_BYTES: u64 = 1024 * 1024;
const MAX_SEARCH_DEPTH: usize = 32;
const MAX_SEARCH_LINE_CHARS: usize = 240;
const MAX_SEARCH_OUTPUT_BYTES: usize = 32 * 1024;

#[async_trait]
impl Tool for ListDirTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "list_dir".to_owned(),
            description: "List entries in a workspace directory, without following symlinks."
                .to_owned(),
            risk: RiskLevel::ReadOnly,
            params: vec![
                ParamSpec {
                    name: "path".to_owned(),
                    kind: ParamKind::String,
                    required: true,
                },
                ParamSpec {
                    name: "max_entries".to_owned(),
                    kind: ParamKind::Integer,
                    required: false,
                },
            ],
        }
    }

    fn permission_requests(&self, args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError> {
        let path = args.require_str("path")?;
        Ok(vec![PermissionRequest {
            tool: "list_dir".to_owned(),
            access: AccessKind::Read,
            path: Some(PathBuf::from(path)),
            detail: None,
        }])
    }

    async fn call(&self, args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        if ctx.cancel.is_cancelled() {
            return ToolResult::failure(ctx.call_id, "list_dir", ToolError::cancelled());
        }
        let path = match args.require_str("path") {
            Ok(path) => path,
            Err(error) => return ToolResult::failure(ctx.call_id, "list_dir", error),
        };
        let limit = match args.get("max_entries") {
            Some(keel_core::ArgValue::Integer(value)) => {
                (*value).clamp(1, MAX_DIR_ENTRIES) as usize
            }
            Some(_) => {
                return ToolResult::failure(
                    ctx.call_id,
                    "list_dir",
                    ToolError::invalid_arguments("argument `max_entries` must be integer"),
                )
            }
            None => DEFAULT_DIR_ENTRIES as usize,
        };
        let resolved = resolve_path(&ctx.workspace_root, Path::new(path));

        let mut entries = match tokio::fs::read_dir(&resolved).await {
            Ok(entries) => entries,
            Err(error) => return ToolResult::failure(ctx.call_id, "list_dir", io_error(&error)),
        };
        let mut names = Vec::new();
        loop {
            let entry = match entries.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(error) => {
                    return ToolResult::failure(ctx.call_id, "list_dir", io_error(&error))
                }
            };
            let kind = match entry.file_type().await {
                Ok(file_type) if file_type.is_symlink() => "l",
                Ok(file_type) if file_type.is_dir() => "d",
                Ok(_) => "f",
                Err(_) => "?",
            };
            names.push(format!("{kind} {}", entry.file_name().to_string_lossy()));
            if names.len() >= limit {
                break;
            }
        }
        names.sort();

        let mut output = String::new();
        for name in &names {
            if output.len() + name.len() + 1 > MAX_DIR_OUTPUT_BYTES {
                output.push_str("... output truncated\n");
                break;
            }
            output.push_str(name);
            output.push('\n');
        }
        if output.is_empty() {
            output.push_str("(empty directory)\n");
        }
        ToolResult::success(ctx.call_id, "list_dir", output)
    }
}

/// Searches UTF-8 files for a case-insensitive substring.
pub struct CodeSearchTool;

impl CodeSearchTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodeSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for CodeSearchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "code_search".to_owned(),
            description: "Search UTF-8 files for a case-insensitive substring.".to_owned(),
            risk: RiskLevel::ReadOnly,
            params: vec![
                ParamSpec {
                    name: "query".to_owned(),
                    kind: ParamKind::String,
                    required: true,
                },
                ParamSpec {
                    name: "path".to_owned(),
                    kind: ParamKind::String,
                    required: false,
                },
                ParamSpec {
                    name: "max_results".to_owned(),
                    kind: ParamKind::Integer,
                    required: false,
                },
            ],
        }
    }

    fn permission_requests(&self, args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError> {
        let path = match args.get("path") {
            Some(keel_core::ArgValue::String(value)) => value.as_str(),
            Some(_) => {
                return Err(ToolError::invalid_arguments(
                    "argument `path` must be string",
                ))
            }
            None => ".",
        };
        Ok(vec![PermissionRequest {
            tool: "code_search".to_owned(),
            access: AccessKind::Read,
            path: Some(PathBuf::from(path)),
            detail: None,
        }])
    }

    async fn call(&self, args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        if ctx.cancel.is_cancelled() {
            return ToolResult::failure(ctx.call_id, "code_search", ToolError::cancelled());
        }
        let query = match args.require_str("query") {
            Ok(query) if !query.is_empty() => query.to_lowercase(),
            Ok(_) => {
                return ToolResult::failure(
                    ctx.call_id,
                    "code_search",
                    ToolError::invalid_arguments("argument `query` must not be empty"),
                )
            }
            Err(error) => return ToolResult::failure(ctx.call_id, "code_search", error),
        };
        let path = match args.get("path") {
            Some(keel_core::ArgValue::String(value)) => value.as_str(),
            Some(_) => {
                return ToolResult::failure(
                    ctx.call_id,
                    "code_search",
                    ToolError::invalid_arguments("argument `path` must be string"),
                )
            }
            None => ".",
        };
        let limit = match args.get("max_results") {
            Some(keel_core::ArgValue::Integer(value)) => {
                (*value).clamp(1, MAX_SEARCH_RESULTS) as usize
            }
            Some(_) => {
                return ToolResult::failure(
                    ctx.call_id,
                    "code_search",
                    ToolError::invalid_arguments("argument `max_results` must be integer"),
                )
            }
            None => DEFAULT_SEARCH_RESULTS as usize,
        };
        let root = resolve_path(&ctx.workspace_root, Path::new(path));

        let mut stack = vec![(root.clone(), 0usize)];
        let mut matches: Vec<String> = Vec::new();
        let mut truncated = false;
        while let Some((directory, depth)) = stack.pop() {
            if matches.len() >= limit {
                truncated = true;
                break;
            }
            let mut entries = match tokio::fs::read_dir(&directory).await {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                if ctx.cancel.is_cancelled() {
                    return ToolResult::failure(ctx.call_id, "code_search", ToolError::cancelled());
                }
                let file_type = match entry.file_type().await {
                    Ok(file_type) => file_type,
                    Err(_) => continue,
                };
                if file_type.is_symlink() {
                    continue;
                }
                let entry_path = entry.path();
                if file_type.is_dir() {
                    if depth < MAX_SEARCH_DEPTH && !is_skipped_directory(&entry_path) {
                        stack.push((entry_path, depth + 1));
                    }
                    continue;
                }
                if !file_type.is_file() {
                    continue;
                }
                let metadata = match entry.metadata().await {
                    Ok(metadata) => metadata,
                    Err(_) => continue,
                };
                if metadata.len() > MAX_SEARCH_FILE_BYTES {
                    continue;
                }
                let bytes = match tokio::fs::read(&entry_path).await {
                    Ok(bytes) => bytes,
                    Err(_) => continue,
                };
                let Ok(text) = std::str::from_utf8(&bytes) else {
                    continue;
                };
                if text.contains('\0') {
                    continue;
                }
                let relative = entry_path
                    .strip_prefix(&ctx.workspace_root)
                    .unwrap_or(&entry_path);
                for (index, line) in text.lines().enumerate() {
                    if matches.len() >= limit {
                        truncated = true;
                        break;
                    }
                    if line.to_lowercase().contains(&query) {
                        let line = truncate_chars(line.trim(), MAX_SEARCH_LINE_CHARS);
                        matches.push(format!(
                            "{}:{}: {line}",
                            display_relative(relative),
                            index + 1
                        ));
                    }
                }
                if truncated {
                    break;
                }
            }
        }

        matches.sort();
        let mut output = matches.join("\n");
        if output.len() > MAX_SEARCH_OUTPUT_BYTES {
            output.truncate(char_boundary(&output, MAX_SEARCH_OUTPUT_BYTES));
            truncated = true;
        }
        if output.is_empty() {
            output.push_str("no matches");
        }
        if truncated {
            output.push_str(&format!("\n... stopped after {limit} matches"));
        }
        ToolResult::success(ctx.call_id, "code_search", output)
    }
}

fn is_skipped_directory(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(
            ".git"
                | ".keel"
                | "target"
                | "node_modules"
                | "dist"
                | "build"
                | ".venv"
                | "__pycache__"
                | ".idea"
                | ".vscode"
        )
    )
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let mut truncated: String = text.chars().take(limit).collect();
    truncated.push_str("...");
    truncated
}

fn char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn display_relative(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Runs an allowlisted program without a shell.
pub struct ShellTool {
    allowlist: Vec<String>,
    timeout: Duration,
    background: Option<Arc<dyn BackgroundTasks>>,
}

impl ShellTool {
    pub fn new(allowlist: Vec<String>) -> Self {
        Self {
            allowlist,
            timeout: Duration::from_secs(60),
            background: None,
        }
    }

    /// Developer programs useful for a coding profile. Shells are excluded so
    /// the tool cannot be turned into an interpolating command string.
    pub fn coding_defaults() -> Self {
        Self::new(command::coding_allowlist())
    }

    /// The host sets this from the same limit the dispatcher enforces, so a
    /// build can be given more time without editing the profile.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Enables `background: true`; the host owns the process registry.
    pub fn with_background(mut self, tasks: Arc<dyn BackgroundTasks>) -> Self {
        self.background = Some(tasks);
        self
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "shell".to_owned(),
            description: "Run a program with a fixed argument list; no shell. Common developer \
                          programs are standard; any other program still runs after the user \
                          confirms it."
                .to_owned(),
            risk: RiskLevel::Dangerous,
            params: vec![
                ParamSpec {
                    name: "program".to_owned(),
                    kind: ParamKind::String,
                    required: true,
                },
                ParamSpec {
                    name: "args".to_owned(),
                    kind: ParamKind::StringList,
                    required: false,
                },
                ParamSpec {
                    name: "cwd".to_owned(),
                    kind: ParamKind::String,
                    required: false,
                },
                ParamSpec {
                    name: "background".to_owned(),
                    kind: ParamKind::String,
                    required: false,
                },
            ],
        }
    }

    fn permission_requests(&self, args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError> {
        let path = match args.get("cwd") {
            Some(keel_core::ArgValue::String(value)) => value.as_str(),
            Some(_) => {
                return Err(ToolError::invalid_arguments(
                    "argument `cwd` must be string",
                ))
            }
            None => ".",
        };
        let program = args.require_str("program")?;
        let mut detail = match args.get("args") {
            Some(keel_core::ArgValue::StringList(values)) if !values.is_empty() => {
                format!("{program} {}", values.join(" "))
            }
            _ => program.to_owned(),
        };
        // The list labels a program, it does not veto it: an unknown binary
        // still runs, but the confirmation says why it stands out.
        if !command::is_allowed(program, &self.allowlist) {
            detail.push_str(" (not in the standard program list)");
        }
        Ok(vec![PermissionRequest {
            tool: "shell".to_owned(),
            access: AccessKind::Execute,
            path: Some(PathBuf::from(path)),
            detail: Some(detail),
        }])
    }

    async fn call(&self, args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        if ctx.cancel.is_cancelled() {
            return ToolResult::failure(ctx.call_id, "shell", ToolError::cancelled());
        }
        let program = match args.require_str("program") {
            Ok(program) => program,
            Err(error) => return ToolResult::failure(ctx.call_id, "shell", error),
        };
        let command_args = match args.get("args") {
            Some(keel_core::ArgValue::StringList(values)) => values.clone(),
            Some(_) => {
                return ToolResult::failure(
                    ctx.call_id,
                    "shell",
                    ToolError::invalid_arguments("argument `args` must be a string list"),
                )
            }
            None => Vec::new(),
        };
        let cwd = match args.get("cwd") {
            Some(keel_core::ArgValue::String(value)) => {
                resolve_path(&ctx.workspace_root, Path::new(value))
            }
            Some(_) => {
                return ToolResult::failure(
                    ctx.call_id,
                    "shell",
                    ToolError::invalid_arguments("argument `cwd` must be string"),
                )
            }
            None => ctx.workspace_root.clone(),
        };
        let spec = CommandSpec {
            program,
            args: &command_args,
            cwd: &cwd,
            timeout: self.timeout,
        };
        let background = matches!(
            args.get("background"),
            Some(keel_core::ArgValue::String(value)) if value.eq_ignore_ascii_case("true")
        );
        if background {
            let Some(tasks) = &self.background else {
                return ToolResult::failure(
                    ctx.call_id,
                    "shell",
                    ToolError::invalid_arguments("background execution is not available here"),
                );
            };
            return match tasks.start(&spec) {
                Ok(message) => ToolResult::success(
                    ctx.call_id,
                    "shell",
                    format!(
                        "{message}
the command continues in the background; its result arrives before the next model call"
                    ),
                ),
                Err(error) => ToolResult::failure(ctx.call_id, "shell", error),
            };
        }
        match command::run(spec, &ctx.cancel).await {
            Ok(output) => ToolResult::success(ctx.call_id, "shell", output.output),
            Err(error) => ToolResult::failure(ctx.call_id, "shell", error),
        }
    }
}

/// Writes a UTF-8 text file inside the workspace.
///
/// When the target already exists, the previous version is copied into the
/// trash first. A failed backup aborts the write; there is no hard-delete
/// fallback.
pub struct WriteFileTool {
    trash: Arc<dyn TrashStore>,
}

impl WriteFileTool {
    pub fn new(trash: Arc<dyn TrashStore>) -> Self {
        Self { trash }
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "write_file".to_owned(),
            description: "Write a UTF-8 text file in the workspace.".to_owned(),
            risk: RiskLevel::Mutating,
            params: vec![
                ParamSpec {
                    name: "path".to_owned(),
                    kind: ParamKind::String,
                    required: true,
                },
                ParamSpec {
                    name: "content".to_owned(),
                    kind: ParamKind::String,
                    required: true,
                },
            ],
        }
    }

    fn permission_requests(&self, args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError> {
        let path = args.require_str("path")?;
        Ok(vec![PermissionRequest {
            tool: "write_file".to_owned(),
            access: AccessKind::Write,
            path: Some(PathBuf::from(path)),
            detail: None,
        }])
    }

    async fn call(&self, args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        if ctx.cancel.is_cancelled() {
            return ToolResult::failure(ctx.call_id, "write_file", ToolError::cancelled());
        }
        let path = match args.require_str("path") {
            Ok(path) => path,
            Err(error) => return ToolResult::failure(ctx.call_id, "write_file", error),
        };
        let content = match args.require_str("content") {
            Ok(content) => content,
            Err(error) => return ToolResult::failure(ctx.call_id, "write_file", error),
        };
        let resolved = resolve_path(&ctx.workspace_root, Path::new(path));

        let backup = match tokio::fs::metadata(&resolved).await {
            Ok(metadata) if metadata.is_file() => match self.trash.backup(&resolved).await {
                Ok(entry) => Some(entry),
                Err(error) => return ToolResult::failure(ctx.call_id, "write_file", error),
            },
            Ok(_) => {
                return ToolResult::failure(
                    ctx.call_id,
                    "write_file",
                    ToolError::invalid_arguments(format!(
                        "`{}` is not a regular file",
                        resolved.display()
                    )),
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return ToolResult::failure(ctx.call_id, "write_file", io_error(&error));
            }
        };

        if let Some(parent) = resolved.parent() {
            if let Err(error) = tokio::fs::create_dir_all(parent).await {
                return ToolResult::failure(ctx.call_id, "write_file", io_error(&error));
            }
        }

        match tokio::fs::write(&resolved, content).await {
            Ok(()) => {
                let message = match backup {
                    Some(entry) => format!(
                        "wrote {} bytes to {} (previous version in trash as {})",
                        content.len(),
                        resolved.display(),
                        entry.id
                    ),
                    None => format!("wrote {} bytes to {}", content.len(), resolved.display()),
                };
                ToolResult::success(ctx.call_id, "write_file", message)
            }
            Err(error) => ToolResult::failure(ctx.call_id, "write_file", io_error(&error)),
        }
    }
}

/// Lists recoverable versions in the workspace trash, newest first.
pub struct ListTrashTool {
    trash: Arc<dyn TrashStore>,
}

impl ListTrashTool {
    pub fn new(trash: Arc<dyn TrashStore>) -> Self {
        Self { trash }
    }
}

#[async_trait]
impl Tool for ListTrashTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "list_trash".to_owned(),
            description: "List recoverable file versions in the workspace trash, newest first. \
                          Pass the path to restore_file, or the entry id for one version."
                .to_owned(),
            risk: RiskLevel::ReadOnly,
            params: vec![ParamSpec {
                name: "limit".to_owned(),
                kind: ParamKind::Integer,
                required: false,
            }],
        }
    }

    fn permission_requests(&self, _args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError> {
        Ok(Vec::new())
    }

    async fn call(&self, args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        if ctx.cancel.is_cancelled() {
            return ToolResult::failure(ctx.call_id, "list_trash", ToolError::cancelled());
        }
        let limit = match args.get("limit") {
            Some(ArgValue::Integer(value)) => (*value).clamp(1, 100) as usize,
            Some(_) => {
                return ToolResult::failure(
                    ctx.call_id,
                    "list_trash",
                    ToolError::invalid_arguments("argument `limit` must be integer"),
                )
            }
            None => 20,
        };
        match self.trash.list(limit).await {
            Ok(entries) if entries.is_empty() => {
                ToolResult::success(ctx.call_id, "list_trash", "no backups")
            }
            Ok(entries) => {
                let mut text = String::new();
                for entry in entries {
                    text.push_str(&format!("{} {}\n", entry.id, entry.original.display()));
                }
                ToolResult::success(ctx.call_id, "list_trash", text)
            }
            Err(error) => ToolResult::failure(ctx.call_id, "list_trash", error),
        }
    }
}

/// Restores one file version from the trash.
///
/// The current file is backed up before the copy, so a restore is itself
/// reversible. The target path is the one the permission gate approved; a
/// trash entry is only accepted when it was recorded for that same path.
pub struct RestoreFileTool {
    trash: Arc<dyn TrashStore>,
}

impl RestoreFileTool {
    pub fn new(trash: Arc<dyn TrashStore>) -> Self {
        Self { trash }
    }
}

#[async_trait]
impl Tool for RestoreFileTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "restore_file".to_owned(),
            description: "Restore a file from the workspace trash. Uses the newest backup for \
                          `path`, or the backup with the given `entry` id from list_trash. The \
                          current version is backed up first."
                .to_owned(),
            risk: RiskLevel::Mutating,
            params: vec![
                ParamSpec {
                    name: "path".to_owned(),
                    kind: ParamKind::String,
                    required: true,
                },
                ParamSpec {
                    name: "entry".to_owned(),
                    kind: ParamKind::String,
                    required: false,
                },
            ],
        }
    }

    fn permission_requests(&self, args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError> {
        let path = args.require_str("path")?;
        Ok(vec![PermissionRequest {
            tool: "restore_file".to_owned(),
            access: AccessKind::Write,
            path: Some(PathBuf::from(path)),
            detail: None,
        }])
    }

    async fn call(&self, args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        if ctx.cancel.is_cancelled() {
            return ToolResult::failure(ctx.call_id, "restore_file", ToolError::cancelled());
        }
        let path = match args.require_str("path") {
            Ok(path) => path,
            Err(error) => return ToolResult::failure(ctx.call_id, "restore_file", error),
        };
        let target = resolve_path(&ctx.workspace_root, Path::new(path));
        let entry = match args.get("entry") {
            Some(ArgValue::String(id)) => match self.trash.find(id).await {
                Ok(entry) => entry,
                Err(error) => return ToolResult::failure(ctx.call_id, "restore_file", error),
            },
            Some(_) => {
                return ToolResult::failure(
                    ctx.call_id,
                    "restore_file",
                    ToolError::invalid_arguments("argument `entry` must be string"),
                )
            }
            // Restore needs the newest backup for this path, not merely one of
            // the newest entries, so do not cap the scan.
            None => match self.trash.list(usize::MAX).await {
                Ok(entries) => match entries
                    .into_iter()
                    .find(|entry| same_path(&entry.original, &target))
                {
                    Some(entry) => entry,
                    None => {
                        return ToolResult::failure(
                            ctx.call_id,
                            "restore_file",
                            ToolError::not_found(format!("no backup for `{path}`")),
                        )
                    }
                },
                Err(error) => return ToolResult::failure(ctx.call_id, "restore_file", error),
            },
        };
        if !same_path(&entry.original, &target) {
            return ToolResult::failure(
                ctx.call_id,
                "restore_file",
                ToolError::invalid_arguments(format!(
                    "trash entry `{}` belongs to `{}`, not `{path}`",
                    entry.id,
                    entry.original.display()
                )),
            );
        }

        let previous = match tokio::fs::metadata(&target).await {
            Ok(metadata) if metadata.is_file() => match self.trash.backup(&target).await {
                Ok(entry) => Some(entry),
                Err(error) => return ToolResult::failure(ctx.call_id, "restore_file", error),
            },
            Ok(_) => {
                return ToolResult::failure(
                    ctx.call_id,
                    "restore_file",
                    ToolError::invalid_arguments(format!(
                        "`{}` is not a regular file",
                        target.display()
                    )),
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return ToolResult::failure(ctx.call_id, "restore_file", io_error(&error));
            }
        };

        match tokio::fs::copy(&entry.stored, &target).await {
            Ok(_) => {
                let message = match previous {
                    Some(previous) => format!(
                        "restored {} from backup {} (previous version in trash as {})",
                        target.display(),
                        entry.id,
                        previous.id
                    ),
                    None => format!("restored {} from backup {}", target.display(), entry.id),
                };
                ToolResult::success(ctx.call_id, "restore_file", message)
            }
            Err(error) => ToolResult::failure(ctx.call_id, "restore_file", io_error(&error)),
        }
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn io_error(error: &std::io::Error) -> ToolError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ToolError::not_found(error.to_string()),
        std::io::ErrorKind::PermissionDenied => ToolError::permission_denied(error.to_string()),
        _ => ToolError::io(error.to_string()),
    }
}

/// Kernel-owned task list exposed to the model.
///
/// The tool is the only editing surface. The list itself lives in kernel state
/// and is rendered by the `task_state` context stage, so it cannot drift into
/// a prose file.
pub struct TodoTool {
    todos: Arc<Mutex<TodoList>>,
}

impl TodoTool {
    pub fn new(todos: Arc<Mutex<TodoList>>) -> Self {
        Self { todos }
    }

    fn apply(&self, args: &ToolArgs) -> Result<String, ToolError> {
        let op = args.require_str("op")?;
        let mut todos = self
            .todos
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match op {
            "add" => {
                let text = args.require_str("text")?;
                todos.add_model(text).map_err(todo_error)?;
            }
            "update" => {
                let id = require_todo_id(args)?;
                let status = require_todo_status(args)?;
                todos.update_status(id, status).map_err(todo_error)?;
            }
            "remove" => {
                let id = require_todo_id(args)?;
                todos.remove(id).map_err(todo_error)?;
            }
            "close" => {
                let outcome = args.require_str("outcome")?;
                let state = match outcome {
                    "completed" => TodoState::Completed,
                    "failed" => TodoState::Failed,
                    "abandoned" => TodoState::Abandoned,
                    _ => {
                        return Err(ToolError::invalid_arguments(
                            "todo outcome must be completed, failed or abandoned",
                        ));
                    }
                };
                todos.close(state).map_err(todo_error)?;
            }
            "list" => {}
            other => {
                return Err(ToolError::invalid_arguments(format!(
                    "unknown todo op `{other}`"
                )));
            }
        }
        Ok(todos.render())
    }
}

fn require_todo_id(args: &ToolArgs) -> Result<u32, ToolError> {
    match args.get("id") {
        Some(ArgValue::Integer(value)) if *value >= 0 && *value <= i64::from(u32::MAX) => {
            Ok(*value as u32)
        }
        Some(ArgValue::Integer(_)) => Err(ToolError::invalid_arguments(
            "todo id must be a positive integer",
        )),
        Some(other) => Err(ToolError::invalid_arguments(format!(
            "argument `id` must be integer, got {}",
            other.kind().as_str()
        ))),
        None => Err(ToolError::invalid_arguments(
            "missing required argument `id`",
        )),
    }
}

fn require_todo_status(args: &ToolArgs) -> Result<TodoStatus, ToolError> {
    let value = args.require_str("status")?;
    TodoStatus::parse(value).ok_or_else(|| {
        ToolError::invalid_arguments(
            "todo status must be pending, in_progress, completed or failed",
        )
    })
}

fn todo_error(error: TodoError) -> ToolError {
    ToolError::invalid_arguments(error.to_string())
}

#[async_trait]
impl Tool for TodoTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "todo".to_owned(),
            description: "Read or update the session task list. Ops: list; add with text; \
                          update with id and status (pending, in_progress, completed, \
                          failed); remove with id; close with outcome (completed, failed, \
                          abandoned)."
                .to_owned(),
            risk: RiskLevel::ReadOnly,
            params: vec![
                ParamSpec {
                    name: "op".to_owned(),
                    kind: ParamKind::String,
                    required: true,
                },
                ParamSpec {
                    name: "id".to_owned(),
                    kind: ParamKind::Integer,
                    required: false,
                },
                ParamSpec {
                    name: "status".to_owned(),
                    kind: ParamKind::String,
                    required: false,
                },
                ParamSpec {
                    name: "text".to_owned(),
                    kind: ParamKind::String,
                    required: false,
                },
                ParamSpec {
                    name: "outcome".to_owned(),
                    kind: ParamKind::String,
                    required: false,
                },
            ],
        }
    }

    fn permission_requests(&self, _args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError> {
        Ok(Vec::new())
    }

    async fn call(&self, args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        if ctx.cancel.is_cancelled() {
            return ToolResult::failure(ctx.call_id, "todo", ToolError::cancelled());
        }
        match self.apply(&args) {
            Ok(content) => ToolResult::success(ctx.call_id, "todo", content),
            Err(error) => ToolResult::failure(ctx.call_id, "todo", error),
        }
    }
}

#[cfg(test)]
mod todo_tool_tests {
    use super::*;
    use keel_core::{CancelToken, ToolCallId, TurnId};

    fn ctx() -> ToolCtx {
        ToolCtx {
            turn_id: TurnId::new(1),
            call_id: ToolCallId::new(1),
            workspace_root: PathBuf::from("."),
            cancel: CancelToken::new(),
        }
    }

    fn content(result: &ToolResult) -> &str {
        match &result.outcome {
            keel_core::ToolOutcome::Success { content } => content,
            keel_core::ToolOutcome::Failure { error } => panic!("tool failed: {error}"),
        }
    }

    #[tokio::test]
    async fn update_moves_a_host_task_and_add_is_locked() {
        let todos = Arc::new(Mutex::new(TodoList::new()));
        let id = todos
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .add_host("chapter 1")
            .unwrap();
        todos
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .lock();
        let tool = TodoTool::new(todos);

        let mut update = ToolArgs::new();
        update.insert("op", ArgValue::String("update".to_owned()));
        update.insert("id", ArgValue::Integer(id as i64));
        update.insert("status", ArgValue::String("in_progress".to_owned()));
        let result = tool.call(update, &ctx()).await;
        assert!(content(&result).contains("#1 [>] chapter 1"));

        let mut add = ToolArgs::new();
        add.insert("op", ArgValue::String("add".to_owned()));
        add.insert("text", ArgValue::String("gap task".to_owned()));
        let result = tool.call(add, &ctx()).await;
        assert!(result.error().is_some());
    }

    #[tokio::test]
    async fn completing_the_last_item_closes_the_list() {
        let todos = Arc::new(Mutex::new(TodoList::new()));
        let id = todos
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .add_host("chapter 1")
            .unwrap();
        let tool = TodoTool::new(todos);

        let mut update = ToolArgs::new();
        update.insert("op", ArgValue::String("update".to_owned()));
        update.insert("id", ArgValue::Integer(id as i64));
        update.insert("status", ArgValue::String("completed".to_owned()));
        let result = tool.call(update.clone(), &ctx()).await;
        assert!(content(&result).contains("[tasks] completed: 1/1"));

        let result = tool.call(update, &ctx()).await;
        assert!(result.error().is_some(), "closed lists reject updates");
    }
}

#[cfg(test)]
mod file_tool_tests {
    use super::*;
    use keel_core::{CancelToken, ToolCallId, TurnId};

    fn ctx(root: &Path) -> ToolCtx {
        ToolCtx {
            turn_id: TurnId::new(1),
            call_id: ToolCallId::new(1),
            workspace_root: root.to_path_buf(),
            cancel: CancelToken::new(),
        }
    }

    fn args(pairs: &[(&str, &str)]) -> ToolArgs {
        let mut args = ToolArgs::new();
        for (key, value) in pairs {
            args.insert(*key, ArgValue::String((*value).to_owned()));
        }
        args
    }

    fn content(result: &ToolResult) -> &str {
        match &result.outcome {
            keel_core::ToolOutcome::Success { content } => content,
            keel_core::ToolOutcome::Failure { error } => panic!("tool failed: {error}"),
        }
    }

    fn trash(root: &Path) -> Arc<dyn TrashStore> {
        Arc::new(crate::trash::LocalTrashStore::new(root.to_path_buf()))
    }

    #[tokio::test]
    async fn make_dir_is_idempotent_and_refuses_files() {
        let dir = tempfile::tempdir().unwrap();
        let tool = MakeDirTool::new();

        let result = tool.call(args(&[("path", "a/b")]), &ctx(dir.path())).await;
        assert!(content(&result).contains("created"));
        assert!(dir.path().join("a/b").is_dir());

        let result = tool.call(args(&[("path", "a/b")]), &ctx(dir.path())).await;
        assert!(content(&result).contains("already exists"));

        std::fs::write(dir.path().join("file"), "x").unwrap();
        let result = tool.call(args(&[("path", "file")]), &ctx(dir.path())).await;
        assert!(result.error().is_some());
    }

    #[tokio::test]
    async fn delete_file_moves_the_file_into_the_trash() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("junk.txt"), "junk").unwrap();
        let trash = trash(dir.path());
        let tool = DeleteFileTool::new(trash.clone());

        let result = tool
            .call(args(&[("path", "junk.txt")]), &ctx(dir.path()))
            .await;

        assert!(content(&result).contains("trash entry"));
        assert!(!dir.path().join("junk.txt").exists());
        let entries = trash.list(10).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].original.ends_with("junk.txt"));
        assert!(entries[0].stored.is_file());
        assert_eq!(std::fs::read_to_string(&entries[0].stored).unwrap(), "junk");
    }

    #[tokio::test]
    async fn delete_file_refuses_directories() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        let tool = DeleteFileTool::new(trash(dir.path()));

        let result = tool
            .call(args(&[("path", "nested")]), &ctx(dir.path()))
            .await;

        assert!(result.error().is_some());
        assert!(dir.path().join("nested").is_dir());
    }

    #[tokio::test]
    async fn move_file_backs_up_an_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("new.txt"), "new").unwrap();
        std::fs::write(dir.path().join("old.txt"), "old").unwrap();
        let tool = MoveFileTool::new(trash(dir.path()));

        let result = tool
            .call(
                args(&[("from", "new.txt"), ("to", "old.txt")]),
                &ctx(dir.path()),
            )
            .await;

        assert!(content(&result).contains("moved"));
        assert!(!dir.path().join("new.txt").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("old.txt")).unwrap(),
            "new"
        );
    }

    #[tokio::test]
    async fn move_file_creates_the_destination_parent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        let tool = MoveFileTool::new(trash(dir.path()));

        let result = tool
            .call(
                args(&[("from", "a.txt"), ("to", "nested/a.txt")]),
                &ctx(dir.path()),
            )
            .await;

        assert!(content(&result).contains("moved"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("nested/a.txt")).unwrap(),
            "a"
        );
    }

    #[tokio::test]
    async fn move_file_reports_a_missing_source() {
        let dir = tempfile::tempdir().unwrap();
        let tool = MoveFileTool::new(trash(dir.path()));

        let result = tool
            .call(
                args(&[("from", "missing.txt"), ("to", "b.txt")]),
                &ctx(dir.path()),
            )
            .await;

        assert_eq!(
            result.error().map(|error| error.code),
            Some(keel_core::ErrorCode::NotFound)
        );
    }
}
