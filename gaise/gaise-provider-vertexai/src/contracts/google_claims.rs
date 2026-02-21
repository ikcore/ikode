use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GoogleClaims {
    pub iss: String,
    pub scope: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
}