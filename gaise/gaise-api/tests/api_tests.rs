use axum::{
    body::Body,
    http::{self, Request, StatusCode},
};
use gaise_api::{create_app, AppState};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt; // for `oneshot`

#[tokio::test]
async fn test_instruct_post_request() {
    let state = Arc::new(AppState {
        ollama_url: "http://localhost:11434".to_string(),
        vertexai_api_url: "".to_string(),
        vertexai_sa_path: None,
        openai_api_url: "https://api.openai.com/v1".to_string(),
        openai_api_key: "".to_string(),
        clients: RwLock::new(HashMap::new()),
    });

    let app = create_app(state);

    // This is how you simulate a POST request in Axum without starting a real server.
    // We use `oneshot` to send a single request and get the response.
    let response = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/v1/instruct")
                .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "model": "nonexistent::model",
                        "input": [
                            {
                                "role": "user",
                                "content": {
                                    "type": "text",
                                    "text": "Hello"
                                }
                            }
                        ]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Since we provided an unknown provider ("nonexistent"), we expect a BAD_REQUEST (400)
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("Unknown provider: nonexistent"));
}
