use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use keel_core::{
    resolve_path, AccessKind, ParamKind, ParamSpec, PermissionRequest, RiskLevel, Tool, ToolArgs,
    ToolCtx, ToolError, ToolResult, ToolSchema,
};

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
        match tokio::fs::read_to_string(&resolved).await {
            Ok(content) => ToolResult::success(ctx.call_id, "read_file", content),
            Err(error) => ToolResult::failure(ctx.call_id, "read_file", io_error(&error)),
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
}

impl ShellTool {
    pub fn new(allowlist: Vec<String>) -> Self {
        Self { allowlist }
    }

    /// Developer programs useful for a coding profile. Shells are excluded so
    /// the tool cannot be turned into an interpolating command string.
    pub fn coding_defaults() -> Self {
        Self::new(command::coding_allowlist())
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "shell".to_owned(),
            description: "Run an allowlisted program with a fixed argument list; no shell."
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
        let detail = match args.get("args") {
            Some(keel_core::ArgValue::StringList(values)) if !values.is_empty() => {
                format!("{program} {}", values.join(" "))
            }
            _ => program.to_owned(),
        };
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
        if !command::is_allowed(program, &self.allowlist) {
            return ToolResult::failure(
                ctx.call_id,
                "shell",
                ToolError::invalid_arguments(format!(
                    "program `{program}` is not allowlisted; allowed: {}",
                    self.allowlist.join(", ")
                )),
            );
        }
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
            timeout: Duration::from_secs(60),
        };
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

fn io_error(error: &std::io::Error) -> ToolError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ToolError::not_found(error.to_string()),
        std::io::ErrorKind::PermissionDenied => ToolError::permission_denied(error.to_string()),
        _ => ToolError::io(error.to_string()),
    }
}
