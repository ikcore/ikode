use super::OneOrMany;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct GaiseEmbeddingsRequest {
    pub model:String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    pub input:OneOrMany<String>,
}