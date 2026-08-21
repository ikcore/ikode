use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Request ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiRequest {
    pub contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GeminiSystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GeminiToolSet>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<GeminiSafetySetting>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct GeminiSystemInstruction {
    pub parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct GeminiContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<GeminiInlineData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<GeminiFunctionCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_response: Option<GeminiFunctionResponse>,
    /// Opaque signature Gemini returns alongside functionCall parts.
    /// Must be echoed back in multi-turn tool conversations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
    /// Marks a text part as a thought summary rather than final answer text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiInlineData {
    pub mime_type: String,
    pub data: String, // base64
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct GeminiFunctionCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct GeminiFunctionResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub response: Value,
    /// Gemini 3 can consume media returned by a function as nested response
    /// parts. Older models ignore or reject this field, so adapters only emit it
    /// when the caller actually supplied supported media.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<GeminiFunctionResponsePart>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiFunctionResponsePart {
    pub inline_data: GeminiFunctionResponseBlob,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiFunctionResponseBlob {
    pub mime_type: String,
    pub display_name: String,
    pub data: String,
}

// ── Generation Config ────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<GeminiThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_modalities: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_config: Option<GeminiImageConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<GeminiResponseFormatConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_resolution: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiImageConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_size: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiResponseFormatConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<GeminiImageConfig>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiThinkingConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_thoughts: Option<bool>,
}

// ── Tools ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiToolSet {
    pub function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct GeminiFunctionDeclaration {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>, // JSON Schema object
}

// ── Safety ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiSafetySetting {
    pub category: String,
    pub threshold: String,
}

impl GeminiSafetySetting {
    pub fn defaults() -> Vec<Self> {
        vec![
            Self {
                category: "HARM_CATEGORY_HATE_SPEECH".into(),
                threshold: "OFF".into(),
            },
            Self {
                category: "HARM_CATEGORY_DANGEROUS_CONTENT".into(),
                threshold: "OFF".into(),
            },
            Self {
                category: "HARM_CATEGORY_SEXUALLY_EXPLICIT".into(),
                threshold: "OFF".into(),
            },
            Self {
                category: "HARM_CATEGORY_HARASSMENT".into(),
                threshold: "OFF".into(),
            },
        ]
    }
}

// ── Response ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiResponse {
    #[serde(default)]
    pub candidates: Vec<GeminiCandidate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiCandidate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiUsageMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_token_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates_token_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_token_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_content_token_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_prompt_token_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thoughts_token_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<Vec<GeminiModalityTokenCount>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_tokens_details: Option<Vec<GeminiModalityTokenCount>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates_tokens_details: Option<Vec<GeminiModalityTokenCount>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_prompt_tokens_details: Option<Vec<GeminiModalityTokenCount>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiModalityTokenCount {
    pub modality: String,
    pub token_count: usize,
}

// ── Embeddings ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiBatchEmbedRequest {
    pub requests: Vec<GeminiEmbedRequest>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiEmbedRequest {
    pub model: String,
    pub content: GeminiContent,
    /// `RETRIEVAL_DOCUMENT`, `RETRIEVAL_QUERY`, ... — accepted by
    /// `gemini-embedding-001`; `gemini-embedding-2` rejects it.
    #[serde(rename = "taskType", skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    /// Matryoshka truncation (128..=3072 on the Gemini embedding models).
    #[serde(
        rename = "outputDimensionality",
        skip_serializing_if = "Option::is_none"
    )]
    pub output_dimensionality: Option<u32>,
}

/// Build the `batchEmbedContents` body through the shared embedding rules
/// (`model-registry.toml` profiles). `gemini-embedding-001` gets `taskType`
/// and raw truncation (normalized locally); `gemini-embedding-2` gets the
/// task as a prompt instruction and no `taskType`. Unknown models default to
/// `taskType` and pass `outputDimensionality` through.
pub fn gemini_embed_request(
    request: &gaise_core::contracts::GaiseEmbeddingsRequest,
) -> (
    GeminiBatchEmbedRequest,
    gaise_core::contracts::ResolvedEmbedding,
) {
    use gaise_core::contracts::{EmbeddingTaskControl, resolve_embedding};
    let profile = gaise_core::registry::embedding_profile("gemini", &request.model);
    let resolved = resolve_embedding(request, profile, &EmbeddingTaskControl::TaskType);
    let requests = resolved
        .texts
        .iter()
        .map(|text| GeminiEmbedRequest {
            model: format!("models/{}", request.model),
            content: GeminiContent {
                role: None,
                parts: vec![GeminiPart {
                    text: Some(text.clone()),
                    ..Default::default()
                }],
            },
            task_type: resolved.wire_task.clone(),
            output_dimensionality: resolved.dimensions,
        })
        .collect();
    (GeminiBatchEmbedRequest { requests }, resolved)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiBatchEmbedResponse {
    #[serde(default)]
    pub embeddings: Vec<GeminiEmbeddingValues>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiEmbeddingValues {
    pub values: Vec<f32>,
}
