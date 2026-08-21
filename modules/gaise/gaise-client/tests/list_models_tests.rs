//! Router behaviour for model listing, exercised with in-process fake clients
//! so no provider traffic is generated.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::Stream;
use gaise_client::{GaiseClientConfig, GaiseClientService};
use gaise_core::GaiseClient;
use gaise_core::contracts::{
    GaiseEmbeddingsRequest, GaiseEmbeddingsResponse, GaiseInstructRequest, GaiseInstructResponse,
    GaiseInstructStreamResponse, GaiseListModelsRequest, GaiseListModelsResponse,
    GaiseMetadataSource, GaiseModality, GaiseModel, GaiseModelStatus, GaiseOperation, GaiseSupport,
};

type BoxErr = Box<dyn std::error::Error + Send + Sync>;
type StreamResult =
    Result<Pin<Box<dyn Stream<Item = Result<GaiseInstructStreamResponse, BoxErr>> + Send>>, BoxErr>;

/// A fake provider that returns a canned list, or fails.
struct FakeClient {
    models: Vec<GaiseModel>,
    fail: bool,
}

#[async_trait]
impl GaiseClient for FakeClient {
    async fn instruct_stream(&self, _: &GaiseInstructRequest) -> StreamResult {
        Err("not used".into())
    }
    async fn instruct(&self, _: &GaiseInstructRequest) -> Result<GaiseInstructResponse, BoxErr> {
        Err("not used".into())
    }
    async fn embeddings(
        &self,
        _: &GaiseEmbeddingsRequest,
    ) -> Result<GaiseEmbeddingsResponse, BoxErr> {
        Err("not used".into())
    }
    async fn list_models(
        &self,
        request: &GaiseListModelsRequest,
    ) -> Result<GaiseListModelsResponse, BoxErr> {
        if self.fail {
            return Err("boom".into());
        }
        let mut response = GaiseListModelsResponse::from_models(self.models.clone());
        response.retain_operation(request.operation);
        Ok(response)
    }
}

/// A client that leaves the trait default in place.
struct LegacyClient;

#[async_trait]
impl GaiseClient for LegacyClient {
    async fn instruct_stream(&self, _: &GaiseInstructRequest) -> StreamResult {
        Err("not used".into())
    }
    async fn instruct(&self, _: &GaiseInstructRequest) -> Result<GaiseInstructResponse, BoxErr> {
        Err("not used".into())
    }
    async fn embeddings(
        &self,
        _: &GaiseEmbeddingsRequest,
    ) -> Result<GaiseEmbeddingsResponse, BoxErr> {
        Err("not used".into())
    }
}

fn anthropic_like(id: &str) -> GaiseModel {
    // Mimics what the Anthropic adapter returns: provider facts only.
    let mut m = GaiseModel::new("anthropic", id);
    m.capabilities.add_input(GaiseModality::Text);
    m.capabilities.add_input(GaiseModality::Image);
    m.capabilities.add_output(GaiseModality::Text);
    m.capabilities.add_operation(GaiseOperation::Instruct);
    m.capabilities.add_operation(GaiseOperation::InstructStream);
    m.capabilities.tools = GaiseSupport::Supported;
    m.capabilities.add_source(GaiseMetadataSource::Provider);
    m.status = GaiseModelStatus::Active;
    m
}

fn openai_like(id: &str) -> GaiseModel {
    // Mimics the OpenAI adapter: identity only.
    let mut m = GaiseModel::new("openai", id);
    m.capabilities.add_source(GaiseMetadataSource::Provider);
    m
}

#[tokio::test]
async fn aggregate_listing_prefixes_ids_enriches_and_reports_partial_failures() {
    let service = GaiseClientService::new(GaiseClientConfig::default());
    service
        .add_client(
            "anthropic",
            Arc::new(FakeClient {
                models: vec![anthropic_like("claude-opus-4-6")],
                fail: false,
            }),
        )
        .await;
    service
        .add_client(
            "openai",
            Arc::new(FakeClient {
                models: vec![
                    openai_like("text-embedding-3-small"),
                    openai_like("mystery-9000"),
                ],
                fail: false,
            }),
        )
        .await;
    service
        .add_client(
            "gemini",
            Arc::new(FakeClient {
                models: vec![],
                fail: true,
            }),
        )
        .await;
    service.add_client("custom", Arc::new(LegacyClient)).await;

    let providers = service.configured_providers().await;
    assert_eq!(providers, vec!["anthropic", "custom", "gemini", "openai"]);

    let response = service
        .list_models(&GaiseListModelsRequest::default())
        .await
        .unwrap();

    // Partial failure is reported, not swallowed, and does not sink the rest.
    let failed: Vec<&str> = response
        .errors
        .iter()
        .map(|e| e.provider.as_str())
        .collect();
    assert_eq!(failed, vec!["custom", "gemini"]);
    assert!(response.errors[0].message.contains("not supported"));
    assert!(response.errors[1].message.contains("boom"));

    let ids: Vec<&str> = response.models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "anthropic::claude-opus-4-6",
            "openai::text-embedding-3-small",
            "openai::mystery-9000"
        ]
    );

    // Registry overlay fills what the provider left unknown without touching provider facts.
    let opus = &response.models[0];
    assert_eq!(opus.capabilities.reasoning, GaiseSupport::Supported);
    assert!(opus.capabilities.reasoning_values.is_some());
    assert!(opus.retirement_not_before.is_some());
    assert_eq!(
        opus.capabilities.sources,
        vec![GaiseMetadataSource::Provider, GaiseMetadataSource::Registry]
    );
    assert_eq!(
        opus.capabilities.input,
        vec![
            GaiseModality::Text,
            GaiseModality::Image,
            GaiseModality::File
        ]
    );

    let embed = &response.models[1];
    assert_eq!(
        embed.capabilities.operations,
        vec![GaiseOperation::Embeddings]
    );
    assert_eq!(embed.capabilities.output, vec![GaiseModality::Embedding]);
    assert_eq!(embed.status, GaiseModelStatus::Active);

    // Unknown model: nothing invented.
    let mystery = &response.models[2];
    assert!(mystery.capabilities.operations.is_empty());
    assert!(mystery.capabilities.input.is_empty());
    assert_eq!(mystery.capabilities.tools, GaiseSupport::Unknown);
    assert_eq!(
        mystery.capabilities.sources,
        vec![GaiseMetadataSource::Provider]
    );
}

#[tokio::test]
async fn single_provider_listing_propagates_errors_and_filters_by_operation() {
    let service = GaiseClientService::new(GaiseClientConfig::default());
    service
        .add_client(
            "anthropic",
            Arc::new(FakeClient {
                models: vec![anthropic_like("claude-sonnet-4-6")],
                fail: false,
            }),
        )
        .await;
    service
        .add_client(
            "gemini",
            Arc::new(FakeClient {
                models: vec![],
                fail: true,
            }),
        )
        .await;

    let err = service
        .list_models(&GaiseListModelsRequest {
            provider: Some("gemini".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("boom"));

    let none = service
        .list_models(&GaiseListModelsRequest {
            provider: Some("anthropic".into()),
            operation: Some(GaiseOperation::Embeddings),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(none.models.is_empty());

    let some = service
        .list_models(&GaiseListModelsRequest {
            provider: Some("anthropic".into()),
            operation: Some(GaiseOperation::InstructStream),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(some.models.len(), 1);
    assert_eq!(some.models[0].id, "anthropic::claude-sonnet-4-6");

    let unknown = service
        .list_models(&GaiseListModelsRequest {
            provider: Some("nope".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(unknown.to_string().contains("Unknown or disabled provider"));
}
