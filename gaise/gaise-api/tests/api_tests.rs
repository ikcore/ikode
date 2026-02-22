use axum::{
    body::Body,
    http::{self, Request, StatusCode},
};
use gaise_api::{create_app, AppState};
use gaise_client::{GaiseClientService, GaiseClientConfig};
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt; // for `oneshot`

#[tokio::test]
#[ignore]
async fn test_instruct_post_request() {
    let config = GaiseClientConfig {
        ollama_url: Some("http://localhost:11434".to_string()),
        ..Default::default()
    };
    let state = Arc::new(AppState {
        client_service: GaiseClientService::new(config),
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

    // Since we provided an unknown provider ("nonexistent"), we expect an INTERNAL_SERVER_ERROR (500)
    // because GaiseClientService returns an error which the handler maps to 500.
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("Unknown provider: nonexistent"));
}
