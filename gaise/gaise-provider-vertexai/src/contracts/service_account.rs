use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServiceAccount {
    pub private_key: String,
    pub client_email: String,
}