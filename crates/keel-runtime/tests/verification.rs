use std::path::PathBuf;

use keel_core::{AcceptanceItem, EvidenceOutcome, VerifySpec};
use keel_runtime::AcceptanceVerifier;

fn item(id: &str, required: bool, verify: VerifySpec) -> AcceptanceItem {
    AcceptanceItem {
        id: id.to_owned(),
        description: format!("check {id}"),
        required,
        verify,
    }
}

#[tokio::test]
async fn compute_items_pass_or_fail_on_real_arithmetic() {
    let dir = tempfile::tempdir().unwrap();
    let verifier = AcceptanceVerifier::new(dir.path());
    let items = vec![
        item(
            "sum",
            true,
            VerifySpec::Compute {
                expression: "(2 + 3) * 4".to_owned(),
                expected: "20".to_owned(),
            },
        ),
        item(
            "wrong",
            true,
            VerifySpec::Compute {
                expression: "2 + 2".to_owned(),
                expected: "5".to_owned(),
            },
        ),
    ];

    let report = verifier.verify(&items).await;

    assert!(!report.passed());
    assert_eq!(report.required_failed, vec!["wrong".to_owned()]);
    assert_eq!(report.evidence[0].outcome, EvidenceOutcome::Passed);
    assert_eq!(report.evidence[1].outcome, EvidenceOutcome::Failed);
}

#[tokio::test]
async fn file_hash_checks_the_real_file_contents() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "hello").unwrap();
    let verifier = AcceptanceVerifier::new(dir.path());
    let items = vec![
        item(
            "hash-ok",
            true,
            VerifySpec::FileHash {
                path: PathBuf::from("hello.txt"),
                sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                    .to_owned(),
            },
        ),
        item(
            "hash-bad",
            true,
            VerifySpec::FileHash {
                path: PathBuf::from("hello.txt"),
                sha256: "00".repeat(32),
            },
        ),
    ];

    let report = verifier.verify(&items).await;

    assert_eq!(report.required_failed, vec!["hash-bad".to_owned()]);
    assert_eq!(report.evidence[0].outcome, EvidenceOutcome::Passed);
    assert_eq!(report.evidence[1].outcome, EvidenceOutcome::Failed);
}

#[tokio::test]
async fn file_hash_paths_may_not_escape_the_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let verifier = AcceptanceVerifier::new(dir.path());
    let items = vec![item(
        "escape",
        true,
        VerifySpec::FileHash {
            path: PathBuf::from("../outside.txt"),
            sha256: "00".repeat(32),
        },
    )];

    let report = verifier.verify(&items).await;

    assert!(!report.passed());
    assert_eq!(report.required_unverified.len(), 0);
    assert_eq!(report.required_failed, vec!["escape".to_owned()]);
}

#[tokio::test]
async fn optional_failures_do_not_block_the_report() {
    let dir = tempfile::tempdir().unwrap();
    let verifier = AcceptanceVerifier::new(dir.path());
    let items = vec![item(
        "nice-to-have",
        false,
        VerifySpec::Compute {
            expression: "1 + 1".to_owned(),
            expected: "3".to_owned(),
        },
    )];

    let report = verifier.verify(&items).await;

    assert!(report.passed());
    assert_eq!(report.optional_failed, vec!["nice-to-have".to_owned()]);
}

#[tokio::test]
async fn command_and_test_items_run_allowlisted_programs() {
    let dir = tempfile::tempdir().unwrap();
    let verifier = AcceptanceVerifier::new(dir.path());
    let items = vec![
        item(
            "command",
            true,
            VerifySpec::Command {
                program: "rustc".to_owned(),
                args: vec!["--version".to_owned()],
                expected_exit: 0,
            },
        ),
        item(
            "test",
            true,
            VerifySpec::Test {
                program: "rustc".to_owned(),
                args: vec!["--version".to_owned()],
            },
        ),
        item(
            "wrong-exit",
            true,
            VerifySpec::Command {
                program: "rustc".to_owned(),
                args: vec!["--version".to_owned()],
                expected_exit: 1,
            },
        ),
    ];

    let report = verifier.verify(&items).await;

    assert_eq!(report.evidence[0].outcome, EvidenceOutcome::Passed);
    assert_eq!(report.evidence[1].outcome, EvidenceOutcome::Passed);
    assert_eq!(report.evidence[2].outcome, EvidenceOutcome::Failed);
    assert_eq!(report.required_failed, vec!["wrong-exit".to_owned()]);
}

#[tokio::test]
async fn commands_outside_the_allowlist_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let verifier = AcceptanceVerifier::new(dir.path());
    let items = vec![item(
        "dangerous",
        true,
        VerifySpec::Command {
            program: "rm".to_owned(),
            args: vec!["-rf".to_owned(), ".".to_owned()],
            expected_exit: 0,
        },
    )];

    let report = verifier.verify(&items).await;

    assert_eq!(report.required_failed, vec!["dangerous".to_owned()]);
    assert!(report.evidence[0].detail.contains("not allowlisted"));
}

#[tokio::test]
async fn human_and_unverifiable_items_remain_unverified() {
    let dir = tempfile::tempdir().unwrap();
    let verifier = AcceptanceVerifier::new(dir.path());
    let items = vec![
        item(
            "human",
            true,
            VerifySpec::Human {
                prompt: "does it look right?".to_owned(),
            },
        ),
        item(
            "unknown",
            false,
            VerifySpec::Unverifiable {
                reason: "not checkable".to_owned(),
            },
        ),
    ];

    let report = verifier.verify(&items).await;

    assert!(!report.passed());
    assert_eq!(report.required_unverified, vec!["human".to_owned()]);
    assert_eq!(report.evidence[0].outcome, EvidenceOutcome::Unverified);
    assert_eq!(report.evidence[1].outcome, EvidenceOutcome::Unverified);
}
