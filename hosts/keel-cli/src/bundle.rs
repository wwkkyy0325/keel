use std::path::{Path, PathBuf};

use keel_core::KernelError;
use serde::Deserialize;

/// A composition bundle: the profile, optional plugin file and optional state
/// directory. Every path is resolved relative to the bundle file, so a bundle
/// works from any working directory.
#[derive(Debug)]
pub struct Bundle {
    pub profile: PathBuf,
    pub plugins: Option<PathBuf>,
    pub state_dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct BundleFile {
    profile: PathBuf,
    plugins: Option<PathBuf>,
    state_dir: Option<PathBuf>,
}

impl Bundle {
    pub fn load(path: &Path) -> Result<Self, KernelError> {
        let raw = std::fs::read_to_string(path).map_err(|error| KernelError::Io {
            message: format!("cannot read bundle `{}`: {error}", path.display()),
        })?;
        let file: BundleFile =
            toml::from_str(&raw).map_err(|error| KernelError::InvalidConfig {
                message: format!("invalid bundle `{}`: {error}", path.display()),
            })?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let resolve = |value: &PathBuf| {
            if value.is_absolute() {
                value.clone()
            } else {
                base.join(value)
            }
        };
        Ok(Self {
            profile: resolve(&file.profile),
            plugins: file.plugins.as_ref().map(resolve),
            state_dir: file.state_dir.as_ref().map(resolve),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_paths_are_relative_to_the_bundle_file() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("coding");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("profile.toml"), "").unwrap();
        std::fs::write(nested.join("plugins.toml"), "").unwrap();
        let bundle_path = nested.join("bundle.toml");
        std::fs::write(
            &bundle_path,
            "name = \"coding\"\nprofile = \"profile.toml\"\nplugins = \"plugins.toml\"\nstate_dir = \"../../state\"\n",
        )
        .unwrap();

        let bundle = Bundle::load(&bundle_path).unwrap();

        assert_eq!(bundle.profile, nested.join("profile.toml"));
        assert_eq!(bundle.plugins, Some(nested.join("plugins.toml")));
        assert_eq!(bundle.state_dir, Some(nested.join("../../state")));
    }

    #[test]
    fn an_absolute_path_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("profile.toml");
        let bundle_path = dir.path().join("bundle.toml");
        let escaped = profile.display().to_string().replace('\\', "\\\\");
        std::fs::write(&bundle_path, format!("profile = \"{escaped}\"\n")).unwrap();

        let bundle = Bundle::load(&bundle_path).unwrap();

        assert_eq!(bundle.profile, profile);
    }

    #[test]
    fn a_missing_bundle_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Bundle::load(&dir.path().join("missing.toml")).is_err());
    }

    #[test]
    fn an_invalid_bundle_is_a_config_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bundle.toml");
        std::fs::write(&path, "not = [valid").unwrap();
        assert!(Bundle::load(&path).is_err());
    }
}
