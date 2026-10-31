use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use keel_core::{
    resolve_path, AccessKind, ErrorCode, KernelError, PermissionDecision, PermissionGate,
    PermissionLevel, PermissionRequest,
};

/// Minimal M1 permission engine.
///
/// The workspace root is the logical sandbox for `WorkspaceAuto`. `FullAuto`
/// removes that boundary but never the hard deny list. Symlink-aware checks
/// and the full admin/deny catalogue land with the M5 permission milestone.
pub struct PermissionEngine {
    root: PathBuf,
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
        Ok(Self {
            root,
            base_level: level,
            grants: Mutex::new(Vec::new()),
        })
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
        if is_denied(&resolved) {
            return PermissionDecision::Deny {
                code: ErrorCode::PermissionDenied,
                message: format!("path `{}` is on the hard deny list", resolved.display()),
            };
        }

        let inside = resolved.starts_with(&self.root);
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
            PermissionLevel::WorkspaceAuto => match request.access {
                AccessKind::Read | AccessKind::Write if inside => PermissionDecision::Allow,
                _ => PermissionDecision::Confirm {
                    reason: format!(
                        "`{}` is outside the workspace sandbox or requires execution access",
                        resolved.display()
                    ),
                },
            },
        }
    }
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
