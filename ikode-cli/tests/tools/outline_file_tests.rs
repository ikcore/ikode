use ikode::tools::OutlineFileArgs;

#[test]
fn spec_requires_a_path() {
    let t = crate::tool("outline_file");
    assert!(crate::has_description(&t));
    assert_eq!(crate::required(&t), vec!["path".to_string()]);
}

#[test]
fn args_deserialize() {
    let a: OutlineFileArgs = serde_json::from_str(r#"{"path":"src/lib.rs"}"#).unwrap();
    assert_eq!(a.path, "src/lib.rs");
}
