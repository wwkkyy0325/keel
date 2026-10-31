//! Deterministic acceptance verification.
//!
//! Compute and file-hash checks run here. Command and test checks wait for the
//! structured shell tool so there is only one execution path.

use std::path::{Component, Path, PathBuf};

use keel_core::{AcceptanceItem, Evidence, EvidenceOutcome, VerificationReport, VerifySpec};
use sha2::{Digest, Sha256};

use crate::calculator;

const MAX_HASH_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Runs acceptance items against a workspace root.
pub struct AcceptanceVerifier {
    root: PathBuf,
}

impl AcceptanceVerifier {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
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
            VerifySpec::Command { .. } | VerifySpec::Test { .. } => unverified(
                &item.id,
                "command",
                "command verification requires the structured shell tool",
            ),
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
