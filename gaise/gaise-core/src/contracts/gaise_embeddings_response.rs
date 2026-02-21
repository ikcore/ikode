use super::GaiseUsage;

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GaiseEmbeddingsResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id:Option<String>,
    pub output:Vec<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage:Option<GaiseUsage>
}