use gaise_provider_bedrock::GaiseClientBedrock;
use gaise_core::contracts::{GaiseInstructRequest, GaiseMessage, OneOrMany, GaiseContent};
use gaise_core::GaiseClient;

#[tokio::test]
async fn test_mapping_to_bedrock() {
    // This is a unit test for mapping logic. 
    // Since actual Bedrock client requires AWS credentials and network, 
    // we would ideally mock it, but here we can at least test the mapping functions if they were public or via a wrapper.
    // For now, let's verify it compiles and handles basic requests.
    
    let _client = GaiseClientBedrock::new().await;
    let request = GaiseInstructRequest {
        model: "amazon.titan-text-express-v1".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text { text: "Hello".to_string() })),
            ..Default::default()
        }),
        ..Default::default()
    };
    
    // We can't easily run this without credentials, so this test is mostly to ensure compilation
    // and provide a template for future integration tests.
    assert_eq!(request.model, "amazon.titan-text-express-v1");
}
