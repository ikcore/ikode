use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    pub max_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    pub stream: bool,
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
    pub properties: HashMap<String, OpenAIParameterProperty>,
    pub required: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIParameterProperty {
    pub r#type: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<OpenAIParameterProperty>>,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIChatStreamResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<OpenAIStreamChoice>,
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
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OpenAIEmbedInput {
    String(String),
    Array(Vec<String>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIEmbedResponse {
    pub object: String,
    pub data: Vec<OpenAIEmbedData>,
    pub model: String,
    pub usage: OpenAIUsage,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIEmbedData {
    pub object: String,
    pub embedding: Vec<f32>,
    pub index: usize,
}
