use serde_json::Value;
use std::fmt::Debug;
pub trait IGaiseLogger: Send + Sync + Debug {
    fn log_request(
        &self,
        correlation_id: Option<&str>,
        request_type: &str,
        model: &str,
        request_json: Value,
    );
    fn log_response(
        &self,
        correlation_id: Option<&str>,
        request_type: &str,
        model: &str,
        response_json: Value,
        usage: Option<Value>,
    );
    fn log_stream_chunk(
        &self,
        correlation_id: Option<&str>,
        request_type: &str,
        model: &str,
        chunk_json: Value,
    );
}
#[derive(Debug, Default)]
pub struct ConsoleGaiseLogger;
impl IGaiseLogger for ConsoleGaiseLogger {
    fn log_request(
        &self,
        correlation_id: Option<&str>,
        request_type: &str,
        model: &str,
        request_json: Value,
    ) {
        let cid = correlation_id.unwrap_or("none");
        println!(
            "[GAISE REQUEST] CID: {} | Type: {} | Model: {} | Request: {}",
            cid, request_type, model, request_json
        );
    }
    fn log_response(
        &self,
        correlation_id: Option<&str>,
        request_type: &str,
        model: &str,
        response_json: Value,
        usage: Option<Value>,
    ) {
        let cid = correlation_id.unwrap_or("none");
        let usage_str = usage.map(|u| u.to_string()).unwrap_or_else(|| "none".to_string());
        println!(
            "[GAISE RESPONSE] CID: {} | Type: {} | Model: {} | Response: {} | Usage: {}",
            cid, request_type, model, response_json, usage_str
        );
    }
    fn log_stream_chunk(
        &self,
        correlation_id: Option<&str>,
        request_type: &str,
        model: &str,
        chunk_json: Value,
    ) {
        let cid = correlation_id.unwrap_or("none");
        println!(
            "[GAISE STREAM] CID: {} | Type: {} | Model: {} | Chunk: {}",
            cid, request_type, model, chunk_json
        );
    }
}
