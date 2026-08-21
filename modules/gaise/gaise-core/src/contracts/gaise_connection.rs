//! Per-request provider connection overrides.
//!
//! `GaiseClientService` is normally configured once (environment variables
//! or `GaiseClientConfig`). A request can carry a [`GaiseConnection`] to
//! override the endpoint and credentials for that call only — multi-tenant
//! gateways, per-user keys, regional endpoints, or tests against a stub
//! server. Overridden clients are cached per distinct connection so repeated
//! calls reuse HTTP pools and (for Vertex AI / Bedrock) cached tokens.
//!
//! Secrets never reach logs: the router serializes requests through
//! [`redact_secrets`] before handing them to a logger.

use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GaiseConnection {
    /// Base URL (OpenAI/Anthropic/Gemini/Ollama/ElevenLabs) or the Vertex AI
    /// `{{MODEL}}` URL template.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
    /// API key for key-authenticated providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// AWS region for Bedrock (credentials still come from the AWS chain).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Vertex AI service-account JSON (`client_email`, `private_key`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_account: Option<serde_json::Value>,
}

impl GaiseConnection {
    pub fn is_empty(&self) -> bool {
        self.api_url.is_none()
            && self.api_key.is_none()
            && self.region.is_none()
            && self.service_account.is_none()
    }

    /// Stable cache discriminator for this override set (hash of all fields),
    /// so the router can keep one client per distinct connection without
    /// storing the secret itself in the key.
    pub fn cache_key(&self) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.api_url.hash(&mut hasher);
        self.api_key.hash(&mut hasher);
        self.region.hash(&mut hasher);
        self.service_account
            .as_ref()
            .map(|v| v.to_string())
            .hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

/// Mask credentials inside a serialized request (any `connection` object's
/// `api_key` and `service_account.private_key`). Used by the router before
/// logging; safe to call on any JSON value.
pub fn redact_secrets(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(conn) = map.get_mut("connection")
                && let Some(conn) = conn.as_object_mut()
            {
                if conn.contains_key("api_key") {
                    conn.insert("api_key".into(), serde_json::Value::String("***".into()));
                }
                if let Some(sa) = conn.get_mut("service_account")
                    && let Some(sa) = sa.as_object_mut()
                    && sa.contains_key("private_key")
                {
                    sa.insert("private_key".into(), serde_json::Value::String("***".into()));
                }
            }
            for child in map.values_mut() {
                redact_secrets(child);
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(redact_secrets),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_is_stable_and_distinct() {
        let a = GaiseConnection {
            api_url: Some("https://a".into()),
            api_key: Some("k1".into()),
            ..Default::default()
        };
        let b = GaiseConnection {
            api_key: Some("k2".into()),
            ..a.clone()
        };
        assert_eq!(a.cache_key(), a.clone().cache_key());
        assert_ne!(a.cache_key(), b.cache_key());
        assert!(!a.cache_key().contains("k1"), "the key is not embedded in the discriminator");
        assert!(GaiseConnection::default().is_empty());
        assert!(!a.is_empty());
    }

    #[test]
    fn redacts_nested_secrets() {
        let mut value = serde_json::json!({
            "model": "openai::gpt-5.6",
            "connection": {"api_url": "https://x", "api_key": "sk-secret", "service_account": {"client_email": "a@b", "private_key": "-----BEGIN"}},
            "nested": [{"connection": {"api_key": "other"}}]
        });
        redact_secrets(&mut value);
        assert_eq!(value["connection"]["api_key"], "***");
        assert_eq!(value["connection"]["api_url"], "https://x");
        assert_eq!(value["connection"]["service_account"]["private_key"], "***");
        assert_eq!(value["connection"]["service_account"]["client_email"], "a@b");
        assert_eq!(value["nested"][0]["connection"]["api_key"], "***");
    }
}
