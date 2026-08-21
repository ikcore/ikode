use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServiceAccountInfo {
    pub private_key: String,
    pub client_email: String,
}
