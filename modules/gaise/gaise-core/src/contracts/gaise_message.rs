use serde::{Serialize, Deserialize};
use crate::contracts::OneOrMany;

use super::{GaiseContent, GaiseToolCall};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GaiseMessage {
    pub role: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<OneOrMany<GaiseContent>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<GaiseToolCall>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    /// Function name associated with a tool result. OpenAI and Anthropic can
    /// identify a result by call ID alone, while Gemini/Vertex function responses
    /// require both the optional call ID and the function name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

impl Default for GaiseMessage {
    fn default() -> Self {
        Self {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::default())),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
        }
    }
}
