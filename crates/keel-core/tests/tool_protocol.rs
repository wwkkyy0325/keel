use keel_core::{ArgValue, ParamKind, ParamSpec, RiskLevel, ToolArgs, ToolSchema};

fn schema() -> ToolSchema {
    ToolSchema {
        name: "sample".to_owned(),
        description: "A sample tool.".to_owned(),
        risk: RiskLevel::ReadOnly,
        params: vec![ParamSpec {
            name: "text".to_owned(),
            kind: ParamKind::String,
            required: true,
        }],
    }
}

#[test]
fn valid_arguments_are_accepted() {
    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::String("hello".to_owned()));
    assert!(schema().validate(&args).is_ok());
    assert_eq!(args.require_str("text").unwrap(), "hello");
}

#[test]
fn missing_required_argument_is_rejected() {
    let error = schema().validate(&ToolArgs::new()).unwrap_err();
    assert_eq!(error.code, keel_core::ErrorCode::InvalidArguments);
    assert!(!error.retryable);
}

#[test]
fn wrong_argument_type_is_rejected() {
    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::Integer(7));
    let error = schema().validate(&args).unwrap_err();
    assert_eq!(error.code, keel_core::ErrorCode::InvalidArguments);
}

#[test]
fn unknown_argument_is_rejected() {
    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::String("hello".to_owned()));
    args.insert("extra", ArgValue::Boolean(true));
    let error = schema().validate(&args).unwrap_err();
    assert_eq!(error.code, keel_core::ErrorCode::InvalidArguments);
}

#[test]
fn require_str_returns_a_typed_error() {
    let mut args = ToolArgs::new();
    args.insert("text", ArgValue::Boolean(false));
    let error = args.require_str("text").unwrap_err();
    assert_eq!(error.code, keel_core::ErrorCode::InvalidArguments);
}
