use std::sync::Arc;
use axum::{
    extract::State,
    http::StatusCode,
    response::{sse::{Event, Sse}, IntoResponse},
    routing::post,
    Json, Router,
};
use futures_util::{StreamExt};
use gaise_core::{
    contracts::{GaiseEmbeddingsRequest, GaiseInstructRequest},
    GaiseClient,
};
use gaise_client::{GaiseClientService};
use tracing::error;

pub struct AppState {
    pub client_service: GaiseClientService,
}

pub fn create_app(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/instruct", post(handle_instruct))
        .route("/v1/instruct/stream", post(handle_instruct_stream))
        .route("/v1/embeddings", post(handle_embeddings))
        .with_state(state)
}

async fn handle_instruct(
    State(state): State<Arc<AppState>>,
    Json(request): Json<GaiseInstructRequest>,
) -> impl IntoResponse {
    match state.client_service.instruct(&request).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => {
            error!("Instruct error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

async fn handle_instruct_stream(
    State(state): State<Arc<AppState>>,
    Json(request): Json<GaiseInstructRequest>,
) -> impl IntoResponse {
    match state.client_service.instruct_stream(&request).await {
        Ok(stream) => {
            let sse_stream = stream.map(|item| {
                match item {
                    Ok(chunk) => {
                        Event::default().json_data(chunk)
                    }
                    Err(e) => {
                        Ok(Event::default().event("error").data(e.to_string()))
                    }
                }
            });
            Sse::new(sse_stream).into_response()
        }
        Err(e) => {
            error!("Instruct stream error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

async fn handle_embeddings(
    State(state): State<Arc<AppState>>,
    Json(request): Json<GaiseEmbeddingsRequest>,
) -> impl IntoResponse {
    match state.client_service.embeddings(&request).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => {
            error!("Embeddings error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}
