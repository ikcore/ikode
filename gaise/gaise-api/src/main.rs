use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::info;
use gaise_api::{create_app, AppState};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let ollama_url = std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let vertexai_api_url = std::env::var("VERTEXAI_API_URL").unwrap_or_else(|_| "".to_string());
    let vertexai_sa_path = std::env::var("VERTEXAI_SA_PATH").ok();
    let openai_api_url = std::env::var("OPENAI_API_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let openai_api_key = std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| "".to_string());

    let state = Arc::new(AppState {
        ollama_url,
        vertexai_api_url,
        vertexai_sa_path,
        openai_api_url,
        openai_api_key,
        clients: RwLock::new(HashMap::new()),
    });

    let app = create_app(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    info!("Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}
