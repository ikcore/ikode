use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct OpenAIChatRequest {
    pub model: String,
    pub messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAITool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    // Processing tier for the request (e.g. "flex", "priority", "default", "auto").
    // Sourced from the OPENAI_API_TIER env var; omitted entirely when unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    pub stream: bool,
    // Only sent on streaming requests; asks OpenAI to emit a final usage-only chunk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<OpenAIStreamOptions>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct OpenAIStreamOptions {
    pub include_usage: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct OpenAIMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<OpenAIContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OpenAIContent {
    Text(String),
    Parts(Vec<OpenAIContentPart>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OpenAIContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: OpenAIImageUrl },
    #[serde(rename = "input_audio")]
    InputAudio { input_audio: OpenAIInputAudio },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIImageUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIInputAudio {
    pub data: String,
    pub format: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAITool {
    pub r#type: String,
    pub function: OpenAIFunction,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIFunction {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: OpenAIParameters,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIParameters {
    pub r#type: String,
    // BTreeMap for deterministic, sorted key order in the serialised request — keeps
    // the tools block byte-stable across turns so OpenAI prompt caching extends past it.
    pub properties: BTreeMap<String, OpenAIParameterProperty>,
    pub required: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIParameterProperty {
    pub r#type: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<OpenAIParameterProperty>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<BTreeMap<String, OpenAIParameterProperty>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIToolCall {
    pub id: String,
    pub r#type: String,
    pub function: OpenAIFunctionCall,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIChatResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<OpenAIChoice>,
    pub usage: Option<OpenAIUsage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIChoice {
    pub index: usize,
    pub message: OpenAIMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct OpenAIUsage {
    #[serde(default)]
    pub prompt_tokens: usize,
    #[serde(default)]
    pub completion_tokens: usize,
    #[serde(default)]
    pub total_tokens: usize,
    // `prompt_tokens` already includes the cached portion; this breaks it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<OpenAIPromptTokensDetails>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens_details: Option<OpenAICompletionTokensDetails>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct OpenAIPromptTokensDetails {
    #[serde(default)]
    pub audio_tokens: Option<usize>,
    #[serde(default)]
    pub cached_tokens: Option<usize>,
    #[serde(default)]
    pub cache_write_tokens: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct OpenAICompletionTokensDetails {
    #[serde(default)]
    pub accepted_prediction_tokens: Option<usize>,
    #[serde(default)]
    pub audio_tokens: Option<usize>,
    #[serde(default)]
    pub reasoning_tokens: Option<usize>,
    #[serde(default)]
    pub rejected_prediction_tokens: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIChatStreamResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<OpenAIStreamChoice>,
    // Present only on the final chunk when `stream_options.include_usage` is set;
    // that chunk carries an empty `choices` array.
    #[serde(default)]
    pub usage: Option<OpenAIUsage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIStreamChoice {
    pub index: usize,
    pub delta: OpenAIStreamDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIStreamDelta {
    pub role: Option<String>,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<OpenAIStreamToolCall>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIStreamToolCall {
    pub index: usize,
    pub id: Option<String>,
    pub r#type: Option<String>,
    pub function: Option<OpenAIStreamFunctionCall>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIStreamFunctionCall {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIEmbedRequest {
    pub model: String,
    pub input: OpenAIEmbedInput,
    /// Matryoshka shortening; only `text-embedding-3-*` accept it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OpenAIEmbedInput {
    String(String),
    Array(Vec<String>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIEmbedResponse {
    #[serde(default)]
    pub object: String,
    pub data: Vec<OpenAIEmbedData>,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub usage: OpenAIEmbedUsage,
}

/// Usage for the **embeddings** endpoint. Unlike chat completions, an embeddings
/// response carries only `prompt_tokens` and `total_tokens` — there is no
/// `completion_tokens` — so it needs its own struct rather than reusing
/// [`OpenAIUsage`] (whose required `completion_tokens` makes the embeddings body fail
/// to deserialize). All fields default to be resilient to OpenAI-compatible proxies
/// that omit usage.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct OpenAIEmbedUsage {
    #[serde(default)]
    pub prompt_tokens: usize,
    #[serde(default)]
    pub total_tokens: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIEmbedData {
    pub object: String,
    pub embedding: Vec<f32>,
    pub index: usize,
}
