use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use keel_core::{
    resolve_path, AccessKind, ParamKind, ParamSpec, PermissionRequest, RiskLevel, Tool, ToolArgs,
    ToolCtx, ToolError, ToolResult, ToolSchema,
};

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
