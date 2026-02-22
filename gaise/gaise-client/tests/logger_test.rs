use gaise_client::{GaiseClientConfig, GaiseClientService};
use gaise_core::GaiseClient;
use gaise_core::contracts::{
    GaiseInstructRequest, GaiseInstructResponse, GaiseEmbeddingsRequest, GaiseEmbeddingsResponse,
    GaiseInstructStreamResponse, GaiseStreamChunk, OneOrMany, GaiseMessage,
};
use gaise_core::logging::IGaiseLogger;
use futures_util::{Stream, StreamExt};
use async_trait::async_trait;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use serde_json::Value;
#[derive(Debug, Default)]
struct TestLogger {
    logs: Arc<Mutex<Vec<String>>>,
}
impl IGaiseLogger for TestLogger {
    fn log_request(&self, cid: Option<&str>, req_type: &str, model: &str, _json: Value) {
        let mut logs = self.logs.lock().unwrap();
        logs.push(format!("REQ: cid={:?}, type={}, model={}", cid, req_type, model));
    }
    fn log_response(&self, cid: Option<&str>, req_type: &str, model: &str, _json: Value, _usage: Option<Value>) {
        let mut logs = self.logs.lock().unwrap();
        logs.push(format!("RES: cid={:?}, type={}, model={}", cid, req_type, model));
    }
    fn log_stream_chunk(&self, cid: Option<&str>, req_type: &str, model: &str, _json: Value) {
        let mut logs = self.logs.lock().unwrap();
        logs.push(format!("CHUNK: cid={:?}, type={}, model={}", cid, req_type, model));
    }
}
struct MockClient;
#[async_trait]
impl GaiseClient for MockClient {
    async fn instruct(&self, _req: &GaiseInstructRequest) -> Result<GaiseInstructResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(GaiseInstructResponse {
            output: OneOrMany::One(GaiseMessage::default()),
            external_id: None,
            usage: None,
        })
    }
    async fn instruct_stream(&self, _req: &GaiseInstructRequest) -> Result<Pin<Box<dyn Stream<Item = Result<GaiseInstructStreamResponse, Box<dyn std::error::Error + Send + Sync>>> + Send>>, Box<dyn std::error::Error + Send + Sync>> {
        let chunks = vec![
            Ok(GaiseInstructStreamResponse {
                chunk: GaiseStreamChunk::Text("hi".to_string()),
                external_id: None,
            }),
        ];
        Ok(Box::pin(futures_util::stream::iter(chunks)))
    }
    async fn embeddings(&self, _req: &GaiseEmbeddingsRequest) -> Result<GaiseEmbeddingsResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(GaiseEmbeddingsResponse {
            external_id: None,
            output: vec![vec![0.1]],
            usage: None,
        })
    }
}
#[tokio::test]
async fn test_logger_integration() {
    let logs = Arc::new(Mutex::new(Vec::new()));
    let logger = Arc::new(TestLogger { logs: logs.clone() });

    let config = GaiseClientConfig {
        logger: Some(logger),
        ..Default::default()
    };
    let service = GaiseClientService::new(config);
    service.add_client("mock", Arc::new(MockClient)).await;
    // Test Instruct
    let req = GaiseInstructRequest {
        model: "mock::model".to_string(),
        correlation_id: Some("cid1".to_string()),
        input: OneOrMany::One(GaiseMessage::default()),
        ..Default::default()
    };
    service.instruct(&req).await.unwrap();
    {
        let l = logs.lock().unwrap();
        assert!(l.contains(&"REQ: cid=Some(\"cid1\"), type=instruct, model=mock::model".to_string()));
        assert!(l.contains(&"RES: cid=Some(\"cid1\"), type=instruct, model=mock::model".to_string()));
    }
    // Test Stream
    let mut stream = service.instruct_stream(&req).await.unwrap();
    while let Some(_) = stream.next().await {}
    {
        let l = logs.lock().unwrap();
        assert!(l.contains(&"REQ: cid=Some(\"cid1\"), type=instruct_stream, model=mock::model".to_string()));
        assert!(l.contains(&"CHUNK: cid=Some(\"cid1\"), type=instruct_stream, model=mock::model".to_string()));
    }
    // Test Embeddings
    let emb_req = GaiseEmbeddingsRequest {
        model: "mock::model".to_string(),
        correlation_id: Some("cid2".to_string()),
        input: OneOrMany::One("test".to_string()),
    };
    service.embeddings(&emb_req).await.unwrap();
    {
        let l = logs.lock().unwrap();
        assert!(l.contains(&"REQ: cid=Some(\"cid2\"), type=embeddings, model=mock::model".to_string()));
        assert!(l.contains(&"RES: cid=Some(\"cid2\"), type=embeddings, model=mock::model".to_string()));
    }
}
