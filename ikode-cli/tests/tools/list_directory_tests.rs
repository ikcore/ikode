use ikode::settings::Mode;
use ikode::tools::{self, ListDirectoryArgs};

#[test]
fn spec_path_is_optional() {
    let t = crate::tool("list_directory");
    assert!(crate::has_description(&t));
    assert!(crate::required(&t).is_empty());
    assert!(crate::param_keys(&t).contains(&"path".to_string()));
}

#[test]
fn args_deserialize_with_and_without_path() {
    let a: ListDirectoryArgs = serde_json::from_str(r#"{"path":"src"}"#).unwrap();
    assert_eq!(a.path.as_deref(), Some("src"));
    let b: ListDirectoryArgs = serde_json::from_str(r#"{}"#).unwrap();
    assert_eq!(b.path, None);
}

#[test]
fn read_only_tool_present_in_plan_mode() {
    assert!(tools::get_tools(Mode::Plan, false)
        .iter()
        .any(|t| t.name == "list_directory"));
}
