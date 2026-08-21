use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        IntoResponse,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use futures_util::StreamExt;
use gaise_client::GaiseClientService;
use gaise_core::{
    GaiseClient,
    contracts::{
        GaiseEmbeddingsRequest, GaiseInstructRequest, GaiseListModelsRequest, GaiseOperation,
    },
    registry,
};
use std::sync::Arc;
use tracing::error;

#[cfg(feature = "live")]
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
#[cfg(feature = "live")]
use gaise_core::{
    GaiseLiveClient,
    contracts::{GaiseLiveConfig, GaiseLiveEvent, GaiseLiveInput},
};
#[cfg(feature = "elevenlabs")]
use gaise_core::{
    GaiseSpeechClient,
    contracts::{GaiseSpeechChunk, GaiseSpeechRequest},
};

pub struct AppState {
    pub client_service: GaiseClientService,
}

pub fn create_app(state: Arc<AppState>) -> Router {
    let router = Router::new()
        .route("/v1/instruct", post(handle_instruct))
        .route("/v1/instruct/stream", post(handle_instruct_stream))
        .route("/v1/embeddings", post(handle_embeddings))
        .route("/v1/models", get(handle_list_models))
        .route("/v1/models/limits", get(handle_model_limits))
        .route("/v1/models/:model", get(handle_get_model));

    #[cfg(feature = "live")]
    let router = router.route("/v1/live", axum::routing::get(handle_live_ws));

    #[cfg(feature = "elevenlabs")]
    let router = router
        .route("/v1/speech", post(handle_speech))
        .route("/v1/speech/stream", post(handle_speech_stream))
        .route("/v1/speech/audio", post(handle_speech_audio));

    router.with_state(state)
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
            let sse_stream = stream.map(|item| match item {
                Ok(chunk) => Event::default().json_data(chunk),
                Err(e) => Ok(Event::default().event("error").data(e.to_string())),
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

/// Query parameters for `GET /v1/models`.
#[derive(Debug, Default, serde::Deserialize)]
pub struct ListModelsQuery {
    /// Restrict to one provider key.
    pub provider: Option<String>,
    /// Keep only models supporting this operation
    /// (`instruct`, `instruct_stream`, `embeddings`, `live`).
    pub operation: Option<String>,
    /// Fetch per-model detail where it costs extra requests (Ollama).
    #[serde(default)]
    pub include_details: bool,
    /// Attach each provider's native record.
    #[serde(default)]
    pub include_raw: bool,
    pub correlation_id: Option<String>,
}

impl ListModelsQuery {
    fn into_request(self) -> Result<GaiseListModelsRequest, String> {
        let operation = match self.operation.as_deref() {
            None | Some("") => None,
            Some(value) => Some(
                GaiseOperation::parse(value)
                    .ok_or_else(|| format!("unknown operation '{value}'"))?,
            ),
        };
        Ok(GaiseListModelsRequest {
            provider: self.provider.filter(|p| !p.is_empty()),
            operation,
            include_details: self.include_details,
            include_raw: self.include_raw,
            correlation_id: self.correlation_id,
            connection: None,
        })
    }
}

async fn handle_list_models(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListModelsQuery>,
) -> impl IntoResponse {
    let request = match query.into_request() {
        Ok(request) => request,
        Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
    };
    match state.client_service.list_models(&request).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => {
            error!("List models error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

/// Query parameters for `GET /v1/models/limits`.
#[derive(Debug, Default, serde::Deserialize)]
pub struct ModelLimitsQuery {
    /// Restrict to one provider key.
    pub provider: Option<String>,
    /// Keep only entries mapped to this operation.
    pub operation: Option<String>,
}

/// `GET /v1/models/limits` — the registry's model × limits matrix
/// (`context_window`, `max_input_tokens`, `max_output_tokens`,
/// `embedding_dimensions`), served without contacting any provider or
/// needing credentials. `GET /v1/models` carries the same fields per model,
/// with provider-reported values taking precedence over the registry.
async fn handle_model_limits(Query(query): Query<ModelLimitsQuery>) -> impl IntoResponse {
    let operation = match query.operation.as_deref() {
        None | Some("") => None,
        Some(value) => match GaiseOperation::parse(value) {
            Some(op) => Some(op),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    format!("unknown operation '{value}'"),
                )
                    .into_response();
            }
        },
    };
    let provider = query.provider.filter(|p| !p.is_empty());
    let mut matrix = registry::limits_matrix(provider.as_deref());
    if let Some(operation) = operation {
        matrix.models.retain(|m| m.operations.contains(&operation));
    }
    Json(matrix).into_response()
}

/// `GET /v1/models/{provider}::{id}` — one model from one provider's listing.
async fn handle_get_model(
    State(state): State<Arc<AppState>>,
    Path(model): Path<String>,
    Query(query): Query<ListModelsQuery>,
) -> impl IntoResponse {
    let Some((provider, _)) = model.split_once("::") else {
        return (
            StatusCode::BAD_REQUEST,
            "model must be in the format 'provider::model'".to_string(),
        )
            .into_response();
    };
    let request = GaiseListModelsRequest {
        provider: Some(provider.to_string()),
        operation: None,
        include_details: query.include_details,
        include_raw: query.include_raw,
        correlation_id: query.correlation_id,
        connection: None,
    };
    match state.client_service.list_models(&request).await {
        Ok(response) => match response.models.into_iter().find(|m| m.id == model) {
            Some(found) => Json(found).into_response(),
            None => (
                StatusCode::NOT_FOUND,
                format!("model '{model}' not listed by {provider}"),
            )
                .into_response(),
        },
        Err(e) => {
            error!("Get model error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

// ── Speech endpoints ───────────────────────────────────────────────

/// `POST /v1/speech` — full clip as JSON (`audio` is a byte array).
#[cfg(feature = "elevenlabs")]
async fn handle_speech(
    State(state): State<Arc<AppState>>,
    Json(request): Json<GaiseSpeechRequest>,
) -> impl IntoResponse {
    match state.client_service.speech(&request).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => {
            error!("Speech error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

/// `POST /v1/speech/stream` — SSE of `GaiseSpeechStreamResponse` (audio,
/// alignment, usage chunks).
#[cfg(feature = "elevenlabs")]
async fn handle_speech_stream(
    State(state): State<Arc<AppState>>,
    Json(request): Json<GaiseSpeechRequest>,
) -> impl IntoResponse {
    match state.client_service.speech_stream(&request).await {
        Ok(stream) => {
            let sse_stream = stream.map(|item| match item {
                Ok(chunk) => Event::default().json_data(chunk),
                Err(e) => Ok(Event::default().event("error").data(e.to_string())),
            });
            Sse::new(sse_stream).into_response()
        }
        Err(e) => {
            error!("Speech stream error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

/// `POST /v1/speech/audio` — raw audio bytes, chunked as the provider
/// produces them, with the MIME type in `Content-Type` and the PCM rate in
/// `X-Gaise-Sample-Rate`. Alignment and usage chunks are dropped.
#[cfg(feature = "elevenlabs")]
async fn handle_speech_audio(
    State(state): State<Arc<AppState>>,
    Json(request): Json<GaiseSpeechRequest>,
) -> impl IntoResponse {
    use axum::body::Body;
    use futures_util::StreamExt as _;

    let mut stream = match state.client_service.speech_stream(&request).await {
        Ok(stream) => stream,
        Err(e) => {
            error!("Speech audio error: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };
    // Peek the first audio chunk so the headers can carry the real format.
    let mut first: Option<(Vec<u8>, String, Option<u32>)> = None;
    while let Some(item) = stream.next().await {
        match item {
            Ok(resp) => {
                if let GaiseSpeechChunk::Audio {
                    data,
                    format,
                    sample_rate,
                } = resp.chunk
                {
                    first = Some((data, format, sample_rate));
                    break;
                }
            }
            Err(e) => {
                error!("Speech audio error: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
            }
        }
    }
    let Some((head, format, sample_rate)) = first else {
        return (
            StatusCode::BAD_GATEWAY,
            "provider returned no audio".to_string(),
        )
            .into_response();
    };
    let rest = stream.filter_map(|item| async move {
        match item {
            Ok(resp) => match resp.chunk {
                GaiseSpeechChunk::Audio { data, .. } => Some(Ok::<_, std::io::Error>(data)),
                _ => None,
            },
            Err(e) => Some(Err(std::io::Error::other(e.to_string()))),
        }
    });
    let body = Body::from_stream(futures_util::stream::once(async move { Ok(head) }).chain(rest));
    let mut response = axum::response::Response::new(body);
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_str(&format).unwrap_or(axum::http::HeaderValue::from_static(
            "application/octet-stream",
        )),
    );
    if let Some(rate) = sample_rate
        && let Ok(value) = axum::http::HeaderValue::from_str(&rate.to_string())
    {
        response.headers_mut().insert("x-gaise-sample-rate", value);
    }
    response
}

// ── Live WebSocket endpoint ─────────────────────────────────────────

#[cfg(feature = "live")]
async fn handle_live_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_live_session(socket, state))
}

#[cfg(feature = "live")]
async fn handle_live_session(socket: WebSocket, state: Arc<AppState>) {
    use futures_util::SinkExt;

    let (mut ws_sink, mut ws_source) = socket.split();

    // First message must be a config message
    let config: GaiseLiveConfig = loop {
        match ws_source.next().await {
            Some(Ok(Message::Text(text))) => match serde_json::from_str::<GaiseLiveConfig>(&text) {
                Ok(cfg) => break cfg,
                Err(e) => {
                    let err = serde_json::json!({"type": "error", "message": format!("Invalid config: {}", e)});
                    let _ = ws_sink.send(Message::Text(err.to_string())).await;
                    return;
                }
            },
            Some(Ok(Message::Close(_))) | None => return,
            _ => continue,
        }
    };

    // Connect to provider
    let session = match state.client_service.live_connect(&config).await {
        Ok(s) => s,
        Err(e) => {
            let err =
                serde_json::json!({"type": "error", "message": format!("Connect error: {}", e)});
            let _ = ws_sink.send(Message::Text(err.to_string())).await;
            return;
        }
    };

    let input_tx = session.tx;
    let mut event_rx = session.rx;

    // Forward provider events → client WebSocket
    let send_handle = tokio::spawn(async move {
        while let Some(event) = event_rx.next().await {
            let msg = match event {
                Ok(GaiseLiveEvent::Audio { data, sample_rate }) => {
                    // Send audio as binary frames
                    // Prepend 4-byte sample rate header for client to know the rate
                    let mut frame = Vec::with_capacity(4 + data.len());
                    frame.extend_from_slice(&sample_rate.to_le_bytes());
                    frame.extend_from_slice(&data);
                    Message::Binary(frame)
                }
                Ok(event) => match serde_json::to_string(&event) {
                    Ok(json) => Message::Text(json),
                    Err(_) => continue,
                },
                Err(e) => {
                    let err = serde_json::json!({"type": "error", "message": e.to_string()});
                    Message::Text(err.to_string())
                }
            };
            if ws_sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Forward client WebSocket → provider input channel
    while let Some(msg) = ws_source.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(_) => break,
        };

        let input = match msg {
            Message::Text(text) => match serde_json::from_str::<GaiseLiveInput>(&text) {
                Ok(input) => input,
                Err(_) => continue,
            },
            Message::Binary(data) => {
                // Binary frames are raw PCM audio (16kHz PCM16 by default)
                GaiseLiveInput::Audio {
                    data: data.to_vec(),
                    sample_rate: 16000,
                }
            }
            Message::Close(_) => {
                let _ = input_tx.send(GaiseLiveInput::Close).await;
                break;
            }
            _ => continue,
        };

        if input_tx.send(input).await.is_err() {
            break;
        }
    }

    // Cleanup
    let _ = input_tx.send(GaiseLiveInput::Close).await;
    send_handle.abort();
}
