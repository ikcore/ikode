use gaise_client::{GaiseClientConfig, GaiseClientService};
use gaise_core::GaiseClient;
use gaise_core::contracts::{
    GaiseInstructRequest, GaiseInstructStreamResponse, GaiseStreamChunk, OneOrMany, GaiseMessage,
};
use futures_util::{Stream, StreamExt};
use async_trait::async_trait;
use std::pin::Pin;
use std::sync::Arc;

struct MockClient;

#[async_trait]
impl GaiseClient for MockClient {
    async fn instruct_stream(
        &self,
        _request: &GaiseInstructRequest,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<GaiseInstructStreamResponse, Box<dyn std::error::Error + Send + Sync>>> + Send>>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let chunks = vec![
            Ok(GaiseInstructStreamResponse {
                chunk: GaiseStreamChunk::Text("Hello".to_string()),
                external_id: None,
            }),
            Ok(GaiseInstructStreamResponse {
                chunk: GaiseStreamChunk::Text("".to_string()),
                external_id: None,
            }),
            Ok(GaiseInstructStreamResponse {
                chunk: GaiseStreamChunk::Text(" World".to_string()),
                external_id: None,
            }),
        ];
        let stream = futures_util::stream::iter(chunks);
        Ok(Box::pin(stream))
    }

    async fn instruct(
        &self,
        _request: &GaiseInstructRequest,
    ) -> Result<gaise_core::contracts::GaiseInstructResponse, Box<dyn std::error::Error + Send + Sync>> {
        todo!()
    }

    async fn embeddings(
        &self,
        _request: &gaise_core::contracts::GaiseEmbeddingsRequest,
    ) -> Result<gaise_core::contracts::GaiseEmbeddingsResponse, Box<dyn std::error::Error + Send + Sync>> {
        todo!()
    }
}

#[tokio::test]
async fn test_stream_filters_empty_chunks() {
    let config = GaiseClientConfig::default();
    let service = GaiseClientService::new(config);
    
    let mock_client = Arc::new(MockClient);
    service.add_client("mock", mock_client).await;

    let request = GaiseInstructRequest {
        model: "mock::test-model".to_string(),
        input: OneOrMany::One(GaiseMessage::default()),
        ..Default::default()
    };

    let mut stream = service.instruct_stream(&request).await.unwrap();
    
    let mut received_chunks = Vec::new();
    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.unwrap();
        if let GaiseStreamChunk::Text(t) = chunk.chunk {
            received_chunks.push(t);
        }
    }

    assert_eq!(received_chunks.len(), 2);
    assert_eq!(received_chunks[0], "Hello");
    assert_eq!(received_chunks[1], " World");
}
