use ikode::tools::AskCodebaseArgs;

#[test]
fn spec_requires_question_and_offers_k() {
    let t = crate::tool("ask_codebase");
    assert!(crate::has_description(&t));
    assert_eq!(crate::required(&t), vec!["question".to_string()]);
    assert!(crate::param_keys(&t).contains(&"k".to_string()));
}

#[test]
fn args_deserialize() {
    let a: AskCodebaseArgs =
        serde_json::from_str(r#"{"question":"how does retry work?"}"#).unwrap();
    assert_eq!(a.question, "how does retry work?");
    assert_eq!(a.k, None);
}
