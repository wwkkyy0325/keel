use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use keel_core::{
    resolve_path, AccessKind, ErrorCode, KernelError, PermissionDecision, PermissionGate,
    PermissionLevel, PermissionRequest,
};

/// One additional directory the agent may access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootScope {
    pub path: PathBuf,
    pub access: AccessKind,
}

/// Permission engine.
///
/// The workspace root is the permission boundary for `WorkspaceAuto`; a host
/// may add labeled roots with read or read-write access. `FullAuto` removes
/// every boundary but never the hard deny list. The dispatcher canonicalizes
/// each requested path before the gate, so a symlink inside a root cannot
/// redirect the check outside it.
pub struct PermissionEngine {
    root: PathBuf,
    extra_roots: Vec<RootScope>,
    /// Directory whose files are readable so the model can follow a spill
    /// marker. Defaults to `<workspace>/.keel/outputs`.
    outputs: PathBuf,
    base_level: PermissionLevel,
    grants: Mutex<Vec<Grant>>,
}

struct Grant {
    prefix: Option<PathBuf>,
    access: Option<AccessKind>,
    expires_at: Instant,
}

impl PermissionEngine {
    pub fn new(root: impl AsRef<Path>, level: PermissionLevel) -> Result<Self, KernelError> {
        let root = std::fs::canonicalize(root.as_ref())
            .map(keel_core::simplify_path)
            .map_err(|error| KernelError::InvalidConfig {
                message: format!(
                    "workspace root `{}` is not accessible: {error}",
                    root.as_ref().display()
                ),
            })?;
        if root.parent().is_none() {
            return Err(KernelError::InvalidConfig {
                message: format!(
                    "workspace root `{}` is a filesystem root; choose a narrower directory",
                    root.display()
                ),
            });
        }
        if is_system_directory(&root) {
            return Err(KernelError::InvalidConfig {
                message: format!("workspace root `{}` is a system directory", root.display()),
            });
        }
        let outputs = root.join(".keel").join("outputs");
        Ok(Self {
            root,
            extra_roots: Vec::new(),
            outputs,
            base_level: level,
            grants: Mutex::new(Vec::new()),
        })
    }

    /// Sets the directory whose files may be read back after a tool result is
    /// truncated. The path should already be canonical and absolute.
    pub fn with_outputs(mut self, path: impl Into<PathBuf>) -> Self {
        self.outputs = path.into();
        self
    }

    /// Adds a directory the agent may use outside the primary workspace.
    ///
    /// The path is canonicalized. Drive/filesystem roots and system
    /// directories are refused: a job should name the smallest directory it
    /// needs, not a whole volume.
    pub fn with_root(
        mut self,
        path: impl AsRef<Path>,
        access: AccessKind,
    ) -> Result<Self, KernelError> {
        let source = path.as_ref();
        let path = std::fs::canonicalize(source)
            .map(keel_core::simplify_path)
            .map_err(|error| KernelError::InvalidConfig {
                message: format!("root `{}` is not accessible: {error}", source.display()),
            })?;
        if access == AccessKind::Execute {
            return Err(KernelError::InvalidConfig {
                message: format!(
                    "root `{}` cannot grant execute access; execution stays confirm-only",
                    path.display()
                ),
            });
        }
        if path.parent().is_none() {
            return Err(KernelError::InvalidConfig {
                message: format!(
                    "root `{}` is a filesystem root; choose a narrower directory",
                    path.display()
                ),
            });
        }
        if is_system_directory(&path) {
            return Err(KernelError::InvalidConfig {
                message: format!("root `{}` is a system directory", path.display()),
            });
        }
        if path == self.root || self.extra_roots.iter().any(|scope| scope.path == path) {
            return Ok(self);
        }
        self.extra_roots.push(RootScope { path, access });
        Ok(self)
    }

    /// Every scope the agent may use: the primary workspace first, then the
    /// extra roots in the order they were added.
    pub fn roots(&self) -> Vec<RootScope> {
        let mut scopes = Vec::with_capacity(self.extra_roots.len() + 1);
        scopes.push(RootScope {
            path: self.root.clone(),
            access: AccessKind::Write,
        });
        scopes.extend(self.extra_roots.iter().cloned());
        scopes
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn level(&self) -> PermissionLevel {
        let grants = self.active_grants();
        if grants.iter().any(is_full_auto_grant) {
            PermissionLevel::FullAuto
        } else {
            self.base_level
        }
    }

    /// Session-level escalation approved by the user through an interaction.
    pub fn escalate_full_auto(&self, ttl: Duration) {
        self.push_grant(Grant {
            prefix: None,
            access: None,
            expires_at: Instant::now() + ttl,
        });
    }

    /// Allows one access kind under a path prefix until the TTL expires.
    pub fn grant_path_prefix(&self, prefix: &Path, access: AccessKind, ttl: Duration) {
        let prefix = resolve_path(&self.root, prefix);
        self.push_grant(Grant {
            prefix: Some(prefix),
            access: Some(access),
            expires_at: Instant::now() + ttl,
        });
    }

    pub fn revoke_all(&self) {
        self.grants
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    fn push_grant(&self, grant: Grant) {
        let mut grants = self
            .grants
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        grants.retain(|grant| grant.expires_at > Instant::now());
        grants.push(grant);
    }

    fn active_grants(&self) -> MutexGuard<'_, Vec<Grant>> {
        let mut grants = self
            .grants
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        grants.retain(|grant| grant.expires_at > Instant::now());
        grants
    }
}

fn is_full_auto_grant(grant: &Grant) -> bool {
    grant.prefix.is_none() && grant.access.is_none()
}

impl PermissionGate for PermissionEngine {
    fn check(&self, request: &PermissionRequest) -> PermissionDecision {
        let Some(path) = &request.path else {
            return match request.access {
                AccessKind::Read => PermissionDecision::Allow,
                AccessKind::Write | AccessKind::Execute => PermissionDecision::Deny {
                    code: ErrorCode::PermissionDenied,
                    message: format!(
                        "tool `{}` must declare a path for this action",
                        request.tool
                    ),
                },
            };
        };

        let resolved = resolve_path(&self.root, path);
        // Truncated tool output is written to the configured outputs
        // directory and the model is told to read it back. The rest of
        // `.keel` stays denied, and the exception is read-only.
        if request.access == AccessKind::Read && resolved.starts_with(&self.outputs) {
            return PermissionDecision::Allow;
        }
        if is_denied(&resolved) {
            return PermissionDecision::Deny {
                code: ErrorCode::PermissionDenied,
                message: format!("path `{}` is on the hard deny list", resolved.display()),
            };
        }

        let scope_access = if resolved.starts_with(&self.root) {
            Some(AccessKind::Write)
        } else {
            self.extra_roots
                .iter()
                .filter(|scope| resolved.starts_with(&scope.path))
                .max_by_key(|scope| scope.path.components().count())
                .map(|scope| scope.access)
        };
        let grants = self.active_grants();
        let full_auto_granted = grants.iter().any(is_full_auto_grant);
        let path_granted = grants.iter().any(|grant| {
            grant.access == Some(request.access)
                && grant
                    .prefix
                    .as_ref()
                    .is_some_and(|prefix| resolved.starts_with(prefix))
        });
        drop(grants);

        if full_auto_granted {
            return PermissionDecision::Allow;
        }
        if path_granted {
            return PermissionDecision::Allow;
        }

        match self.base_level {
            PermissionLevel::FullAuto => PermissionDecision::Allow,
            PermissionLevel::WorkspaceAuto => match scope_access {
                Some(access) if permits(access, request.access) => PermissionDecision::Allow,
                Some(_) if request.access == AccessKind::Execute => PermissionDecision::Confirm {
                    reason: "execution requires confirmation".to_owned(),
                },
                Some(_) => PermissionDecision::Confirm {
                    reason: format!("`{}` is read-only in this root", resolved.display()),
                },
                None => PermissionDecision::Confirm {
                    reason: format!(
                        "`{}` is outside the workspace and its extra roots",
                        resolved.display()
                    ),
                },
            },
        }
    }
}

/// A read root allows reads only; a write root allows reads and writes.
/// Execution is always a confirmation, wherever the path is.
fn permits(scope: AccessKind, requested: AccessKind) -> bool {
    match requested {
        AccessKind::Read => true,
        AccessKind::Write => scope == AccessKind::Write,
        AccessKind::Execute => false,
    }
}

/// A root that would hand the agent an operating-system area instead of a
/// task directory. This is a guardrail against accidental `--root C:\`, not a
/// security boundary.
fn is_system_directory(path: &Path) -> bool {
    let text = path.to_string_lossy().to_ascii_lowercase();
    let text = text.trim_end_matches(['\\', '/']);
    // A whole user-profile parent is too broad, but `C:\Users\<name>` is a
    // normal workspace, so only the exact parent is refused.
    if text == r"c:\users" || text == "/home" {
        return true;
    }
    let windows = [
        r"c:\windows",
        r"c:\program files",
        r"c:\program files (x86)",
        r"c:\programdata",
        r"c:\$recycle.bin",
        r"c:\system volume information",
    ];
    if windows
        .iter()
        .any(|prefix| text == *prefix || text.starts_with(&format!("{prefix}\\")))
    {
        return true;
    }
    let unix = [
        "/etc", "/usr", "/bin", "/sbin", "/boot", "/sys", "/proc", "/dev", "/var", "/root",
    ];
    unix.iter()
        .any(|prefix| text == *prefix || text.starts_with(&format!("{prefix}/")))
}

fn is_denied(resolved: &Path) -> bool {
    if let Some(name) = resolved.file_name().and_then(|name| name.to_str()) {
        let lower = name.to_ascii_lowercase();
        if lower == ".env"
            || lower.starts_with(".env.")
            || lower == ".netrc"
            || lower == "id_rsa"
            || lower == "id_ed25519"
            || lower == "credentials.json"
            || lower.ends_with(".pem")
            || lower.ends_with(".key")
            || lower.ends_with(".p12")
            || lower.ends_with(".pfx")
        {
            return true;
        }
    }

    for component in resolved.components() {
        if let Component::Normal(part) = component {
            let lower = part.to_string_lossy().to_ascii_lowercase();
            if matches!(
                lower.as_str(),
                ".ssh"
                    | ".gnupg"
                    | ".aws"
                    | ".azure"
                    | ".kube"
                    | ".docker"
                    | ".git"
                    | ".keel"
                    | "secrets"
            ) {
                return true;
            }
        }
    }

    let text = resolved.to_string_lossy().to_ascii_lowercase();
    let unix_prefixes = [
        "/etc/shadow",
        "/etc/sudoers",
        "/etc/sudoers.d",
        "/boot",
        "/sys",
        "/proc/sys",
        "/dev/mem",
        "/dev/sd",
        "/dev/nvme",
    ];
    if unix_prefixes
        .iter()
        .any(|prefix| text == *prefix || text.starts_with(prefix))
    {
        return true;
    }
    text.starts_with("c:\\windows\\system32") || text.starts_with("c:\\windows\\syswow64")
}

#[cfg(test)]
mod tests {
    use super::*;
    use keel_core::resolve_path;

    fn engine(level: PermissionLevel) -> (PermissionEngine, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let engine = PermissionEngine::new(dir.path(), level).unwrap();
        (engine, dir)
    }

    fn request(access: AccessKind, path: &Path) -> PermissionRequest {
        PermissionRequest {
            tool: "test".to_owned(),
            access,
            path: Some(path.to_path_buf()),
            detail: None,
        }
    }

    #[test]
    fn read_inside_workspace_is_allowed() {
        let (engine, dir) = engine(PermissionLevel::WorkspaceAuto);
        let inside = resolve_path(engine.root(), &dir.path().join("file.txt"));
        assert_eq!(
            engine.check(&request(AccessKind::Read, &inside)),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn write_inside_workspace_is_allowed() {
        let (engine, dir) = engine(PermissionLevel::WorkspaceAuto);
        let inside = resolve_path(engine.root(), &dir.path().join("file.txt"));
        assert_eq!(
            engine.check(&request(AccessKind::Write, &inside)),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn outside_read_and_write_require_confirmation() {
        let (engine, _dir) = engine(PermissionLevel::WorkspaceAuto);
        let outside = engine.root().join("..").join("outside.txt");
        assert!(matches!(
            engine.check(&request(AccessKind::Read, &outside)),
            PermissionDecision::Confirm { .. }
        ));
        assert!(matches!(
            engine.check(&request(AccessKind::Write, &outside)),
            PermissionDecision::Confirm { .. }
        ));
    }

    #[test]
    fn full_auto_allows_outside_paths() {
        let (engine, _dir) = engine(PermissionLevel::FullAuto);
        let outside = engine.root().join("..").join("outside.txt");
        assert_eq!(
            engine.check(&request(AccessKind::Write, &outside)),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn full_auto_still_denies_sensitive_paths() {
        let (engine, dir) = engine(PermissionLevel::FullAuto);
        for relative in [".env", ".ssh/id_rsa", ".git/config", ".keel/trash/item"] {
            let path = resolve_path(engine.root(), &dir.path().join(relative));
            assert!(
                matches!(
                    engine.check(&request(AccessKind::Read, &path)),
                    PermissionDecision::Deny { .. }
                ),
                "{relative} must be denied"
            );
        }
    }

    #[test]
    fn reading_a_spilled_output_is_allowed() {
        for level in [PermissionLevel::WorkspaceAuto, PermissionLevel::FullAuto] {
            let (engine, dir) = engine(level);
            let path = resolve_path(
                engine.root(),
                &dir.path().join(".keel/outputs/turn-1-call-0.txt"),
            );
            assert_eq!(
                engine.check(&request(AccessKind::Read, &path)),
                PermissionDecision::Allow
            );
        }
    }

    #[test]
    fn a_configured_outputs_directory_is_readable() {
        let workspace = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let outputs = state.path().join("outputs");
        let engine = PermissionEngine::new(workspace.path(), PermissionLevel::WorkspaceAuto)
            .unwrap()
            .with_outputs(&outputs);
        let path = outputs.join("turn-1-call-0.txt");

        assert_eq!(
            engine.check(&request(AccessKind::Read, &path)),
            PermissionDecision::Allow
        );
        assert!(matches!(
            engine.check(&request(AccessKind::Write, &path)),
            PermissionDecision::Confirm { .. }
        ));
    }

    #[test]
    fn the_rest_of_keel_stays_denied() {
        let (engine, dir) = engine(PermissionLevel::FullAuto);
        let trash = resolve_path(engine.root(), &dir.path().join(".keel/trash/item"));
        assert!(matches!(
            engine.check(&request(AccessKind::Read, &trash)),
            PermissionDecision::Deny { .. }
        ));
        let output = resolve_path(engine.root(), &dir.path().join(".keel/outputs/item"));
        assert!(matches!(
            engine.check(&request(AccessKind::Write, &output)),
            PermissionDecision::Deny { .. }
        ));
    }

    #[test]
    fn execute_inside_requires_confirmation() {
        let (engine, dir) = engine(PermissionLevel::WorkspaceAuto);
        let inside = resolve_path(engine.root(), &dir.path().join("script.sh"));
        assert!(matches!(
            engine.check(&request(AccessKind::Execute, &inside)),
            PermissionDecision::Confirm { .. }
        ));
    }

    #[test]
    fn mutating_tool_without_a_path_is_denied() {
        let (engine, _dir) = engine(PermissionLevel::FullAuto);
        let request = PermissionRequest {
            tool: "test".to_owned(),
            access: AccessKind::Write,
            path: None,
            detail: None,
        };
        assert!(matches!(
            engine.check(&request),
            PermissionDecision::Deny { .. }
        ));
    }

    #[test]
    fn session_escalation_allows_outside_paths() {
        let (engine, _dir) = engine(PermissionLevel::WorkspaceAuto);
        let outside = engine.root().join("..").join("outside.txt");
        assert!(matches!(
            engine.check(&request(AccessKind::Write, &outside)),
            PermissionDecision::Confirm { .. }
        ));
        engine.escalate_full_auto(Duration::from_secs(1));
        assert_eq!(engine.level(), PermissionLevel::FullAuto);
        assert_eq!(
            engine.check(&request(AccessKind::Write, &outside)),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn escalation_expires_after_ttl() {
        let (engine, _dir) = engine(PermissionLevel::WorkspaceAuto);
        let outside = engine.root().join("..").join("outside.txt");
        engine.escalate_full_auto(Duration::from_millis(10));
        assert_eq!(
            engine.check(&request(AccessKind::Write, &outside)),
            PermissionDecision::Allow
        );
        std::thread::sleep(Duration::from_millis(30));
        assert!(matches!(
            engine.check(&request(AccessKind::Write, &outside)),
            PermissionDecision::Confirm { .. }
        ));
        assert_eq!(engine.level(), PermissionLevel::WorkspaceAuto);
    }

    #[test]
    fn path_prefix_grant_is_scoped_by_path_and_access() {
        let (engine, _dir) = engine(PermissionLevel::WorkspaceAuto);
        let shared = engine.root().join("..").join("shared");
        let inside_shared = shared.join("file.txt");
        engine.grant_path_prefix(&shared, AccessKind::Write, Duration::from_secs(1));

        assert_eq!(
            engine.check(&request(AccessKind::Write, &inside_shared)),
            PermissionDecision::Allow
        );
        assert!(matches!(
            engine.check(&request(AccessKind::Read, &inside_shared)),
            PermissionDecision::Confirm { .. }
        ));
        let other = engine.root().join("..").join("other.txt");
        assert!(matches!(
            engine.check(&request(AccessKind::Write, &other)),
            PermissionDecision::Confirm { .. }
        ));
    }

    #[test]
    fn hard_deny_list_survives_full_auto_grants() {
        let (engine, dir) = engine(PermissionLevel::WorkspaceAuto);
        engine.escalate_full_auto(Duration::from_secs(60));
        let env = resolve_path(engine.root(), &dir.path().join(".env"));
        assert!(matches!(
            engine.check(&request(AccessKind::Read, &env)),
            PermissionDecision::Deny { .. }
        ));
    }

    #[test]
    fn revoke_all_restores_the_base_level() {
        let (engine, _dir) = engine(PermissionLevel::WorkspaceAuto);
        let outside = engine.root().join("..").join("outside.txt");
        engine.escalate_full_auto(Duration::from_secs(60));
        assert_eq!(engine.level(), PermissionLevel::FullAuto);
        engine.revoke_all();
        assert_eq!(engine.level(), PermissionLevel::WorkspaceAuto);
        assert!(matches!(
            engine.check(&request(AccessKind::Write, &outside)),
            PermissionDecision::Confirm { .. }
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_system_paths_are_denied() {
        let (engine, _dir) = engine(PermissionLevel::FullAuto);
        let path = PathBuf::from("C:\\Windows\\System32\\config\\SAM");
        assert!(matches!(
            engine.check(&request(AccessKind::Read, &path)),
            PermissionDecision::Deny { .. }
        ));
    }
}

#[cfg(test)]
mod root_scope_tests {
    use super::*;
    use keel_core::resolve_path;

    fn request(access: AccessKind, path: &Path) -> PermissionRequest {
        PermissionRequest {
            tool: "test".to_owned(),
            access,
            path: Some(path.to_path_buf()),
            detail: None,
        }
    }

    #[test]
    fn an_extra_read_root_allows_reads_and_confirms_writes() {
        let workspace = tempfile::tempdir().unwrap();
        let extra = tempfile::tempdir().unwrap();
        let engine = PermissionEngine::new(workspace.path(), PermissionLevel::WorkspaceAuto)
            .unwrap()
            .with_root(extra.path(), AccessKind::Read)
            .unwrap();
        let path = extra.path().join("note.txt");

        assert_eq!(
            engine.check(&request(AccessKind::Read, &path)),
            PermissionDecision::Allow
        );
        assert!(matches!(
            engine.check(&request(AccessKind::Write, &path)),
            PermissionDecision::Confirm { .. }
        ));
    }

    #[test]
    fn an_extra_write_root_allows_reads_and_writes() {
        let workspace = tempfile::tempdir().unwrap();
        let extra = tempfile::tempdir().unwrap();
        let engine = PermissionEngine::new(workspace.path(), PermissionLevel::WorkspaceAuto)
            .unwrap()
            .with_root(extra.path(), AccessKind::Write)
            .unwrap();
        let path = extra.path().join("note.txt");

        assert_eq!(
            engine.check(&request(AccessKind::Read, &path)),
            PermissionDecision::Allow
        );
        assert_eq!(
            engine.check(&request(AccessKind::Write, &path)),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn execution_in_an_extra_root_still_requires_confirmation() {
        let workspace = tempfile::tempdir().unwrap();
        let extra = tempfile::tempdir().unwrap();
        let engine = PermissionEngine::new(workspace.path(), PermissionLevel::WorkspaceAuto)
            .unwrap()
            .with_root(extra.path(), AccessKind::Write)
            .unwrap();
        let path = extra.path().join("script.sh");

        assert!(matches!(
            engine.check(&request(AccessKind::Execute, &path)),
            PermissionDecision::Confirm { .. }
        ));
    }

    #[test]
    fn the_most_specific_root_wins() {
        let workspace = tempfile::tempdir().unwrap();
        let outer = tempfile::tempdir().unwrap();
        let inner = outer.path().join("inner");
        std::fs::create_dir(&inner).unwrap();
        let engine = PermissionEngine::new(workspace.path(), PermissionLevel::WorkspaceAuto)
            .unwrap()
            .with_root(outer.path(), AccessKind::Write)
            .unwrap()
            .with_root(&inner, AccessKind::Read)
            .unwrap();
        let path = inner.join("note.txt");

        assert_eq!(
            engine.check(&request(AccessKind::Read, &path)),
            PermissionDecision::Allow
        );
        assert!(matches!(
            engine.check(&request(AccessKind::Write, &path)),
            PermissionDecision::Confirm { .. }
        ));
    }

    #[test]
    fn the_primary_root_is_listed_first() {
        let workspace = tempfile::tempdir().unwrap();
        let extra = tempfile::tempdir().unwrap();
        let engine = PermissionEngine::new(workspace.path(), PermissionLevel::WorkspaceAuto)
            .unwrap()
            .with_root(extra.path(), AccessKind::Read)
            .unwrap();
        let roots = engine.roots();
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0].path, resolve_path(engine.root(), engine.root()));
        assert_eq!(roots[0].access, AccessKind::Write);
        assert_eq!(roots[1].access, AccessKind::Read);
    }

    #[test]
    fn a_filesystem_root_is_rejected() {
        #[cfg(windows)]
        let root = PathBuf::from("C:\\");
        #[cfg(not(windows))]
        let root = PathBuf::from("/");

        assert!(PermissionEngine::new(&root, PermissionLevel::WorkspaceAuto).is_err());
    }

    #[test]
    fn a_system_directory_is_rejected_as_a_root() {
        #[cfg(windows)]
        let root = PathBuf::from(r"C:\Windows");
        #[cfg(not(windows))]
        let root = PathBuf::from("/etc");

        assert!(PermissionEngine::new(&root, PermissionLevel::WorkspaceAuto).is_err());
    }
}
