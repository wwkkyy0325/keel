use std::path::{Component, Path, PathBuf};

use serde::Serialize;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
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
/// touching the filesystem. `..` never climbs above the drive or filesystem
/// root. The dispatcher canonicalizes the result before the permission gate,
/// so a symlink inside the workspace cannot point the check at another path.
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
    enum Piece {
        Prefix(std::ffi::OsString),
        Root(std::ffi::OsString),
        Normal(std::ffi::OsString),
    }

    let mut parts: Vec<Piece> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Only a normal component can be popped. Popping the drive or
                // filesystem root would turn an absolute path into a relative
                // one, which would then resolve against the process cwd.
                if matches!(parts.last(), Some(Piece::Normal(_))) {
                    parts.pop();
                }
            }
            Component::Prefix(prefix) => {
                parts.push(Piece::Prefix(prefix.as_os_str().to_os_string()));
            }
            Component::RootDir => parts.push(Piece::Root(component.as_os_str().to_os_string())),
            Component::Normal(part) => parts.push(Piece::Normal(part.to_os_string())),
        }
    }
    let mut normalized = PathBuf::new();
    for part in parts {
        match part {
            Piece::Prefix(value) | Piece::Root(value) | Piece::Normal(value) => {
                normalized.push(value);
            }
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_components_do_not_pop_the_filesystem_root() {
        #[cfg(windows)]
        let (root, expected) = (Path::new(r"C:\ws\proj"), Path::new(r"C:\etc"));
        #[cfg(not(windows))]
        let (root, expected) = (Path::new("/ws/proj"), Path::new("/etc"));

        assert_eq!(resolve_path(root, Path::new("../../../etc")), expected);
    }

    #[test]
    fn an_absolute_path_keeps_its_drive_and_root() {
        #[cfg(windows)]
        {
            assert_eq!(
                resolve_path(Path::new(r"C:\ws"), Path::new(r"C:\..\Windows")),
                PathBuf::from(r"C:\Windows")
            );
        }
        #[cfg(not(windows))]
        {
            assert_eq!(
                resolve_path(Path::new("/ws"), Path::new("/../etc")),
                PathBuf::from("/etc")
            );
        }
    }

    #[test]
    fn inner_parent_components_still_collapse() {
        #[cfg(windows)]
        let root = Path::new(r"C:\ws");
        #[cfg(not(windows))]
        let root = Path::new("/ws");

        #[cfg(windows)]
        let expected = PathBuf::from(r"C:\ws\b");
        #[cfg(not(windows))]
        let expected = PathBuf::from("/ws/b");

        assert_eq!(resolve_path(root, Path::new("a/../b")), expected);
    }
}
