use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Client → Server events ──────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeSessionUpdate {
    pub r#type: String, // "session.update"
    pub session: OpenAIRealtimeSessionConfig,
}

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeSessionConfig {
    pub r#type: String, // "realtime"

    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_modalities: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<Value>, // number or "inf"

    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<OpenAIRealtimeAudioConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<OpenAIRealtimeReasoning>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAIRealtimeTool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeReasoning {
    pub effort: String,
}

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeAudioConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<OpenAIRealtimeAudioInputConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<OpenAIRealtimeAudioOutputConfig>,
}

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeAudioInputConfig {
    pub format: OpenAIRealtimeAudioFormat,

    // `null` explicitly disables server VAD; omission would retain the API
    // default. Always serialize this field so the two states remain distinct.
    pub turn_detection: Option<OpenAIRealtimeTurnDetection>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcription: Option<OpenAIRealtimeTranscriptionConfig>,
}

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeAudioOutputConfig {
    pub format: OpenAIRealtimeAudioFormat,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeAudioFormat {
    pub r#type: String, // "audio/pcm"
    pub rate: u32,
}

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeTool {
    pub r#type: String, // "function"
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: Value,
}

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeTurnDetection {
    pub r#type: String, // "server_vad"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_response: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interrupt_response: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix_padding_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silence_duration_ms: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeTranscriptionConfig {
    pub model: String,
}

// ── Audio buffer append ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeAudioAppend {
    pub r#type: String, // "input_audio_buffer.append"
    pub audio: String,  // base64 PCM16
}

// ── Audio buffer commit ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeAudioCommit {
    pub r#type: String, // "input_audio_buffer.commit"
}

// ── Conversation item create (text or tool response) ────────────────

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeItemCreate {
    pub r#type: String, // "conversation.item.create"
    pub item: OpenAIRealtimeItem,
}

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeItem {
    pub r#type: String, // "message" or "function_call_output"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<OpenAIRealtimeItemContent>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeItemContent {
    pub r#type: String, // "input_text"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// ── Response create (trigger model response) ────────────────────────

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeResponseCreate {
    pub r#type: String, // "response.create"
}

#[derive(Debug, Serialize)]
pub struct OpenAIRealtimeSimpleEvent {
    pub r#type: String,
}

// ── Server → Client events ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct OpenAIRealtimeServerEvent {
    pub r#type: String,

    // session.created / session.updated
    #[serde(default)]
    pub session: Option<Value>,

    // response.output_audio.delta (and legacy response.audio.delta)
    #[serde(default)]
    pub delta: Option<String>,

    // response.audio_transcript.delta / response.text.delta
    // (reuse delta field)

    // response.function_call_arguments.done
    #[serde(default)]
    pub call_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
    #[serde(default)]
    pub output_index: Option<usize>,
    #[serde(default)]
    pub item_id: Option<String>,

    // conversation.item.input_audio_transcription.completed
    #[serde(default)]
    pub transcript: Option<String>,
    #[serde(default)]
    pub usage: Option<OpenAIRealtimeTranscriptionUsage>,

    // response.done
    #[serde(default)]
    pub response: Option<OpenAIRealtimeResponseDone>,

    // error
    #[serde(default)]
    pub error: Option<OpenAIRealtimeError>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIRealtimeResponseDone {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub usage: Option<OpenAIRealtimeResponseUsage>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIRealtimeResponseUsage {
    #[serde(default)]
    pub total_tokens: Option<usize>,
    #[serde(default)]
    pub input_tokens: Option<usize>,
    #[serde(default)]
    pub output_tokens: Option<usize>,

    #[serde(default)]
    pub input_token_details: Option<OpenAIRealtimeInputTokenDetails>,

    #[serde(default)]
    pub output_token_details: Option<OpenAIRealtimeOutputTokenDetails>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIRealtimeInputTokenDetails {
    #[serde(default)]
    pub cached_tokens: Option<usize>,
    #[serde(default)]
    pub text_tokens: Option<usize>,
    #[serde(default)]
    pub audio_tokens: Option<usize>,
    #[serde(default)]
    pub image_tokens: Option<usize>,
    #[serde(default)]
    pub cached_tokens_details: Option<OpenAIRealtimeCachedTokenDetails>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIRealtimeCachedTokenDetails {
    #[serde(default)]
    pub text_tokens: Option<usize>,
    #[serde(default)]
    pub audio_tokens: Option<usize>,
    #[serde(default)]
    pub image_tokens: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIRealtimeOutputTokenDetails {
    #[serde(default)]
    pub text_tokens: Option<usize>,
    #[serde(default)]
    pub audio_tokens: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIRealtimeTranscriptionUsage {
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub input_tokens: Option<usize>,
    #[serde(default)]
    pub output_tokens: Option<usize>,
    #[serde(default)]
    pub total_tokens: Option<usize>,
    #[serde(default)]
    pub seconds: Option<f64>,
    #[serde(default)]
    pub input_token_details: Option<OpenAIRealtimeTranscriptionInputTokenDetails>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIRealtimeTranscriptionInputTokenDetails {
    #[serde(default)]
    pub text_tokens: Option<usize>,
    #[serde(default)]
    pub audio_tokens: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIRealtimeError {
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
}
