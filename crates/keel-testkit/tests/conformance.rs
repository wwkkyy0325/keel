use keel_core::{ArgValue, ErrorCode, ModelError, Session, ToolArgs, SNAPSHOT_VERSION};
use keel_runtime::MockStep;
use keel_testkit::{
    assert_cancelled, assert_done, assert_event_order, assert_event_present, assert_failed,
    event_names_contract, text_turn_events, tool_turn_events, Probe,
};

fn echo_args() -> ToolArgs {
    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::String("hello".to_owned()));
    args
}

#[tokio::test]
async fn a_text_turn_follows_the_event_contract() {
    let probe = Probe::new(vec![MockStep::Text("hello".to_owned())]).unwrap();
    let outcome = probe.run("hi").await;

    assert_done(&outcome).unwrap();
    assert_event_order(probe.events(), &text_turn_events()).unwrap();
}

#[tokio::test]
async fn a_tool_turn_dispatches_and_audits_the_call() {
    let probe = Probe::new(vec![
        MockStep::ToolCall {
            name: "echo".to_owned(),
            args: echo_args(),
        },
        MockStep::Text("done".to_owned()),
    ])
    .unwrap();
    let outcome = probe.run("echo hello").await;

    assert_done(&outcome).unwrap();
    assert_event_order(probe.events(), &tool_turn_events()).unwrap();
    assert_event_present(probe.events(), "tool_finished").unwrap();
}

#[tokio::test]
async fn a_model_failure_is_audited() {
    let probe = Probe::new(vec![MockStep::Error(ModelError::provider("boom"))]).unwrap();
    let outcome = probe.run("hi").await;

    assert_failed(&outcome, ErrorCode::ModelProvider).unwrap();
    assert_event_present(probe.events(), "model_failed").unwrap();
}

#[tokio::test]
async fn a_cancelled_turn_is_terminal() {
    let probe = Probe::new(vec![MockStep::Hang]).unwrap();
    let outcome = probe.run_cancelled("hi").await;

    assert_cancelled(&outcome).unwrap();
    assert_event_present(probe.events(), "turn_cancelled").unwrap();
}

#[tokio::test]
async fn the_snapshot_version_is_enforced() {
    let probe = Probe::new(vec![MockStep::Text("hello".to_owned())]).unwrap();
    assert_done(&probe.run("hi").await).unwrap();

    let snapshot = probe.snapshot();
    assert_eq!(snapshot.format_version, SNAPSHOT_VERSION);

    let mut newer = snapshot;
    newer.format_version = SNAPSHOT_VERSION + 1;
    assert!(
        Session::restore(newer).is_err(),
        "a newer snapshot must be refused, not guessed"
    );
}

const _: () = assert!(!keel_core::KERNEL_VERSION.is_empty());
const _: () = assert!(keel_core::EVENT_FORMAT_VERSION >= 1);
const _: () = assert!(SNAPSHOT_VERSION >= 1);

#[test]
fn every_event_variant_is_in_the_contract_list() {
    let names = event_names_contract();
    for required in [
        "model_finished",
        "model_failed",
        "permission_granted",
        "permission_revoked",
        "background_started",
        "background_result",
        "sub_agent_started",
        "sub_agent_finished",
    ] {
        assert!(names.iter().any(|name| name == required), "{required}");
    }
}
