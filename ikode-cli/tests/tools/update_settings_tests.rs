use ikode::settings::{is_mutating, Mode};
use ikode::tools::{self, UpdateSettingsArgs};

#[test]
fn spec_requires_key_and_value() {
    let t = crate::tool("update_settings");
    assert!(crate::has_description(&t));
    let mut req = crate::required(&t);
    req.sort();
    assert_eq!(req, vec!["key".to_string(), "value".to_string()]);
}

#[test]
fn args_deserialize() {
    let a: UpdateSettingsArgs =
        serde_json::from_str(r#"{"key":"summarize_tests","value":"true"}"#).unwrap();
    assert_eq!(a.key, "summarize_tests");
    assert_eq!(a.value, "true");
}

#[test]
fn is_mutating_and_withheld_in_plan_mode() {
    // Writing config changes the world, so it is gated like other mutating tools and
    // withheld entirely in read-only plan mode.
    assert!(is_mutating("update_settings"));
    assert!(!tools::get_tools(Mode::Plan, false)
        .iter()
        .any(|t| t.name == "update_settings"));
}
