//! Per-request `connection` overrides: resolution order, caching, and log
//! redaction. Nothing here contacts a provider.

use std::sync::{Arc, Mutex};

use gaise_client::{GaiseClientConfig, GaiseClientService};
use gaise_core::GaiseClient;
use gaise_core::contracts::{
    GaiseConnection, GaiseContent, GaiseInstructRequest, GaiseMessage, OneOrMany,
};
use gaise_core::logging::IGaiseLogger;
use serde_json::Value;

#[derive(Debug, Default)]
struct CaptureLogger {
    requests: Arc<Mutex<Vec<Value>>>,
}

impl IGaiseLogger for CaptureLogger {
    fn log_request(&self, _: Option<&str>, _: &str, _: &str, json: Value) {
        self.requests.lock().unwrap().push(json);
    }
    fn log_response(&self, _: Option<&str>, _: &str, _: &str, _: Value, _: Option<Value>) {}
    fn log_stream_chunk(&self, _: Option<&str>, _: &str, _: &str, _: Value) {}
}

fn request(model: &str, connection: Option<GaiseConnection>) -> GaiseInstructRequest {
    GaiseInstructRequest {
        model: model.into(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".into(),
            content: Some(OneOrMany::One(GaiseContent::Text { text: "hi".into() })),
            ..Default::default()
        }),
        connection,
        ..Default::default()
    }
}

#[tokio::test]
async fn override_supplies_missing_credentials_and_is_cached_per_connection() {
    // No OpenAI key configured: plain routing fails, an override succeeds in
    // building the client (no HTTP is made by get_client_with).
    let service = GaiseClientService::new(GaiseClientConfig::default());
    let err = service.get_client("openai").await.err().unwrap();
    assert!(err.to_string().contains("OpenAI API Key not configured"));

    let conn_a = GaiseConnection {
        api_key: Some("sk-a".into()),
        api_url: Some("http://127.0.0.1:1/v1".into()),
        ..Default::default()
    };
    let conn_b = GaiseConnection {
        api_key: Some("sk-b".into()),
        ..conn_a.clone()
    };
    let a1 = service
        .get_client_with("openai", Some(&conn_a))
        .await
        .unwrap();
    let a2 = service
        .get_client_with("openai", Some(&conn_a))
        .await
        .unwrap();
    let b = service
        .get_client_with("openai", Some(&conn_b))
        .await
        .unwrap();
    assert!(
        Arc::ptr_eq(&a1, &a2),
        "same connection reuses the cached client"
    );
    assert!(
        !Arc::ptr_eq(&a1, &b),
        "different credentials get a distinct client"
    );

    // An empty override behaves like no override.
    let err = service
        .get_client_with("openai", Some(&GaiseConnection::default()))
        .await
        .err()
        .expect("empty override is no override");
    assert!(err.to_string().contains("not configured"));

    // Region override for Bedrock and service-account override for Vertex are
    // validated before any network activity.
    let bad_sa = GaiseConnection {
        api_url: Some(
            "https://x/v1/projects/p/locations/l/publishers/google/models/{{MODEL}}".into(),
        ),
        service_account: Some(serde_json::json!({"client_email": "only-email"})),
        ..Default::default()
    };
    let err = service
        .get_client_with("vertexai", Some(&bad_sa))
        .await
        .err()
        .expect("bad service account");
    assert!(
        err.to_string()
            .contains("invalid VertexAI service account override"),
        "{err}"
    );
}

#[tokio::test]
async fn configured_credentials_still_win_when_no_override_is_given() {
    let service = GaiseClientService::new(GaiseClientConfig {
        anthropic_api_key: Some("configured".into()),
        ..Default::default()
    });
    let configured = service.get_client("anthropic").await.unwrap();
    let overridden = service
        .get_client_with(
            "anthropic",
            Some(&GaiseConnection {
                api_key: Some("per-request".into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    assert!(!Arc::ptr_eq(&configured, &overridden));
    let again = service.get_client("anthropic").await.unwrap();
    assert!(
        Arc::ptr_eq(&configured, &again),
        "the configured client is untouched by overrides"
    );
}

#[tokio::test]
async fn logged_requests_never_contain_override_secrets() {
    let logger = Arc::new(CaptureLogger::default());
    let service = GaiseClientService::new(GaiseClientConfig {
        logger: Some(logger.clone()),
        ..Default::default()
    });
    let connection = GaiseConnection {
        api_url: Some("http://127.0.0.1:1/v1".into()),
        api_key: Some("sk-top-secret".into()),
        service_account: Some(serde_json::json!({"client_email": "a@b", "private_key": "PRIVATE"})),
        ..Default::default()
    };
    // gpt-5.5-pro is rejected by the adapter before any network call, but the
    // router has already logged the request by then.
    let err = service
        .instruct(&request("openai::gpt-5.5-pro", Some(connection)))
        .await
        .expect_err("Responses-only model");
    assert!(err.to_string().contains("Responses API"));
    let logged = logger.requests.lock().unwrap();
    assert_eq!(logged.len(), 1);
    let text = logged[0].to_string();
    assert!(!text.contains("sk-top-secret"), "{text}");
    assert!(!text.contains("PRIVATE"), "{text}");
    assert_eq!(logged[0]["connection"]["api_key"], "***");
    assert_eq!(logged[0]["connection"]["api_url"], "http://127.0.0.1:1/v1");
}
