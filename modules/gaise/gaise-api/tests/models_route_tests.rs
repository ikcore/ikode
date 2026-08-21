//! `GET /v1/models` routes, exercised with an in-process fake provider.

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
use gaise_core::GaiseClient;
use gaise_core::contracts::{
    GaiseEmbeddingsRequest, GaiseEmbeddingsResponse, GaiseInstructRequest, GaiseInstructResponse,
    GaiseInstructStreamResponse, GaiseListModelsRequest, GaiseListModelsResponse, GaiseModality,
    GaiseModel, GaiseOperation,
};
use tower::ServiceExt;

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

struct FakeClient;

#[async_trait]
impl GaiseClient for FakeClient {
    async fn instruct_stream(
        &self,
        _: &GaiseInstructRequest,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<GaiseInstructStreamResponse, BoxErr>> + Send>>,
        BoxErr,
    > {
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
        let mut chat = GaiseModel::new("openai", "gpt-5.6");
        chat.capabilities.add_input(GaiseModality::Text);
        chat.capabilities.add_operation(GaiseOperation::Instruct);
        let mut embed = GaiseModel::new("openai", "text-embedding-3-small");
        embed.capabilities.add_operation(GaiseOperation::Embeddings);
        let mut response = GaiseListModelsResponse::from_models(vec![chat, embed]);
        response.retain_operation(request.operation);
        Ok(response)
    }
}

async fn app() -> axum::Router {
    let service = GaiseClientService::new(GaiseClientConfig::default());
    service.add_client("openai", Arc::new(FakeClient)).await;
    create_app(Arc::new(AppState {
        client_service: service,
    }))
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn lists_models_with_routable_ids_and_filters() {
    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    let ids: Vec<&str> = json["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec!["openai::gpt-5.6", "openai::text-embedding-3-small"]
    );
    assert!(
        json.get("errors").is_none(),
        "no errors key when nothing failed"
    );
    // Registry overlay ran: gpt-5.6 gains image input and reasoning values.
    let chat = &json["models"][0];
    assert!(
        chat["capabilities"]["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "image")
    );
    assert!(chat["capabilities"]["reasoning_values"].is_array());

    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/models?provider=openai&operation=embeddings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["models"].as_array().unwrap().len(), 1);
    assert_eq!(json["models"][0]["id"], "openai::text-embedding-3-small");
}

#[tokio::test]
async fn get_model_and_validation_errors() {
    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/models/openai::gpt-5.6")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["id"], "openai::gpt-5.6");

    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/models/openai::nope")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/models/no-separator")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/models?operation=teleport")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/models?provider=missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn limits_matrix_is_served_from_the_registry() {
    // No fake provider takes part: the matrix needs no credentials.
    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/models/limits")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["source"], "registry");
    assert!(body["audited_on"].as_str().is_some_and(|d| d.len() == 10));
    let models = body["models"].as_array().unwrap();
    assert!(models.len() > 100, "every registry entry is a row");
    let opus = models
        .iter()
        .find(|m| m["id"] == "anthropic::claude-opus-5")
        .expect("opus row");
    assert_eq!(opus["provider"], "anthropic");
    assert_eq!(opus["context_window"], 1_000_000);
    assert_eq!(opus["max_output_tokens"], 128_000);
    assert!(opus.get("limits").is_none(), "limits are flattened");
    let embed = models
        .iter()
        .find(|m| m["id"] == "openai::text-embedding-3-small")
        .expect("embedding row");
    assert_eq!(embed["max_input_tokens"], 8192);
    assert_eq!(embed["embedding_dimensions"], 1536);
    assert!(embed.get("context_window").is_none());

    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/models/limits?provider=ollama&operation=embeddings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let models = body["models"].as_array().unwrap();
    assert!(!models.is_empty());
    assert!(models.iter().all(|m| m["provider"] == "ollama"));
    assert!(models.iter().all(|m| {
        m["operations"]
            .as_array()
            .unwrap()
            .contains(&"embeddings".into())
    }));

    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/models/limits?operation=teleport")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
