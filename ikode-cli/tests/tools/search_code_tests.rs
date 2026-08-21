use ikode::tools::SearchCodeArgs;

#[test]
fn spec_requires_query_and_offers_k() {
    let t = crate::tool("search_code");
    assert!(crate::has_description(&t));
    assert_eq!(crate::required(&t), vec!["query".to_string()]);
    assert!(crate::param_keys(&t).contains(&"k".to_string()));
}

#[test]
fn args_deserialize() {
    let a: SearchCodeArgs = serde_json::from_str(r#"{"query":"retry","k":5}"#).unwrap();
    assert_eq!(a.query, "retry");
    assert_eq!(a.k, Some(5));
}
