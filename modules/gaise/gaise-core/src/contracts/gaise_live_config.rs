use super::{GaiseGenerationConfig, GaiseTool, GaiseToolConfig};

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GaiseLiveConfig {
    pub model: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,

    #[serde(default)]
    pub modalities: Vec<GaiseLiveModality>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GaiseTool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<GaiseToolConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GaiseGenerationConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub vad_config: Option<GaiseVadConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcription: Option<GaiseTranscriptionConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,

    /// Per-request provider endpoint/credential overrides (take precedence
    /// over the router configuration for this call only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<super::GaiseConnection>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum GaiseLiveModality {
    Text,
    Audio,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GaiseVadConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_sensitivity: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_sensitivity: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub silence_duration_ms: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix_padding_ms: Option<u32>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GaiseTranscriptionConfig {
    #[serde(default)]
    pub input: bool,

    #[serde(default)]
    pub output: bool,
}

fn default_true() -> bool {
    true
}
