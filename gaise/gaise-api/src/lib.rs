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
use gaise_provider_ollama::ollama_client::GaiseClientOllama;
use gaise_provider_vertexai::vertexai_client::GaiseClientVertexAI;
use gaise_provider_openai::openai_client::GaiseClientOpenAI;
use gaise_provider_vertexai::contracts::ServiceAccount;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::error;

pub struct AppState {
    pub ollama_url: String,
    pub vertexai_api_url: String,
    pub vertexai_sa_path: Option<String>,
    pub openai_api_url: String,
    pub openai_api_key: String,
    pub clients: RwLock<HashMap<String, Arc<dyn GaiseClient>>>,
}

pub fn create_app(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/instruct", post(handle_instruct))
        .route("/v1/instruct/stream", post(handle_instruct_stream))
        .route("/v1/embeddings", post(handle_embeddings))
        .with_state(state)
}

async fn get_client(
    state: &AppState,
    model: &str,
) -> Result<(Arc<dyn GaiseClient>, String), (StatusCode, String)> {
    let parts: Vec<&str> = model.splitn(2, "::").collect();
    if parts.len() < 2 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Model name must be in the format 'provider::model'".to_string(),
        ));
    }

    let provider = parts[0];
    let actual_model = parts[1].to_string();

    {
        let clients = state.clients.read().await;
        if let Some(client) = clients.get(provider) {
            return Ok((client.clone(), actual_model));
        }
    }

    let client: Arc<dyn GaiseClient> = match provider {
        "ollama" => Arc::new(GaiseClientOllama::new(state.ollama_url.clone())),
        "vertexai" => {
            let sa_path = state.vertexai_sa_path.as_ref().ok_or_else(|| {
                (StatusCode::INTERNAL_SERVER_ERROR, "VertexAI Service Account path not configured".to_string())
            })?;
            let sa_json = std::fs::read_to_string(sa_path).map_err(|e| {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to read Service Account file: {}", e))
            })?;
            let sa: ServiceAccount = serde_json::from_str(&sa_json).map_err(|e| {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to parse Service Account file: {}", e))
            })?;
            Arc::new(GaiseClientVertexAI::new(&sa, state.vertexai_api_url.clone()).await)
        }
        "openai" => {
            Arc::new(GaiseClientOpenAI::new(state.openai_api_url.clone(), state.openai_api_key.clone()))
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Unknown provider: {}", provider),
            ))
        }
    };

    let mut clients = state.clients.write().await;
    clients.insert(provider.to_string(), client.clone());
    Ok((client, actual_model))
}

async fn handle_instruct(
    State(state): State<Arc<AppState>>,
    Json(mut request): Json<GaiseInstructRequest>,
) -> impl IntoResponse {
    let (client, actual_model) = match get_client(&state, &request.model).await {
        Ok(res) => res,
        Err(e) => return e.into_response(),
    };
    request.model = actual_model;

    match client.instruct(&request).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => {
            error!("Instruct error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

async fn handle_instruct_stream(
    State(state): State<Arc<AppState>>,
    Json(mut request): Json<GaiseInstructRequest>,
) -> impl IntoResponse {
    let (client, actual_model) = match get_client(&state, &request.model).await {
        Ok(res) => res,
        Err(e) => return e.into_response(),
    };
    request.model = actual_model;

    match client.instruct_stream(&request).await {
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
    Json(mut request): Json<GaiseEmbeddingsRequest>,
) -> impl IntoResponse {
    let (client, actual_model) = match get_client(&state, &request.model).await {
        Ok(res) => res,
        Err(e) => return e.into_response(),
    };
    request.model = actual_model;

    match client.embeddings(&request).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => {
            error!("Embeddings error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}
