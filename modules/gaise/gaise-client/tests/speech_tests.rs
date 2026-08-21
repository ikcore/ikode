#![cfg(feature = "elevenlabs")]
//! Speech routing through `GaiseClientService`, with an in-process fake so
//! no ElevenLabs traffic is generated.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{Stream, StreamExt};
use gaise_client::{GaiseClientConfig, GaiseClientService};
use gaise_core::GaiseSpeechClient;
use gaise_core::contracts::{
    GaiseSpeechChunk, GaiseSpeechRequest, GaiseSpeechResponse, GaiseSpeechStreamResponse,
};

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

struct FakeSpeech;

#[async_trait]
impl GaiseSpeechClient for FakeSpeech {
    async fn speech(&self, request: &GaiseSpeechRequest) -> Result<GaiseSpeechResponse, BoxErr> {
        assert_eq!(
            request.model, "fake-voice-model",
            "router strips the provider prefix"
        );
        Ok(GaiseSpeechResponse {
            audio: vec![1, 2, 3],
            format: "audio/pcm".into(),
            sample_rate: Some(24_000),
            external_id: Some("req-1".into()),
            alignment: None,
            usage: None,
        })
    }

    async fn speech_stream(
        &self,
        _request: &GaiseSpeechRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<GaiseSpeechStreamResponse, BoxErr>> + Send>>, BoxErr>
    {
        let chunks = vec![
            Ok(GaiseSpeechStreamResponse {
                chunk: GaiseSpeechChunk::Audio {
                    data: vec![1],
                    format: "audio/pcm".into(),
                    sample_rate: Some(24_000),
                },
                external_id: None,
            }),
            Ok(GaiseSpeechStreamResponse {
                chunk: GaiseSpeechChunk::Audio {
                    data: vec![2],
                    format: "audio/pcm".into(),
                    sample_rate: Some(24_000),
                },
                external_id: None,
            }),
        ];
        Ok(Box::pin(futures_util::stream::iter(chunks)))
    }
}

#[tokio::test]
async fn routes_speech_by_provider_prefix() {
    let service = GaiseClientService::new(GaiseClientConfig::default());
    service
        .add_speech_client("fakevoice", Arc::new(FakeSpeech))
        .await;

    let response = service
        .speech(&GaiseSpeechRequest {
            model: "fakevoice::fake-voice-model".into(),
            input: "hello".into(),
            voice: Some("v1".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(response.audio, vec![1, 2, 3]);
    assert_eq!(response.format, "audio/pcm");

    let stream = service
        .speech_stream(&GaiseSpeechRequest {
            model: "fakevoice::fake-voice-model".into(),
            input: "hello".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let chunks: Vec<_> = stream.collect().await;
    assert_eq!(chunks.len(), 2);

    // Unknown provider and missing prefix are routing errors.
    let err = service
        .speech(&GaiseSpeechRequest {
            model: "nope::x".into(),
            input: "hi".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("No speech provider"));
    let err = service
        .speech(&GaiseSpeechRequest {
            model: "no-prefix".into(),
            input: "hi".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("provider::model"));

    // ElevenLabs needs a key before it is built; the provider is recognised.
    let err = service
        .speech(&GaiseSpeechRequest {
            model: "elevenlabs::eleven_v3".into(),
            input: "hi".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("ElevenLabs API Key not configured")
    );
    assert!(
        !service
            .configured_providers()
            .await
            .contains(&"elevenlabs".to_string())
    );
}
