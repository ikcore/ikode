
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GaiseGenerationConfig {

    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature:Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k:Option<usize>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p:Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens:Option<usize>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_tokens:Option<usize>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_effort:Option<String>, // low, medium, high

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key:Option<String>,
}