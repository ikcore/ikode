use gaise_api::{AppState, create_app};
use gaise_client::{GaiseClientConfig, GaiseClientService};
use gaise_core::logging::ConsoleGaiseLogger;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let ollama_url = std::env::var("OLLAMA_URL").ok();
    let vertexai_api_url = std::env::var("VERTEXAI_API_URL").ok();
    let vertexai_sa_path = std::env::var("VERTEXAI_SA_PATH").ok();
    let openai_api_url = std::env::var("OPENAI_API_URL").ok();
    let openai_api_key = std::env::var("OPENAI_API_KEY").ok();
    let bedrock_region = std::env::var("BEDROCK_REGION").ok();
    let anthropic_api_url = std::env::var("ANTHROPIC_API_URL").ok();
    let anthropic_api_key = std::env::var("ANTHROPIC_API_KEY").ok();
    let gemini_api_url = std::env::var("GEMINI_API_URL").ok();
    let gemini_api_key = std::env::var("GEMINI_API_KEY").ok();
    #[cfg(feature = "elevenlabs")]
    let elevenlabs_api_url = std::env::var("ELEVENLABS_API_URL").ok();
    #[cfg(feature = "elevenlabs")]
    let elevenlabs_api_key = std::env::var("ELEVENLABS_API_KEY").ok();

    let vertexai_sa = vertexai_sa_path.and_then(|path| {
        let sa_json = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&sa_json).ok()
    });

    let config = GaiseClientConfig {
        ollama_url,
        vertexai_api_url,
        vertexai_sa,
        openai_api_url,
        openai_api_key,
        bedrock_region,
        anthropic_api_url,
        anthropic_api_key,
        gemini_api_url,
        gemini_api_key,
        #[cfg(feature = "elevenlabs")]
        elevenlabs_api_url,
        #[cfg(feature = "elevenlabs")]
        elevenlabs_api_key,
        logger: Some(Arc::new(ConsoleGaiseLogger)),
    };

    let state = Arc::new(AppState {
        client_service: GaiseClientService::new(config),
    });

    let app = create_app(state);

    let port = std::env::var("GAISE_PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    info!("Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}
