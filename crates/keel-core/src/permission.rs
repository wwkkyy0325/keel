use std::path::{Component, Path, PathBuf};

use crate::error::ErrorCode;

/// Permission level configured for a session.
///
/// `WorkspaceAuto` is the logical sandbox: normal actions inside the workspace
/// run automatically, while actions outside it require confirmation.
/// `FullAuto` removes the workspace boundary; the hard deny list still applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionLevel {
    WorkspaceAuto,
    FullAuto,
}

impl PermissionLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceAuto => "workspace_auto",
            Self::FullAuto => "full_auto",
        }
    }
}

/// Kind of access a tool asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessKind {
    Read,
    Write,
    Execute,
}

impl AccessKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Execute => "execute",
        }
    }
}

/// One permission question asked by a tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    pub tool: String,
    pub access: AccessKind,
    pub path: Option<PathBuf>,
    /// Human-readable detail shown in the confirmation prompt, such as the
    /// command line for a shell call. It is not an authorization input.
    pub detail: Option<String>,
}

/// Answer to a permission question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Confirm { reason: String },
    Deny { code: ErrorCode, message: String },
}

/// Permission gate used by tools. The concrete engine lives in the runtime.
pub trait PermissionGate: Send + Sync {
    fn check(&self, request: &PermissionRequest) -> PermissionDecision;
}

/// Resolves a tool path against the workspace root and normalizes it without
/// touching the filesystem. Symlink-aware resolution is a later hardening
/// step; lexical normalization is enough to stop `..` traversal here.
pub fn resolve_path(root: &Path, path: &Path) -> PathBuf {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    simplify_path(normalize(&candidate))
}

/// Strips the Windows verbatim prefix so canonicalized roots and tool paths
/// compare with the same representation.
pub fn simplify_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path
}

fn normalize(path: &Path) -> PathBuf {
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop();
            }
            Component::Prefix(prefix) => parts.push(prefix.as_os_str().to_os_string()),
            Component::RootDir => parts.push(component.as_os_str().to_os_string()),
            Component::Normal(part) => parts.push(part.to_os_string()),
        }
    }
    let mut normalized = PathBuf::new();
    for part in parts {
        normalized.push(part);
    }
    normalized
}
