use ikode::settings::{is_mutating, Mode};
use ikode::tools::{self, DeleteFileArgs};

#[test]
fn spec_requires_a_path() {
    let t = crate::tool("delete_file");
    assert!(crate::has_description(&t));
    assert_eq!(crate::required(&t), vec!["path".to_string()]);
}

#[test]
fn args_deserialize() {
    let a: DeleteFileArgs = serde_json::from_str(r#"{"path":"a.rs"}"#).unwrap();
    assert_eq!(a.path, "a.rs");
}

#[test]
fn is_mutating_and_withheld_in_plan_mode() {
    assert!(is_mutating("delete_file"));
    assert!(!tools::get_tools(Mode::Plan, false)
        .iter()
        .any(|t| t.name == "delete_file"));
}
