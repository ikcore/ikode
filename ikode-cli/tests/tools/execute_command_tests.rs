use ikode::settings::{is_mutating, Mode};
use ikode::tools::{self, ExecuteCommandArgs};

#[test]
fn spec_requires_a_command() {
    let t = crate::tool("execute_command");
    assert!(crate::has_description(&t));
    assert_eq!(crate::required(&t), vec!["command".to_string()]);
}

#[test]
fn args_deserialize() {
    let a: ExecuteCommandArgs = serde_json::from_str(r#"{"command":"cargo test"}"#).unwrap();
    assert_eq!(a.command, "cargo test");
    assert_eq!(a.timeout_seconds, None);
}

#[test]
fn is_mutating_and_withheld_in_plan_mode() {
    assert!(is_mutating("execute_command"));
    let plan = tools::get_tools(Mode::Plan, false);
    assert!(!plan.iter().any(|t| t.name == "execute_command"));
}
