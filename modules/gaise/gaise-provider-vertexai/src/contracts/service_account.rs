use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServiceAccount {
    pub private_key: String,
    pub client_email: String,
}

impl ServiceAccount {
    /// Build from individual env vars (matches .NET convention).
    /// Required: `GOOGLE_ACCOUNT_ID`, `GOOGLE_PRIVATE_KEY`.
    pub fn from_env() -> Option<Self> {
        let client_email = std::env::var("GOOGLE_ACCOUNT_ID")
            .ok()
            .filter(|s| !s.is_empty())?;
        let private_key = std::env::var("GOOGLE_PRIVATE_KEY")
            .ok()
            .filter(|s| !s.is_empty())?;
        let private_key = private_key.replace("\\n", "\n");
        Some(Self {
            private_key,
            client_email,
        })
    }

    /// Build from a JSON string (e.g. `VERTEXAI_SA_JSON` env var).
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }
}
