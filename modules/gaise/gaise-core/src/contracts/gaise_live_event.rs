use super::{GaiseFunctionCall, GaiseUsage};

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GaiseLiveEvent {
    SessionStarted {
        session_id: String,
        model: String,
    },
    Audio {
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
        sample_rate: u32,
    },
    Transcript {
        role: String,
        text: String,
    },
    Text {
        text: String,
    },
    Reasoning {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    ToolCall {
        id: String,
        function: GaiseFunctionCall,
    },
    ToolCallCancelled {
        ids: Vec<String>,
    },
    TurnComplete,
    Interrupted,
    Usage(GaiseUsage),
    Error {
        message: String,
    },
    SessionEnded,
}
