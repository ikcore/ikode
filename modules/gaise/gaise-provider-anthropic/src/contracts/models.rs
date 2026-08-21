use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AnthropicRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    pub max_tokens: usize,
    // System prompt sent as an array of text blocks so the last block can carry a
    // `cache_control` breakpoint (Anthropic only caches when explicitly marked).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<Vec<AnthropicSystemBlock>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<AnthropicThinking>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_config: Option<AnthropicOutputConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

/// Prompt-cache breakpoint marker. Attaching this to the last system block, the
/// last tool, or the last content block of a message tells Anthropic to cache the
/// entire prefix up to that point (5-minute ephemeral TTL). Without it Anthropic
/// caches nothing and re-bills the full prompt every turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicCacheControl {
    pub r#type: String,
}

impl AnthropicCacheControl {
    pub fn ephemeral() -> Self {
        Self {
            r#type: "ephemeral".to_string(),
        }
    }
}

/// One block of the `system` array. `type` is always `"text"`.
#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicSystemBlock {
    pub r#type: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<AnthropicCacheControl>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicThinking {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<usize>,
    /// Controls whether Anthropic returns a summarized or omitted thinking block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

/// Provider-native effort control. Modern Claude models expose effort under
/// `output_config`; it is separate from the optional thinking configuration and
/// affects text, tool calls, and adaptive thinking together.
#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicOutputConfig {
    pub effort: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: AnthropicContent,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

impl Default for AnthropicContent {
    fn default() -> Self {
        AnthropicContent::Text(String::new())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<AnthropicCacheControl>,
    },
    #[serde(rename = "image")]
    Image {
        source: AnthropicImageSource,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<AnthropicCacheControl>,
    },
    #[serde(rename = "document")]
    Document {
        source: AnthropicDocumentSource,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<AnthropicCacheControl>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<AnthropicCacheControl>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: AnthropicToolResultContent,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<AnthropicCacheControl>,
    },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    #[serde(rename = "redacted_thinking")]
    RedactedThinking { data: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnthropicToolResultContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

impl AnthropicContentBlock {
    /// Attach a cache breakpoint to this block (used on the last block of the last
    /// message to roll the conversation prefix into the cache each turn).
    pub fn set_cache_control(&mut self, cc: Option<AnthropicCacheControl>) {
        match self {
            AnthropicContentBlock::Text { cache_control, .. }
            | AnthropicContentBlock::Image { cache_control, .. }
            | AnthropicContentBlock::Document { cache_control, .. }
            | AnthropicContentBlock::ToolUse { cache_control, .. }
            | AnthropicContentBlock::ToolResult { cache_control, .. } => *cache_control = cc,
            AnthropicContentBlock::Thinking { .. }
            | AnthropicContentBlock::RedactedThinking { .. } => {}
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicImageSource {
    pub r#type: String,
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicDocumentSource {
    pub r#type: String,
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicTool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: AnthropicInputSchema,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<AnthropicCacheControl>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicInputSchema {
    pub r#type: String,
    // BTreeMap for deterministic, sorted key order in the serialised request — keeps
    // the tools block byte-stable across turns so prompt caching extends past it.
    pub properties: BTreeMap<String, AnthropicProperty>,
    pub required: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicProperty {
    pub r#type: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<AnthropicProperty>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<BTreeMap<String, AnthropicProperty>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicResponse {
    pub id: String,
    pub r#type: String,
    pub role: String,
    pub content: Vec<AnthropicContentBlock>,
    pub model: String,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: AnthropicUsage,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AnthropicUsage {
    #[serde(default)]
    pub input_tokens: usize,
    #[serde(default)]
    pub output_tokens: usize,
    // Prompt-caching counters. `input_tokens` from Anthropic EXCLUDES these, so the
    // true total input is input_tokens + cache_read + cache_creation.
    #[serde(default)]
    pub cache_creation_input_tokens: usize,
    #[serde(default)]
    pub cache_read_input_tokens: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation: Option<AnthropicCacheCreation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens_details: Option<AnthropicOutputTokensDetails>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_tool_use: Option<AnthropicServerToolUsage>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AnthropicCacheCreation {
    #[serde(default)]
    pub ephemeral_1h_input_tokens: usize,
    #[serde(default)]
    pub ephemeral_5m_input_tokens: usize,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AnthropicOutputTokensDetails {
    #[serde(default)]
    pub thinking_tokens: usize,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AnthropicServerToolUsage {
    #[serde(default)]
    pub web_fetch_requests: usize,
    #[serde(default)]
    pub web_search_requests: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicStreamResponse {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<AnthropicDelta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_block: Option<AnthropicContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<AnthropicStreamMessage>,
    // `message_delta` events carry the cumulative output token count at the top
    // level (only `output_tokens` is present, hence the defaulted fields above).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<AnthropicUsage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicStreamMessage {
    pub id: String,
    pub r#type: String,
    pub role: String,
    pub model: String,
    pub usage: AnthropicUsage,
}
