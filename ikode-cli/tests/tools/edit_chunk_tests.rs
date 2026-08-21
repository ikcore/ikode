use ikode::settings::{is_mutating, Mode};
use ikode::tools::{self, EditChunkArgs};

#[test]
fn spec_requires_chunk_id_and_new_text() {
    let t = crate::tool("edit_chunk");
    assert!(crate::has_description(&t));
    let mut req = crate::required(&t);
    req.sort();
    assert_eq!(req, vec!["chunk_id".to_string(), "new_text".to_string()]);
}

#[test]
fn impact_depth_is_optional() {
    let t = crate::tool("edit_chunk");
    let mut keys = crate::param_keys(&t);
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "chunk_id".to_string(),
            "impact_depth".to_string(),
            "new_text".to_string()
        ]
    );
}

#[test]
fn args_deserialize_without_depth() {
    let a: EditChunkArgs =
        serde_json::from_str(r#"{"chunk_id":"src/lib.rs::foo","new_text":"fn foo() {}"}"#).unwrap();
    assert_eq!(a.chunk_id.as_str(), "src/lib.rs::foo");
    assert_eq!(a.new_text.as_str(), "fn foo() {}");
    assert_eq!(a.impact_depth, None);
}

#[test]
fn is_mutating_and_withheld_in_plan_mode() {
    assert!(is_mutating("edit_chunk"));
    assert!(!tools::get_tools(Mode::Plan, false)
        .iter()
        .any(|t| t.name == "edit_chunk"));
}
