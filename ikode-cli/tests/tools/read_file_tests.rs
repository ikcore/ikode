use ikode::tools::ReadFileArgs;

#[test]
fn spec_requires_path_and_offers_offset_limit() {
    let t = crate::tool("read_file");
    assert!(crate::has_description(&t));
    assert_eq!(crate::required(&t), vec!["path".to_string()]);
    let keys = crate::param_keys(&t);
    for k in ["path", "offset", "limit"] {
        assert!(keys.contains(&k.to_string()), "missing param {k}");
    }
}

#[test]
fn args_deserialize_with_optional_fields() {
    let a: ReadFileArgs = serde_json::from_str(r#"{"path":"src/x.rs","offset":10}"#).unwrap();
    assert_eq!(a.path, "src/x.rs");
    assert_eq!(a.offset, Some(10));
    assert_eq!(a.limit, None);
}
