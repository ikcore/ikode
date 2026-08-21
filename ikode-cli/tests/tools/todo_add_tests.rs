use ikode::tools::TodoAddArgs;

#[test]
fn spec_requires_a_tasks_array() {
    let t = crate::tool("todo_add");
    assert!(crate::has_description(&t));
    assert_eq!(crate::required(&t), vec!["tasks".to_string()]);
    assert!(crate::param_keys(&t).contains(&"tasks".to_string()));
}

#[test]
fn args_deserialize() {
    let a: TodoAddArgs = serde_json::from_str(r#"{"tasks":["a","b"]}"#).unwrap();
    assert_eq!(a.tasks, vec!["a".to_string(), "b".to_string()]);
}
