#![cfg(feature = "elevenlabs")]
//! `/v1/speech*` routes, exercised with an in-process fake speech provider.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::Stream;
use gaise_api::{AppState, create_app};
use gaise_client::{GaiseClientConfig, GaiseClientService};
use gaise_core::GaiseSpeechClient;
use gaise_core::contracts::{
    GaiseSpeechAlignment, GaiseSpeechChunk, GaiseSpeechRequest, GaiseSpeechResponse,
    GaiseSpeechStreamResponse,
};
use tower::ServiceExt;

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

struct FakeSpeech;

#[async_trait]
impl GaiseSpeechClient for FakeSpeech {
    async fn speech(&self, _: &GaiseSpeechRequest) -> Result<GaiseSpeechResponse, BoxErr> {
        Ok(GaiseSpeechResponse {
            audio: vec![9, 8, 7],
            format: "audio/mpeg".into(),
            sample_rate: Some(44_100),
            external_id: Some("req-9".into()),
            alignment: None,
            usage: None,
        })
    }

    async fn speech_stream(
        &self,
        _: &GaiseSpeechRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<GaiseSpeechStreamResponse, BoxErr>> + Send>>, BoxErr>
    {
        let items = vec![
            Ok(GaiseSpeechStreamResponse {
                chunk: GaiseSpeechChunk::Alignment(GaiseSpeechAlignment {
                    characters: vec!["h".into()],
                    start_seconds: vec![0.0],
                    end_seconds: vec![0.1],
                }),
                external_id: None,
            }),
            Ok(GaiseSpeechStreamResponse {
                chunk: GaiseSpeechChunk::Audio {
                    data: vec![1, 2],
                    format: "audio/pcm".into(),
                    sample_rate: Some(16_000),
                },
                external_id: None,
            }),
            Ok(GaiseSpeechStreamResponse {
                chunk: GaiseSpeechChunk::Audio {
                    data: vec![3],
                    format: "audio/pcm".into(),
                    sample_rate: Some(16_000),
                },
                external_id: None,
            }),
            Ok(GaiseSpeechStreamResponse {
                chunk: GaiseSpeechChunk::Usage(Default::default()),
                external_id: None,
            }),
        ];
        Ok(Box::pin(futures_util::stream::iter(items)))
    }
}

async fn app() -> axum::Router {
    let service = GaiseClientService::new(GaiseClientConfig::default());
    service
        .add_speech_client("fakevoice", Arc::new(FakeSpeech))
        .await;
    create_app(Arc::new(AppState {
        client_service: service,
    }))
}

fn post(path: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"model":"fakevoice::m","input":"hello","voice":"v"}"#,
        ))
        .unwrap()
}

#[tokio::test]
async fn speech_json_route_returns_bytes_and_format() {
    let response = app().await.oneshot(post("/v1/speech")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["audio"], serde_json::json!([9, 8, 7]));
    assert_eq!(json["format"], "audio/mpeg");
    assert_eq!(json["sample_rate"], 44_100);
}

#[tokio::test]
async fn speech_audio_route_streams_raw_bytes_with_headers() {
    let response = app().await.oneshot(post("/v1/speech/audio")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "audio/pcm");
    assert_eq!(response.headers()["x-gaise-sample-rate"], "16000");
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(
        bytes.to_vec(),
        vec![1, 2, 3],
        "alignment and usage chunks are dropped"
    );
}

#[tokio::test]
async fn speech_stream_route_is_sse() {
    let response = app()
        .await
        .oneshot(post("/v1/speech/stream"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.contains(r#""alignment""#));
    assert!(text.contains(r#""audio""#));
    assert!(text.contains(r#""usage""#));

    let response = app()
        .await
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/speech")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"model":"missing::m","input":"hello"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}
