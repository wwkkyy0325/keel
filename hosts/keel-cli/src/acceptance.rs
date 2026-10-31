use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use keel_core::{AcceptanceItem, KernelError, VerifySpec};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptanceFile {
    items: Vec<AcceptanceItemFile>,
}

#[derive(Deserialize)]
struct AcceptanceItemFile {
    id: String,
    description: String,
    #[serde(default = "default_required")]
    required: bool,
    #[serde(flatten)]
    verify: VerifySpecFile,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum VerifySpecFile {
    Compute {
        expression: String,
        expected: String,
    },
    Command {
        program: String,
        #[serde(default)]
        args: Vec<String>,
        expected_exit: i32,
    },
    Test {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
    FileHash {
        path: String,
        sha256: String,
    },
    Human {
        prompt: String,
    },
    Unverifiable {
        reason: String,
    },
}

fn default_required() -> bool {
    true
}

/// Loads a host-provided acceptance checklist.
pub fn load(path: &Path) -> Result<Vec<AcceptanceItem>, KernelError> {
    let raw = std::fs::read_to_string(path).map_err(|error| KernelError::InvalidConfig {
        message: format!("cannot read acceptance file `{}`: {error}", path.display()),
    })?;
    let file: AcceptanceFile =
        toml::from_str(&raw).map_err(|error| KernelError::InvalidConfig {
            message: format!("invalid acceptance file `{}`: {error}", path.display()),
        })?;
    if file.items.is_empty() {
        return Err(KernelError::InvalidConfig {
            message: "acceptance file has no items".to_owned(),
        });
    }

    let mut seen = BTreeSet::new();
    file.items
        .into_iter()
        .map(|item| {
            if item.id.trim().is_empty() {
                return Err(KernelError::InvalidConfig {
                    message: "acceptance item id must not be empty".to_owned(),
                });
            }
            if !seen.insert(item.id.clone()) {
                return Err(KernelError::InvalidConfig {
                    message: format!("duplicate acceptance item `{}`", item.id),
                });
            }
            Ok(AcceptanceItem {
                id: item.id,
                description: item.description,
                required: item.required,
                verify: item.verify.into(),
            })
        })
        .collect()
}

impl From<VerifySpecFile> for VerifySpec {
    fn from(spec: VerifySpecFile) -> Self {
        match spec {
            VerifySpecFile::Compute {
                expression,
                expected,
            } => Self::Compute {
                expression,
                expected,
            },
            VerifySpecFile::Command {
                program,
                args,
                expected_exit,
            } => Self::Command {
                program,
                args,
                expected_exit,
            },
            VerifySpecFile::Test { program, args } => Self::Test { program, args },
            VerifySpecFile::FileHash { path, sha256 } => Self::FileHash {
                path: PathBuf::from(path),
                sha256,
            },
            VerifySpecFile::Human { prompt } => Self::Human { prompt },
            VerifySpecFile::Unverifiable { reason } => Self::Unverifiable { reason },
        }
    }
}
