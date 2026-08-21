use gaise_core::contracts::{GaiseContent, GaiseInstructRequest, GaiseMessage, OneOrMany};

#[test]
fn test_request_contract_without_initializing_aws() {
    // Keep this test hermetic: `GaiseClientBedrock::new()` loads the AWS credential
    // chain and may probe instance metadata. Request-contract tests must never
    // initialize a real SDK client or touch the network.
    let request = GaiseInstructRequest {
        model: "amazon.titan-text-express-v1".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Hello".to_string(),
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    assert_eq!(request.model, "amazon.titan-text-express-v1");
    let encoded = serde_json::to_value(&request).unwrap();
    assert_eq!(encoded["model"], "amazon.titan-text-express-v1");
    assert_eq!(encoded["input"]["content"]["text"], "Hello");
}
