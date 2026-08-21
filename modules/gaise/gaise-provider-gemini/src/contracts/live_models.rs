use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Client → Server messages ────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveSetup {
    pub setup: GeminiLiveSetupConfig,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveSetupConfig {
    pub model: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiLiveGenerationConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GeminiLiveSystemInstruction>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GeminiLiveToolSet>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub realtime_input_config: Option<GeminiLiveRealtimeInputConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio_transcription: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_audio_transcription: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_modalities: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub speech_config: Option<GeminiLiveSpeechConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<usize>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<usize>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<super::models::GeminiThinkingConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_resolution: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveSpeechConfig {
    pub voice_config: GeminiLiveVoiceConfig,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveVoiceConfig {
    pub prebuilt_voice_config: GeminiLivePrebuiltVoice,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLivePrebuiltVoice {
    pub voice_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveRealtimeInputConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automatic_activity_detection: Option<GeminiLiveVadConfig>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveVadConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_of_speech_sensitivity: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_of_speech_sensitivity: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix_padding_ms: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub silence_duration_ms: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveSystemInstruction {
    pub parts: Vec<GeminiLiveTextPart>,
}

#[derive(Debug, Serialize)]
pub struct GeminiLiveTextPart {
    pub text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveToolSet {
    pub function_declarations: Vec<GeminiLiveFunctionDeclaration>,
}

#[derive(Debug, Serialize)]
pub struct GeminiLiveFunctionDeclaration {
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

// ── Realtime Input (audio / text / end) ────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveRealtimeInput {
    pub realtime_input: GeminiLiveRealtimeInputData,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveRealtimeInputData {
    /// Deprecated by Gemini, retained only so older serialized callers remain
    /// source compatible. New code should use `audio` or `video`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_chunks: Option<Vec<GeminiLiveMediaChunk>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<GeminiLiveMediaChunk>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<GeminiLiveMediaChunk>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_start: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_end: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_stream_end: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveMediaChunk {
    pub mime_type: String,
    pub data: String, // base64
}

// ── Client Content (text message with history) ─────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveClientContent {
    pub client_content: GeminiLiveClientContentData,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveClientContentData {
    pub turns: Vec<GeminiLiveTurn>,
    pub turn_complete: bool,
}

#[derive(Debug, Serialize)]
pub struct GeminiLiveTurn {
    pub role: String,
    pub parts: Vec<GeminiLiveTextPart>,
}

// ── Tool Response ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveToolResponse {
    pub tool_response: GeminiLiveToolResponseData,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveToolResponseData {
    pub function_responses: Vec<GeminiLiveFunctionResponse>,
}

#[derive(Debug, Serialize)]
pub struct GeminiLiveFunctionResponse {
    pub id: String,
    pub name: String,
    pub response: Value,
}

// ── Server → Client messages ────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveServerMessage {
    #[serde(default)]
    pub error: Option<Value>,

    #[serde(default)]
    pub setup_complete: Option<Value>,

    #[serde(default)]
    pub server_content: Option<GeminiLiveServerContent>,

    #[serde(default)]
    pub tool_call: Option<GeminiLiveToolCall>,

    #[serde(default)]
    pub tool_call_cancellation: Option<GeminiLiveToolCallCancellation>,

    #[serde(default)]
    pub usage_metadata: Option<GeminiLiveUsageMetadata>,

    #[serde(default)]
    pub go_away: Option<GeminiLiveGoAway>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveServerContent {
    #[serde(default)]
    pub model_turn: Option<GeminiLiveModelTurn>,

    #[serde(default)]
    pub turn_complete: Option<bool>,

    #[serde(default)]
    pub interrupted: Option<bool>,

    #[serde(default)]
    pub generation_complete: Option<bool>,

    #[serde(default)]
    pub input_transcription: Option<GeminiLiveTranscription>,

    #[serde(default)]
    pub output_transcription: Option<GeminiLiveTranscription>,
}

#[derive(Debug, Deserialize)]
pub struct GeminiLiveModelTurn {
    #[serde(default)]
    pub parts: Vec<GeminiLiveServerPart>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveServerPart {
    #[serde(default)]
    pub text: Option<String>,

    #[serde(default)]
    pub thought: Option<bool>,

    #[serde(default)]
    pub thought_signature: Option<String>,

    #[serde(default)]
    pub inline_data: Option<GeminiLiveInlineData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveInlineData {
    pub mime_type: String,
    pub data: String, // base64
}

#[derive(Debug, Deserialize)]
pub struct GeminiLiveTranscription {
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveToolCall {
    #[serde(default)]
    pub function_calls: Vec<GeminiLiveServerFunctionCall>,
}

#[derive(Debug, Deserialize)]
pub struct GeminiLiveServerFunctionCall {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub args: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct GeminiLiveToolCallCancellation {
    #[serde(default)]
    pub ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveUsageMetadata {
    #[serde(default)]
    pub prompt_token_count: Option<usize>,

    #[serde(default)]
    pub cached_content_token_count: Option<usize>,

    #[serde(default)]
    pub response_token_count: Option<usize>,

    #[serde(default)]
    pub tool_use_prompt_token_count: Option<usize>,

    #[serde(default)]
    pub thoughts_token_count: Option<usize>,

    #[serde(default)]
    pub total_token_count: Option<usize>,

    #[serde(default)]
    pub prompt_tokens_details: Option<Vec<GeminiLiveModalityTokenCount>>,

    #[serde(default)]
    pub cache_tokens_details: Option<Vec<GeminiLiveModalityTokenCount>>,

    #[serde(default)]
    pub response_tokens_details: Option<Vec<GeminiLiveModalityTokenCount>>,

    #[serde(default)]
    pub tool_use_prompt_tokens_details: Option<Vec<GeminiLiveModalityTokenCount>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveModalityTokenCount {
    pub modality: String,
    pub token_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveGoAway {
    #[serde(default)]
    pub time_left: Option<String>,
}
