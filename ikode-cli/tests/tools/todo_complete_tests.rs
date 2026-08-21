use ikode::tools::TodoCompleteArgs;

#[test]
fn spec_requires_an_ids_array() {
    let t = crate::tool("todo_complete");
    assert!(crate::has_description(&t));
    assert_eq!(crate::required(&t), vec!["ids".to_string()]);
}

#[test]
fn args_deserialize() {
    let a: TodoCompleteArgs = serde_json::from_str(r#"{"ids":[1,2,3]}"#).unwrap();
    assert_eq!(a.ids, vec![1, 2, 3]);
}
