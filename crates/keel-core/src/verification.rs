//! Acceptance checklist contracts.
//!
//! The model proposes items; only a verifier produces `Evidence`. Final text
//! is never evidence on its own.

use std::path::PathBuf;

/// One item in a plan's acceptance checklist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceItem {
    pub id: String,
    pub description: String,
    pub required: bool,
    pub verify: VerifySpec,
}

/// How an acceptance item is checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifySpec {
    /// Evaluate an arithmetic expression and compare with `expected`.
    Compute {
        expression: String,
        expected: String,
    },
    /// Run a program with a fixed argument list and expect an exit code.
    Command {
        program: String,
        args: Vec<String>,
        expected_exit: i32,
    },
    /// Run a program with a fixed argument list and expect exit code zero.
    Test { program: String, args: Vec<String> },
    /// Compare the SHA-256 of a workspace-relative file.
    FileHash { path: PathBuf, sha256: String },
    /// Requires an explicit human answer.
    Human { prompt: String },
    /// The item cannot be checked automatically.
    Unverifiable { reason: String },
}

/// Outcome recorded by a verifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceOutcome {
    Passed,
    Failed,
    /// No verifier ran, or the verifier is not available yet.
    Unverified,
    /// An interaction was deferred, so the dependent item was not checked.
    Deferred,
}

/// Typed evidence produced by a verifier. The model cannot construct this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub item_id: String,
    pub verifier: String,
    pub outcome: EvidenceOutcome,
    pub detail: String,
}

/// Result of verifying a checklist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationReport {
    pub evidence: Vec<Evidence>,
    pub required_failed: Vec<String>,
    pub required_unverified: Vec<String>,
    pub optional_failed: Vec<String>,
}

impl VerificationReport {
    /// A report passes only when every required item has passing evidence.
    pub fn passed(&self) -> bool {
        self.required_failed.is_empty() && self.required_unverified.is_empty()
    }
}
