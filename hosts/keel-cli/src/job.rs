use std::path::{Path, PathBuf};

use keel_core::{AccessKind, KernelError};
use serde::Deserialize;

/// A reusable task: composition, scope, state and the first input in one file.
/// Every path is resolved relative to the job file.
#[derive(Debug)]
pub struct Job {
    pub bundle: Option<PathBuf>,
    pub profile: Option<PathBuf>,
    pub plugins: Option<PathBuf>,
    pub workspace: Option<PathBuf>,
    pub state_dir: Option<PathBuf>,
    pub store: Option<PathBuf>,
    pub session_id: Option<String>,
    pub input: Option<String>,
    pub roots: Vec<(PathBuf, AccessKind)>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobFile {
    bundle: Option<PathBuf>,
    profile: Option<PathBuf>,
    plugins: Option<PathBuf>,
    workspace: Option<PathBuf>,
    state_dir: Option<PathBuf>,
    store: Option<PathBuf>,
    session_id: Option<String>,
    input: Option<String>,
    #[serde(default)]
    roots: Vec<JobRoot>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobRoot {
    path: PathBuf,
    access: Option<String>,
}

impl Job {
    pub fn load(path: &Path) -> Result<Self, KernelError> {
        let raw = std::fs::read_to_string(path).map_err(|error| KernelError::Io {
            message: format!("cannot read job `{}`: {error}", path.display()),
        })?;
        let file: JobFile = toml::from_str(&raw).map_err(|error| KernelError::InvalidConfig {
            message: format!("invalid job `{}`: {error}", path.display()),
        })?;
        if file.bundle.is_some() && (file.profile.is_some() || file.plugins.is_some()) {
            return Err(invalid(
                "a job may set either `bundle` or `profile`/`plugins`, not both",
            ));
        }
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let resolve = |value: &PathBuf| {
            if value.is_absolute() {
                value.clone()
            } else {
                base.join(value)
            }
        };
        let mut roots = Vec::with_capacity(file.roots.len());
        for root in file.roots {
            roots.push((resolve(&root.path), parse_access(root.access.as_deref())?));
        }
        Ok(Self {
            bundle: file.bundle.as_ref().map(resolve),
            profile: file.profile.as_ref().map(resolve),
            plugins: file.plugins.as_ref().map(resolve),
            workspace: file.workspace.as_ref().map(resolve),
            state_dir: file.state_dir.as_ref().map(resolve),
            store: file.store.as_ref().map(resolve),
            session_id: file.session_id,
            input: file.input,
            roots,
        })
    }
}

fn parse_access(value: Option<&str>) -> Result<AccessKind, KernelError> {
    match value.map(str::to_ascii_lowercase).as_deref() {
        None | Some("rw") | Some("write") | Some("read-write") => Ok(AccessKind::Write),
        Some("ro") | Some("read") | Some("read-only") => Ok(AccessKind::Read),
        Some(other) => Err(invalid(format!(
            "unknown root access `{other}`; use `ro` or `rw`"
        ))),
    }
}

fn invalid(message: impl Into<String>) -> KernelError {
    KernelError::InvalidConfig {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_job(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("job.toml");
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn job_paths_are_relative_to_the_job_file() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("tasks");
        std::fs::create_dir(&nested).unwrap();
        std::fs::create_dir(nested.join("ws")).unwrap();
        std::fs::create_dir(nested.join("extra")).unwrap();
        let path = write_job(
            &nested,
            "profile = \"../profile.toml\"\nworkspace = \"ws\"\nstate_dir = \"state\"\ninput = \"tidy\"\n\n[[roots]]\npath = \"extra\"\naccess = \"ro\"\n",
        );

        let job = Job::load(&path).unwrap();

        assert_eq!(job.profile, Some(nested.join("../profile.toml")));
        assert_eq!(job.workspace, Some(nested.join("ws")));
        assert_eq!(job.state_dir, Some(nested.join("state")));
        assert_eq!(job.input.as_deref(), Some("tidy"));
        assert_eq!(job.roots.len(), 1);
        assert_eq!(job.roots[0], (nested.join("extra"), AccessKind::Read));
    }

    #[test]
    fn access_defaults_to_write_and_accepts_read_aliases() {
        assert_eq!(parse_access(None).unwrap(), AccessKind::Write);
        assert_eq!(parse_access(Some("rw")).unwrap(), AccessKind::Write);
        assert_eq!(parse_access(Some("RO")).unwrap(), AccessKind::Read);
        assert_eq!(parse_access(Some("write")).unwrap(), AccessKind::Write);
        assert!(parse_access(Some("execute")).is_err());
    }

    #[test]
    fn bundle_and_profile_cannot_be_combined() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_job(
            dir.path(),
            "bundle = \"bundle.toml\"\nprofile = \"profile.toml\"\n",
        );
        assert!(Job::load(&path).is_err());
    }

    #[test]
    fn an_unknown_field_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_job(dir.path(), "profile = \"profile.toml\"\ntypo = true\n");
        assert!(Job::load(&path).is_err());
    }
}
