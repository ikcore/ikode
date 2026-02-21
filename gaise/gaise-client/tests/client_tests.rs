use gaise_client::{GaiseClientService, GaiseClientConfig};
use gaise_core::GaiseClient;
use gaise_core::contracts::GaiseInstructRequest;

#[tokio::test]
async fn test_provider_resolution() {
    let config = GaiseClientConfig {
        ollama_url: Some("http://localhost:11434".to_string()),
        ..Default::default()
    };
    let service = GaiseClientService::new(config);

    // Test resolving ollama
    let client = service.get_client("ollama").await;
    assert!(client.is_ok());

    // Test resolving unknown provider
    let client = service.get_client("unknown").await;
    assert!(client.is_err());
}

#[tokio::test]
async fn test_instruct_delegation_parsing() {
    let config = GaiseClientConfig {
        ollama_url: Some("http://localhost:11434".to_string()),
        ..Default::default()
    };
    let service = GaiseClientService::new(config);

    let request = GaiseInstructRequest {
        model: "ollama::llama3".to_string(),
        input: gaise_core::contracts::OneOrMany::Many(vec![]),
        generation_config: None,
        tools: None,
        tool_config: None,
    };

    // We can't easily mock the underlying clients without changing their traits or using a mock library
    // but we can at least check if it fails with the right error if a provider is missing config
    let request_vertex = GaiseInstructRequest {
        model: "vertexai::gemini-pro".to_string(),
        ..request.clone()
    };
    
    let result = service.instruct(&request_vertex).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("VertexAI Service Account not configured"));
}

#[tokio::test]
async fn test_embeddings_delegation_parsing() {
    let service = GaiseClientService::new(GaiseClientConfig::default());

    let request = gaise_core::contracts::GaiseEmbeddingsRequest {
        model: "openai::text-embedding-3-small".to_string(),
        input: gaise_core::contracts::OneOrMany::One("hello".to_string()),
    };

    let result = service.embeddings(&request).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("OpenAI API Key not configured"));
}

#[tokio::test]
#[cfg(feature = "bedrock")]
async fn test_bedrock_resolution() {
    let config = GaiseClientConfig {
        bedrock_region: Some("us-east-1".to_string()),
        ..Default::default()
    };
    let service = GaiseClientService::new(config);

    // Test resolving bedrock
    // Note: This actually calls BedrockClient::new() which might try to load credentials
    // but at least we can check if it initializes without error in this environment
    // or if it fails gracefully.
    let client = service.get_client("bedrock").await;
    assert!(client.is_ok());
}
