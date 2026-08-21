use ikode::settings::{is_mutating, Mode};
use ikode::tools::{self, CreateFileArgs};

#[test]
fn spec_requires_path_and_content() {
    let t = crate::tool("create_file");
    assert!(crate::has_description(&t));
    let mut req = crate::required(&t);
    req.sort();
    assert_eq!(req, vec!["content".to_string(), "path".to_string()]);
}

#[test]
fn args_deserialize() {
    let a: CreateFileArgs =
        serde_json::from_str(r#"{"path":"a.rs","content":"fn x(){}"}"#).unwrap();
    assert_eq!(a.path, "a.rs");
    assert_eq!(a.content, "fn x(){}");
}

#[test]
fn is_mutating_and_withheld_in_plan_mode() {
    assert!(is_mutating("create_file"));
    assert!(!tools::get_tools(Mode::Plan, false)
        .iter()
        .any(|t| t.name == "create_file"));
}
