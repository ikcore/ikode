use ikode::settings::{is_mutating, Mode};
use ikode::tools::{self, EditFileArgs};

#[test]
fn spec_requires_path_old_and_new_text() {
    let t = crate::tool("edit_file");
    assert!(crate::has_description(&t));
    let mut req = crate::required(&t);
    req.sort();
    assert_eq!(
        req,
        vec![
            "new_text".to_string(),
            "old_text".to_string(),
            "path".to_string()
        ]
    );
}

#[test]
fn args_deserialize() {
    let a: EditFileArgs =
        serde_json::from_str(r#"{"path":"a.rs","old_text":"x","new_text":"y"}"#).unwrap();
    assert_eq!(
        (a.path.as_str(), a.old_text.as_str(), a.new_text.as_str()),
        ("a.rs", "x", "y")
    );
}

#[test]
fn is_mutating_and_withheld_in_plan_mode() {
    assert!(is_mutating("edit_file"));
    assert!(!tools::get_tools(Mode::Plan, false)
        .iter()
        .any(|t| t.name == "edit_file"));
}
