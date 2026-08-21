use serde_json::Value;

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GaiseLiveInput {
    Audio {
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
        sample_rate: u32,
    },
    /// A still image or video frame for realtime vision models.
    ///
    /// OpenAI Realtime currently accepts PNG and JPEG data URIs. Gemini Live
    /// accepts image frames through its realtime `video` stream.
    Image {
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
        mime_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    Text {
        text: String,
    },
    ToolResponse {
        call_id: String,
        name: String,
        result: Value,
    },
    /// Mark the beginning of user activity when provider-side VAD is disabled.
    ActivityStart,
    /// Mark the end of user activity. Providers with a manual audio buffer use
    /// this to commit the buffer and request a response.
    ActivityEnd,
    /// Flush a paused audio stream without closing the realtime session.
    AudioStreamEnd,
    /// Clear buffered, not-yet-committed input audio where supported.
    ClearAudio,
    /// Cancel the response currently being generated where supported.
    CancelResponse,
    Close,
}
