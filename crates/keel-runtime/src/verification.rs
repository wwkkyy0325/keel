//! Deterministic acceptance verification.
//!
//! Compute and file-hash checks run here. Command and test checks wait for the
//! structured shell tool so there is only one execution path.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use keel_core::{
    AcceptanceItem, CancelToken, Evidence, EvidenceOutcome, VerificationReport, VerifySpec,
};
use sha2::{Digest, Sha256};

use crate::calculator;
use crate::command::{self, CommandSpec};

const MAX_HASH_FILE_BYTES: u64 = 16 * 1024 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// Runs acceptance items against a workspace root.
pub struct AcceptanceVerifier {
    root: PathBuf,
    allowlist: Vec<String>,
}

impl AcceptanceVerifier {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            allowlist: command::coding_allowlist(),
        }
    }

    /// Verifies every item and summarizes the required failures.
    pub async fn verify(&self, items: &[AcceptanceItem]) -> VerificationReport {
        let mut report = VerificationReport {
            evidence: Vec::with_capacity(items.len()),
            required_failed: Vec::new(),
            required_unverified: Vec::new(),
            optional_failed: Vec::new(),
        };

        for item in items {
            let evidence = self.verify_item(item).await;
            match evidence.outcome {
                EvidenceOutcome::Passed => {}
                EvidenceOutcome::Failed if item.required => {
                    report.required_failed.push(item.id.clone());
                }
                EvidenceOutcome::Failed => report.optional_failed.push(item.id.clone()),
                EvidenceOutcome::Unverified | EvidenceOutcome::Deferred if item.required => {
                    report.required_unverified.push(item.id.clone());
                }
                EvidenceOutcome::Unverified | EvidenceOutcome::Deferred => {}
            }
            report.evidence.push(evidence);
        }

        report
    }

    async fn verify_item(&self, item: &AcceptanceItem) -> Evidence {
        match &item.verify {
            VerifySpec::Compute {
                expression,
                expected,
            } => verify_compute(&item.id, expression, expected),
            VerifySpec::FileHash { path, sha256 } => {
                self.verify_file_hash(&item.id, path, sha256).await
            }
            VerifySpec::Command {
                program,
                args,
                expected_exit,
            } => {
                self.verify_command(&item.id, "command", program, args, Some(*expected_exit))
                    .await
            }
            VerifySpec::Test { program, args } => {
                self.verify_command(&item.id, "test", program, args, Some(0))
                    .await
            }
            VerifySpec::Human { prompt } => unverified(
                &item.id,
                "human",
                format!("requires human confirmation: {prompt}"),
            ),
            VerifySpec::Unverifiable { reason } => {
                unverified(&item.id, "unverifiable", reason.clone())
            }
        }
    }

    /// Whether a check may run `program`. A bare name must be on the
    /// allowlist. A project interpreter is named by an absolute path inside
    /// the workspace: the workspace owns the file, and the file name still has
    /// to be allowlisted, so `.venv/bin/python` runs but `.venv/bin/anything`
    /// does not.
    fn program_allowed(&self, program: &str) -> bool {
        if command::is_allowed(program, &self.allowlist) {
            return true;
        }
        let path = Path::new(program);
        if !path.is_absolute()
            || path
                .components()
                .any(|component| component == Component::ParentDir)
            || !path.starts_with(&self.root)
            || !path.is_file()
        {
            return false;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return false;
        };
        command::is_allowed(name, &self.allowlist)
    }

    async fn verify_command(
        &self,
        item_id: &str,
        verifier: &str,
        program: &str,
        args: &[String],
        expected_exit: Option<i32>,
    ) -> Evidence {
        if !self.program_allowed(program) {
            return failed(
                item_id,
                verifier,
                format!("program `{program}` is not allowlisted"),
            );
        }
        let spec = CommandSpec {
            program,
            args,
            cwd: &self.root,
            timeout: COMMAND_TIMEOUT,
            max_duration: None,
        };
        let started = std::time::Instant::now();
        let output = match command::run(spec, &CancelToken::new()).await {
            Ok(output) => output,
            Err(error) => return failed(item_id, verifier, error.to_string()),
        };
        let elapsed = started.elapsed().as_secs_f64();
        let expected = expected_exit.unwrap_or(0);
        let mut evidence = if output.exit_code == Some(expected) {
            Evidence {
                item_id: item_id.to_owned(),
                verifier: verifier.to_owned(),
                outcome: EvidenceOutcome::Passed,
                detail: format!("exit code {expected}\n{}", output.output),
            }
        } else {
            failed(
                item_id,
                verifier,
                format!(
                    "expected exit code {expected}, got {:?}\n{}",
                    output.exit_code, output.output
                ),
            )
        };
        // A check's cost is part of its report: it tells a host whether the
        // full suite is affordable on this workspace.
        evidence
            .detail
            .push_str(&format!("\n[duration: {elapsed:.2} s]"));
        evidence
    }

    async fn verify_file_hash(&self, item_id: &str, path: &Path, expected: &str) -> Evidence {
        let relative = match relative_workspace_path(path) {
            Ok(relative) => relative,
            Err(message) => return failed(item_id, "file_hash", message),
        };
        let full_path = self.root.join(relative);

        let metadata = match tokio::fs::metadata(&full_path).await {
            Ok(metadata) => metadata,
            Err(error) => {
                return failed(
                    item_id,
                    "file_hash",
                    format!("cannot read `{}`: {error}", relative.display()),
                )
            }
        };
        if metadata.len() > MAX_HASH_FILE_BYTES {
            return failed(
                item_id,
                "file_hash",
                format!(
                    "`{}` exceeds {MAX_HASH_FILE_BYTES} bytes",
                    relative.display()
                ),
            );
        }

        let bytes = match tokio::fs::read(&full_path).await {
            Ok(bytes) => bytes,
            Err(error) => {
                return failed(
                    item_id,
                    "file_hash",
                    format!("cannot read `{}`: {error}", relative.display()),
                )
            }
        };
        let actual = format!("{:x}", Sha256::digest(&bytes));
        let expected = expected.trim();
        if actual.eq_ignore_ascii_case(expected) {
            Evidence {
                item_id: item_id.to_owned(),
                verifier: "file_hash".to_owned(),
                outcome: EvidenceOutcome::Passed,
                detail: format!("sha256 {}", actual),
            }
        } else {
            failed(
                item_id,
                "file_hash",
                format!("expected {expected}, got {actual}"),
            )
        }
    }
}

fn verify_compute(item_id: &str, expression: &str, expected: &str) -> Evidence {
    let actual = match calculator::evaluate(expression) {
        Ok(value) => value,
        Err(message) => return failed(item_id, "compute", message),
    };
    let expected = match expected.trim().parse::<f64>() {
        Ok(value) => value,
        Err(_) => {
            return failed(
                item_id,
                "compute",
                format!("expected value `{expected}` is not a number"),
            )
        }
    };
    if close_enough(actual, expected) {
        Evidence {
            item_id: item_id.to_owned(),
            verifier: "compute".to_owned(),
            outcome: EvidenceOutcome::Passed,
            detail: format!("{expression} = {actual}"),
        }
    } else {
        failed(
            item_id,
            "compute",
            format!("{expression} = {actual}, expected {expected}"),
        )
    }
}

fn close_enough(actual: f64, expected: f64) -> bool {
    let scale = actual.abs().max(expected.abs()).max(1.0);
    (actual - expected).abs() <= 1e-9 * scale
}

fn relative_workspace_path(path: &Path) -> Result<&Path, String> {
    if path.as_os_str().is_empty() {
        return Err("file hash path must not be empty".to_owned());
    }
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "file hash path `{}` must stay inside the workspace",
            path.display()
        ));
    }
    Ok(path)
}

fn failed(item_id: &str, verifier: &str, detail: String) -> Evidence {
    Evidence {
        item_id: item_id.to_owned(),
        verifier: verifier.to_owned(),
        outcome: EvidenceOutcome::Failed,
        detail,
    }
}

fn unverified(item_id: &str, verifier: &str, detail: impl Into<String>) -> Evidence {
    Evidence {
        item_id: item_id.to_owned(),
        verifier: verifier.to_owned(),
        outcome: EvidenceOutcome::Unverified,
        detail: detail.into(),
    }
}
