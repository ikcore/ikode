
use super::{GaiseGenerationConfig, GaiseMessage, OneOrMany, GaiseTool, GaiseToolConfig};


#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GaiseInstructRequest {

    pub model:String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools:Option<Vec<GaiseTool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_config:Option<GaiseToolConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config:Option<GaiseGenerationConfig>,

    pub input:OneOrMany<GaiseMessage>
}
