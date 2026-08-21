#[test]
fn spec_takes_no_required_params() {
    let t = crate::tool("graph_overview");
    assert!(crate::has_description(&t));
    assert!(crate::required(&t).is_empty());
}
