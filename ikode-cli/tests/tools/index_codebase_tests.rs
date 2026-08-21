#[test]
fn spec_takes_no_required_params() {
    let t = crate::tool("index_codebase");
    assert!(crate::has_description(&t));
    assert!(crate::required(&t).is_empty());
}
