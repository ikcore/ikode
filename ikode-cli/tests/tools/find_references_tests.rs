use ikode::tools::FindReferencesArgs;

#[test]
fn spec_requires_symbol_and_offers_depth_direction() {
    let t = crate::tool("find_references");
    assert!(crate::has_description(&t));
    assert_eq!(crate::required(&t), vec!["symbol".to_string()]);
    let keys = crate::param_keys(&t);
    for k in ["symbol", "depth", "direction"] {
        assert!(keys.contains(&k.to_string()), "missing param {k}");
    }
}

#[test]
fn args_deserialize() {
    let a: FindReferencesArgs =
        serde_json::from_str(r#"{"symbol":"Indexer","direction":"callees","depth":3}"#).unwrap();
    assert_eq!(a.symbol, "Indexer");
    assert_eq!(a.direction.as_deref(), Some("callees"));
    assert_eq!(a.depth, Some(3));
}
