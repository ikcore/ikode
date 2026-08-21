use ikode::tools::TodoInsertArgs;

#[test]
fn spec_requires_before_id_and_task() {
    let t = crate::tool("todo_insert");
    assert!(crate::has_description(&t));
    let mut req = crate::required(&t);
    req.sort();
    assert_eq!(req, vec!["before_id".to_string(), "task".to_string()]);
}

#[test]
fn args_deserialize() {
    let a: TodoInsertArgs = serde_json::from_str(r#"{"before_id":3,"task":"do x"}"#).unwrap();
    assert_eq!(a.before_id, 3);
    assert_eq!(a.task, "do x");
}
