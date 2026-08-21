use base64::Engine;
use gaise_core::contracts::{
    GaiseContent, GaiseEmbeddingsRequest, GaiseEmbeddingsResponse, GaiseInstructRequest,
    GaiseInstructResponse, GaiseInstructStreamResponse, GaiseMessage, GaiseReasoningEffort,
    GaiseStreamChunk, GaiseUsage, OneOrMany, audio_media_type, file_media_type, image_media_type,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn google_modality_key(modality: &str, prefix: &str) -> String {
    let name = modality
        .strip_prefix("MODALITY_")
        .unwrap_or(modality)
        .to_ascii_lowercase();
    format!("{prefix}{name}_tokens")
}

fn insert_google_modality_usage(
    target: &mut HashMap<String, usize>,
    details: Option<&Vec<GoogleModalityTokenCount>>,
    prefix: &str,
) {
    if let Some(details) = details {
        for detail in details {
            *target
                .entry(google_modality_key(&detail.modality, prefix))
                .or_insert(0) += detail.token_count;
        }
    }
}

fn map_google_usage(usage: &GoogleUsageMetadata) -> GaiseUsage {
    let mut input = HashMap::new();
    if let Some(value) = usage.prompt_token_count {
        input.insert("prompt_tokens".to_string(), value);
    }
    if let Some(value) = usage.cached_content_token_count {
        input.insert("cached_tokens".to_string(), value);
    }
    if let Some(value) = usage.tool_use_prompt_token_count {
        input.insert("tool_prompt_tokens".to_string(), value);
    }
    insert_google_modality_usage(&mut input, usage.prompt_tokens_details.as_ref(), "");
    insert_google_modality_usage(&mut input, usage.cache_tokens_details.as_ref(), "cached_");
    insert_google_modality_usage(
        &mut input,
        usage.tool_use_prompt_tokens_details.as_ref(),
        "tool_",
    );

    let mut output = HashMap::new();
    if let Some(value) = usage.candidates_token_count {
        output.insert("candidates_tokens".to_string(), value);
    }
    if let Some(value) = usage.thoughts_token_count {
        output.insert("reasoning_tokens".to_string(), value);
    }
    insert_google_modality_usage(&mut output, usage.candidates_tokens_details.as_ref(), "");

    GaiseUsage {
        input: (!input.is_empty()).then_some(input),
        output: (!output.is_empty()).then_some(output),
        total: usage
            .total_token_count
            .map(|value| HashMap::from([("total_tokens".to_string(), value)])),
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleAccessToken {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: usize,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleInstructRequest {
    pub contents: Vec<GoogleContent>,

    #[serde(rename = "system_instruction", skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GoogleContent>,

    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GoogleParameters>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GoogleTool>>,

    #[serde(rename = "toolConfig", skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<GoogleToolConfig>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleTool {
    #[serde(rename = "functionDeclarations")]
    pub function_declarations: Vec<GoogleFunctionDeclaration>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleFunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: GoogleSchema,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleSchema {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<std::collections::HashMap<String, GoogleSchema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<GoogleSchema>>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleToolConfig {
    #[serde(rename = "functionCallingConfig")]
    pub function_calling_config: GoogleFunctionCallingConfig,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleFunctionCallingConfig {
    pub mode: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleContent {
    pub role: String,
    pub parts: Vec<GooglePart>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GooglePart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(rename = "inlineData", skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<GoogleInlineData>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<GoogleFunctionCall>,
    #[serde(rename = "functionResponse", skip_serializing_if = "Option::is_none")]
    pub tool_response: Option<GoogleToolResponse>,
    #[serde(rename = "thought", skip_serializing_if = "Option::is_none")]
    pub thought: Option<bool>,
    #[serde(rename = "thoughtSignature", skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleFunctionCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleToolResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub response: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<GoogleFunctionResponsePart>>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleFunctionResponsePart {
    #[serde(rename = "inlineData")]
    pub inline_data: GoogleFunctionResponseBlob,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleFunctionResponseBlob {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub data: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleInlineData {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub data: String,
}

/// Gemini family rules for `generationConfig`, audited 2026-08-20 against
/// the Gemini API and Vertex AI thinking/model pages (see
/// `wiki/vendor-gemini.md#model-family-rules`). Gemini 3.x uses
/// `thinkingLevel`, 2.5 uses `thinkingBudget`; sampling parameters are
/// deprecated (ignored or rejected) on every Gemini 3.x model.
pub fn model_uses_thinking_level(model: &str) -> bool {
    model.to_ascii_lowercase().starts_with("gemini-3")
}

pub fn model_uses_fixed_sampling(model: &str) -> bool {
    model.to_ascii_lowercase().starts_with("gemini-3")
}

const LEVELS_ALL: &[&str] = &["MINIMAL", "LOW", "MEDIUM", "HIGH"];
const LEVELS_NO_MINIMAL: &[&str] = &["LOW", "MEDIUM", "HIGH"];
const LEVELS_IMAGE: &[&str] = &["MINIMAL", "HIGH"];
const LEVELS_HIGH_ONLY: &[&str] = &["HIGH"];

/// `thinkingLevel` values a Gemini 3.x family accepts.
pub fn thinking_levels_for(model: &str) -> &'static [&'static str] {
    let m = model.to_ascii_lowercase();
    if m.contains("pro-image") {
        LEVELS_HIGH_ONLY
    } else if m.contains("-image") {
        LEVELS_IMAGE
    } else if m.starts_with("gemini-3.7")
        || m.contains("gemini-3.1-pro")
        || m.contains("gemini-3-pro")
    {
        LEVELS_NO_MINIMAL
    } else {
        LEVELS_ALL
    }
}

/// Resolve a provider-neutral effort onto the family's `thinkingLevel` set
/// through the canonical vocabulary ([`GaiseReasoningEffort`]). Thinking
/// cannot be disabled on Gemini 3.x, so `none` becomes the lowest level;
/// `xhigh`/`max`/`ultra` become the highest; `auto` returns `None` (the
/// thinking block is sent without a level so the model picks); custom
/// strings are forwarded upper-cased.
pub fn normalize_thinking_level(model: &str, effort: &str) -> Option<String> {
    let accepted: Vec<GaiseReasoningEffort> = thinking_levels_for(model)
        .iter()
        .map(|l| GaiseReasoningEffort::parse(l))
        .collect();
    match GaiseReasoningEffort::parse(effort) {
        GaiseReasoningEffort::Auto => None,
        GaiseReasoningEffort::Custom(raw) => Some(raw.to_ascii_uppercase()),
        level => Some(level.clamp_to(&accepted).as_str().to_ascii_uppercase()),
    }
}

/// Map a manual token budget onto a 3.x `thinkingLevel`.
pub fn thinking_level_from_tokens(model: &str, tokens: usize) -> String {
    let level = match tokens {
        0..=2_000 => "low",
        2_001..=12_000 => "medium",
        _ => "high",
    };
    normalize_thinking_level(model, level).unwrap_or_else(|| "HIGH".to_string())
}

/// `thinkingBudget` for Gemini 2.5, clamped to the family's documented range:
/// Pro 128–32,768 (cannot be disabled), Flash 0–24,576, Flash-Lite 0 or
/// 512–24,576. `None` when neither a budget nor an effort was requested.
pub fn thinking_budget_for(
    model: &str,
    tokens: Option<usize>,
    effort: Option<&str>,
) -> Option<i64> {
    let m = model.to_ascii_lowercase();
    let (min_on, max, can_disable) = if m.contains("gemini-2.5-pro") {
        (128, 32_768, false)
    } else if m.contains("flash-lite") {
        (512, 24_576, true)
    } else if m.contains("gemini-2.5") {
        (1, 24_576, true)
    } else {
        // Unknown older family: forward the budget untouched.
        return tokens.map(|t| t as i64);
    };
    let requested = match (tokens, effort.map(GaiseReasoningEffort::parse)) {
        (Some(t), _) => t,
        // `auto` is Gemini's dynamic budget (-1): the model decides.
        (None, Some(GaiseReasoningEffort::Auto)) => return Some(-1),
        (None, Some(GaiseReasoningEffort::None)) => 0,
        (None, Some(GaiseReasoningEffort::Minimal)) => min_on.max(512),
        (None, Some(GaiseReasoningEffort::Low)) => 2_048,
        (None, Some(GaiseReasoningEffort::Medium)) => 8_192,
        (None, Some(GaiseReasoningEffort::High)) => max.min(24_576),
        (
            None,
            Some(
                GaiseReasoningEffort::XHigh
                | GaiseReasoningEffort::Max
                | GaiseReasoningEffort::Ultra,
            ),
        ) => max,
        (None, Some(GaiseReasoningEffort::Custom(_))) | (None, None) => return None,
    };
    Some(if requested == 0 {
        if can_disable { 0 } else { min_on as i64 }
    } else {
        requested.clamp(min_on, max) as i64
    })
}

fn normalize_media_resolution(resolution: &str) -> String {
    let upper = resolution.to_ascii_uppercase();
    if upper.starts_with("MEDIA_RESOLUTION_") {
        upper
    } else {
        format!("MEDIA_RESOLUTION_{upper}")
    }
}

fn supports_inline_file(media_type: &str) -> bool {
    media_type == "application/pdf"
        || media_type == "application/json"
        || media_type == "application/rtf"
        || media_type == "application/xml"
        || media_type.starts_with("text/")
}

fn response_file_name(media_type: &str) -> String {
    let extension = match media_type {
        "application/pdf" => "pdf",
        "application/json" => "json",
        "text/csv" => "csv",
        "text/html" => "html",
        "text/markdown" => "md",
        _ => "bin",
    };
    format!("response.{extension}")
}

fn inline_data_to_gaise(inline: &GoogleInlineData) -> Option<GaiseContent> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(&inline.data)
        .ok()?;
    if inline.mime_type.starts_with("image/") {
        Some(GaiseContent::Image {
            data,
            format: Some(inline.mime_type.clone()),
        })
    } else if inline.mime_type.starts_with("audio/") {
        Some(GaiseContent::Audio {
            data,
            format: Some(inline.mime_type.clone()),
        })
    } else {
        Some(GaiseContent::File {
            data,
            name: Some(response_file_name(&inline.mime_type)),
        })
    }
}

impl GoogleContent {
    pub fn from(gaise: &GaiseContent, role: String) -> GoogleContent {
        GoogleContent {
            role,
            parts: vec![GooglePart::from(gaise)],
        }
    }
    pub fn from_many(gaise: &[GaiseContent], role: String) -> GoogleContent {
        GoogleContent {
            role,
            parts: gaise.iter().flat_map(GooglePart::from_gaise).collect(),
        }
    }
}

impl GooglePart {
    fn text(text: String) -> Self {
        Self {
            text: Some(text),
            inline_data: None,
            tool_call: None,
            tool_response: None,
            thought: None,
            thought_signature: None,
        }
    }

    fn inline(data: &[u8], mime_type: String) -> Self {
        Self {
            text: None,
            inline_data: Some(GoogleInlineData {
                mime_type,
                data: base64::engine::general_purpose::STANDARD.encode(data),
            }),
            tool_call: None,
            tool_response: None,
            thought: None,
            thought_signature: None,
        }
    }

    pub fn from_gaise(gaise: &GaiseContent) -> Vec<GooglePart> {
        match gaise {
            GaiseContent::Text { text } => vec![Self::text(text.clone())],
            GaiseContent::Audio { data, format } => {
                vec![Self::inline(data, audio_media_type(format.as_deref()))]
            }
            GaiseContent::Image { data, format } => {
                vec![Self::inline(data, image_media_type(format.as_deref()))]
            }
            GaiseContent::File { data, name } => {
                let media_type = file_media_type(name.as_deref());
                if supports_inline_file(media_type) {
                    vec![Self::inline(data, media_type.to_string())]
                } else if let Ok(text) = String::from_utf8(data.clone()) {
                    let name = name.as_deref().unwrap_or("document");
                    vec![Self::text(format!(
                        "<attached_document name=\"{name}\">\n{text}\n</attached_document>"
                    ))]
                } else {
                    let name = name.as_deref().unwrap_or("document");
                    vec![Self::text(format!(
                        "[Unsupported inline binary document for Vertex AI generateContent: {name}]"
                    ))]
                }
            }
            GaiseContent::Reasoning { text, signature } => vec![Self {
                text: Some(text.clone()),
                inline_data: None,
                tool_call: None,
                tool_response: None,
                thought: Some(true),
                thought_signature: signature.clone(),
            }],
            GaiseContent::RedactedReasoning { .. } => vec![Self::text(
                "[Encrypted reasoning retained only on its source provider]".to_string(),
            )],
            GaiseContent::Parts { parts } => {
                parts.iter().flat_map(GooglePart::from_gaise).collect()
            }
        }
    }

    pub fn from(gaise: &GaiseContent) -> GooglePart {
        GooglePart::from_gaise(gaise)
            .into_iter()
            .next()
            .unwrap_or_else(|| GooglePart::text(String::new()))
    }
}

fn collect_text_content(content: &GaiseContent, output: &mut Vec<String>) {
    match content {
        GaiseContent::Text { text } => output.push(text.clone()),
        GaiseContent::Parts { parts } => {
            for part in parts {
                collect_text_content(part, output);
            }
        }
        _ => {}
    }
}

fn function_response_media_name(preferred: Option<&str>, media_type: &str, index: usize) -> String {
    let extension = match media_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "application/pdf" => "pdf",
        "text/plain" => "txt",
        _ => "bin",
    };
    let name = preferred
        .filter(|name| !name.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("tool-result-{index}.{extension}"));
    name.chars().take(128).collect()
}

fn collect_function_response_content(
    content: &GaiseContent,
    text: &mut Vec<String>,
    media: &mut Vec<GoogleFunctionResponsePart>,
) {
    match content {
        GaiseContent::Text { text: value } => text.push(value.clone()),
        GaiseContent::Image { data, format } => {
            let media_type = image_media_type(format.as_deref());
            if matches!(
                media_type.as_str(),
                "image/png" | "image/jpeg" | "image/webp"
            ) {
                let index = media.len() + 1;
                media.push(GoogleFunctionResponsePart {
                    inline_data: GoogleFunctionResponseBlob {
                        display_name: function_response_media_name(None, &media_type, index),
                        mime_type: media_type,
                        data: base64::engine::general_purpose::STANDARD.encode(data),
                    },
                });
            } else {
                text.push(format!(
                    "[Unsupported Vertex AI function-response image type: {media_type}]"
                ));
            }
        }
        GaiseContent::File { data, name } => {
            let media_type = file_media_type(name.as_deref());
            if matches!(media_type, "application/pdf" | "text/plain") {
                let index = media.len() + 1;
                media.push(GoogleFunctionResponsePart {
                    inline_data: GoogleFunctionResponseBlob {
                        display_name: function_response_media_name(
                            name.as_deref(),
                            media_type,
                            index,
                        ),
                        mime_type: media_type.to_string(),
                        data: base64::engine::general_purpose::STANDARD.encode(data),
                    },
                });
            } else if let Ok(value) = std::str::from_utf8(data) {
                let name = name.as_deref().unwrap_or("document");
                text.push(format!(
                    "<attached_document name=\"{name}\">\n{value}\n</attached_document>"
                ));
            } else {
                let name = name.as_deref().unwrap_or("document");
                text.push(format!(
                    "[Unsupported binary Vertex AI function-response document: {name}]"
                ));
            }
        }
        GaiseContent::Audio { format, .. } => text.push(format!(
            "[Unsupported Vertex AI function-response audio type: {}]",
            audio_media_type(format.as_deref())
        )),
        GaiseContent::Reasoning { text: value, .. } => text.push(value.clone()),
        GaiseContent::RedactedReasoning { .. } => {
            text.push("[Encrypted reasoning retained only on its source provider]".to_string())
        }
        GaiseContent::Parts { parts } => {
            for part in parts {
                collect_function_response_content(part, text, media);
            }
        }
    }
}

fn function_response_value(text: String) -> serde_json::Value {
    if text.trim().is_empty() {
        return serde_json::json!({});
    }
    let parsed = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
    if parsed.is_object() {
        parsed
    } else {
        serde_json::json!({ "result": parsed })
    }
}

impl GoogleInstructRequest {
    pub fn add_content(&mut self, msg: GaiseMessage) {
        if msg.role == "system" {
            if let Some(ref content) = msg.content {
                let mut text_parts = Vec::new();
                match content {
                    OneOrMany::One(content) => collect_text_content(content, &mut text_parts),
                    OneOrMany::Many(contents) => {
                        for content in contents {
                            collect_text_content(content, &mut text_parts);
                        }
                    }
                }
                let parts = text_parts.into_iter().map(GooglePart::text);
                if let Some(system) = self.system_instruction.as_mut() {
                    system.parts.extend(parts);
                } else {
                    self.system_instruction = Some(GoogleContent {
                        role: "system".to_owned(),
                        parts: parts.collect(),
                    });
                }
            }
            return;
        }

        let mut parts = vec![];

        if msg.role == "tool" || msg.tool_call_id.is_some() {
            let mut text_parts = Vec::new();
            let mut media_parts = Vec::new();
            if let Some(ref content) = msg.content {
                match content {
                    OneOrMany::One(content) => collect_function_response_content(
                        content,
                        &mut text_parts,
                        &mut media_parts,
                    ),
                    OneOrMany::Many(contents) => {
                        for content in contents {
                            collect_function_response_content(
                                content,
                                &mut text_parts,
                                &mut media_parts,
                            );
                        }
                    }
                }
            }
            let call_id = msg.tool_call_id.clone();
            let name = msg
                .tool_name
                .clone()
                .or_else(|| call_id.clone())
                .unwrap_or_default();
            parts.push(GooglePart {
                text: None,
                inline_data: None,
                tool_call: None,
                tool_response: Some(GoogleToolResponse {
                    id: call_id,
                    name,
                    response: function_response_value(text_parts.join("\n")),
                    parts: (!media_parts.is_empty()).then_some(media_parts),
                }),
                thought: None,
                thought_signature: None,
            });
        } else {
            if let Some(ref content) = msg.content {
                match content {
                    OneOrMany::One(x) => {
                        parts.extend(GooglePart::from_gaise(x));
                    }
                    OneOrMany::Many(items) => {
                        for item in items {
                            parts.extend(GooglePart::from_gaise(item));
                        }
                    }
                }
            }

            if let Some(ref tool_calls) = msg.tool_calls {
                for tc in tool_calls {
                    parts.push(GooglePart {
                        text: None,
                        inline_data: None,
                        tool_call: Some(GoogleFunctionCall {
                            id: (!tc.id.is_empty()).then_some(tc.id.clone()),
                            name: tc.function.name.clone(),
                            args: tc
                                .function
                                .arguments
                                .as_ref()
                                .and_then(|a| serde_json::from_str(a).ok())
                                .unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
                        }),
                        tool_response: None,
                        thought: None,
                        thought_signature: tc.thought_signature.clone(),
                    });
                }
            }
        }

        if !parts.is_empty() {
            self.contents.push(GoogleContent {
                role: to_google_role(&msg.role).unwrap_or(msg.role),
                parts,
            });
        }
    }

    pub fn from(source: &GaiseInstructRequest) -> GoogleInstructRequest {
        let mut request = GoogleInstructRequest {
            contents: vec![],
            system_instruction: None,
            generation_config: source.generation_config.as_ref().map(|gc| {
                let fixed_sampling = model_uses_fixed_sampling(&source.model);
                let thinking_config = if model_uses_thinking_level(&source.model) {
                    gc.thinking_effort
                        .as_deref()
                        .map(|effort| normalize_thinking_level(&source.model, effort))
                        .or_else(|| {
                            gc.thinking_tokens.map(|tokens| {
                                Some(thinking_level_from_tokens(&source.model, tokens))
                            })
                        })
                        .map(|thinking_level| GoogleThinkingConfig {
                            thinking_level,
                            thinking_budget: None,
                            include_thoughts: Some(gc.include_thoughts.unwrap_or(true)),
                        })
                } else {
                    thinking_budget_for(
                        &source.model,
                        gc.thinking_tokens,
                        gc.thinking_effort.as_deref(),
                    )
                    .map(|budget| GoogleThinkingConfig {
                        thinking_level: None,
                        thinking_budget: Some(budget),
                        include_thoughts: Some(gc.include_thoughts.unwrap_or(true)),
                    })
                };

                GoogleParameters {
                    temperature: (!fixed_sampling).then_some(gc.temperature).flatten(),
                    max_output_tokens: gc.max_tokens,
                    top_p: (!fixed_sampling).then_some(gc.top_p).flatten(),
                    top_k: (!fixed_sampling).then_some(gc.top_k).flatten(),
                    thinking_config,
                    response_modalities: gc.response_modalities.as_ref().map(|modalities| {
                        modalities
                            .iter()
                            .map(|value| value.to_uppercase())
                            .collect()
                    }),
                    image_config: None,
                    response_format: gc.image_config.as_ref().map(|config| {
                        GoogleResponseFormatConfig {
                            image: Some(GoogleImageConfig {
                                aspect_ratio: config.aspect_ratio.clone(),
                                image_size: config.image_size.clone(),
                            }),
                        }
                    }),
                    media_resolution: gc
                        .input_media_resolution
                        .as_ref()
                        .map(|resolution| normalize_media_resolution(resolution)),
                    ..Default::default()
                }
            }),
            tools: source.tools.as_ref().map(|tools| {
                vec![GoogleTool {
                    function_declarations: tools
                        .iter()
                        .map(|t| GoogleFunctionDeclaration {
                            name: t.name.clone(),
                            description: t.description.clone().unwrap_or_default(),
                            parameters: t.parameters.as_ref().map(GoogleSchema::from).unwrap_or(
                                GoogleSchema {
                                    r#type: "object".to_string(),
                                    description: None,
                                    properties: Some(std::collections::HashMap::new()),
                                    required: None,
                                    items: None,
                                },
                            ),
                        })
                        .collect(),
                }]
            }),
            tool_config: source.tool_config.as_ref().map(|tc| GoogleToolConfig {
                function_calling_config: GoogleFunctionCallingConfig {
                    mode: tc.mode.clone().unwrap_or("AUTO".to_string()).to_uppercase(),
                },
            }),
        };
        match &source.input {
            OneOrMany::One(x) => {
                request.add_content(x.clone());
            }
            OneOrMany::Many(vx) => {
                for x in vx.iter() {
                    request.add_content(x.clone());
                }
            }
        };
        request
    }
}

impl GoogleSchema {
    pub fn from(source: &gaise_core::contracts::GaiseToolParameter) -> GoogleSchema {
        GoogleSchema {
            r#type: source.r#type.clone().unwrap_or("object".to_string()),
            description: source.description.clone(),
            properties: source.properties.as_ref().map(|props| {
                props
                    .iter()
                    .map(|(k, v)| (k.clone(), GoogleSchema::from(v)))
                    .collect()
            }),
            required: source.required.clone(),
            items: source
                .items
                .as_ref()
                .map(|items| Box::new(GoogleSchema::from(items))),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleEmbeddingsRequest {
    pub instances: Vec<GoogleInstance>,
    pub parameters: GoogleParameters,
}

impl GoogleEmbeddingsRequest {
    pub fn from(model: &GaiseEmbeddingsRequest) -> GoogleEmbeddingsRequest {
        vertex_embed_request(model).0
    }
}

/// Build the `:predict` body through the shared embedding rules
/// (`model-registry.toml` profiles): `task_type` for `gemini-embedding-001`
/// and the `text-embedding-*` family, a prompt instruction for
/// `gemini-embedding-2`, dimensions clamped to the model's range (768 for the
/// legacy family, 3072 for Gemini Embedding), and `single_input` for
/// `gemini-embedding-001`, which takes one text per call. Unknown models
/// default to `task_type` and pass dimensions through.
pub fn vertex_embed_request(
    request: &GaiseEmbeddingsRequest,
) -> (
    GoogleEmbeddingsRequest,
    gaise_core::contracts::ResolvedEmbedding,
) {
    use gaise_core::contracts::{EmbeddingTaskControl, resolve_embedding};
    let profile = gaise_core::registry::embedding_profile("vertexai", &request.model);
    let resolved = resolve_embedding(request, profile, &EmbeddingTaskControl::TaskType);
    let instances = resolved
        .texts
        .iter()
        .map(|x| GoogleInstance {
            content: Some(x.clone()),
            task_type: resolved.wire_task.clone(),
            ..Default::default()
        })
        .collect();
    (
        GoogleEmbeddingsRequest {
            instances,
            parameters: GoogleParameters {
                auto_truncate: Some(true),
                output_dimensionality: resolved.dimensions,
                ..Default::default()
            },
        },
        resolved,
    )
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct GoogleInstance {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Embedding task type (`RETRIEVAL_DOCUMENT`, `RETRIEVAL_QUERY`, ...).
    #[serde(rename = "task_type", skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<GoogleMessage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct GoogleParameters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "topP")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "topK")]
    pub top_k: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "autoTruncate")]
    pub auto_truncate: Option<bool>,
    /// Matryoshka truncation for embedding models.
    #[serde(
        rename = "outputDimensionality",
        skip_serializing_if = "Option::is_none"
    )]
    pub output_dimensionality: Option<u32>,
    #[serde(rename = "thinkingConfig", skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<GoogleThinkingConfig>,
    #[serde(rename = "responseModalities", skip_serializing_if = "Option::is_none")]
    pub response_modalities: Option<Vec<String>>,
    #[serde(rename = "imageConfig", skip_serializing_if = "Option::is_none")]
    pub image_config: Option<GoogleImageConfig>,
    #[serde(rename = "responseFormat", skip_serializing_if = "Option::is_none")]
    pub response_format: Option<GoogleResponseFormatConfig>,
    #[serde(rename = "mediaResolution", skip_serializing_if = "Option::is_none")]
    pub media_resolution: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct GoogleImageConfig {
    #[serde(rename = "aspectRatio", skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    #[serde(rename = "imageSize", skip_serializing_if = "Option::is_none")]
    pub image_size: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct GoogleResponseFormatConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<GoogleImageConfig>,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct GoogleThinkingConfig {
    #[serde(rename = "thinkingLevel", skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
    #[serde(rename = "thinkingBudget", skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<i64>,
    #[serde(rename = "includeThoughts", skip_serializing_if = "Option::is_none")]
    pub include_thoughts: Option<bool>,
}

/*
impl GoogleParameters {
    pub fn from(request:&GaiseInstructRequest) -> GoogleParameters {
        GoogleParameters {
            temperature: Some(request.temperature.unwrap_or(0.2)),
            max_output_tokens: Some(request.max_tokens.unwrap_or(1024)),
            top_p: Some(request.top_p.unwrap_or(1.0)),
            top_k: Some(request.top_k.unwrap_or(40)),
            ..Default::default()
        }
    }
}
    */

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleMessage {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    //#[serde(rename = "citationMetadata")]
    //#[serde(skip_serializing_if = "Option::is_none")]
    //pub citation_metadata: Option<Vec<GoogleCitationMetadata>>
}

/*
impl GoogleMessage {
    pub fn from(input:&GenerativeAITextMessage) -> GoogleMessage {
        GoogleMessage {
            content: input.content.clone().unwrap(),
            author: to_google_role(&input.role),
            //citation_metadata: None,
        }
    }
}
    */

pub fn to_google_role(input: &str) -> Option<String> {
    let result = match input {
        "assistant" => "model".to_owned(),
        "tool" => "user".to_owned(),
        _ => input.to_string(),
    };
    Some(result)
}

pub fn to_gaise_role(input: &str) -> Option<String> {
    let result = match input {
        "model" => "assistant".to_owned(),
        _ => input.to_string(),
    };
    Some(result)
}

/*
{
    "candidates": [
        {
            "content": {
                "role": "model",
                "parts": [
                    {
                        "text": "Hello there! Larry here, your Soho House Agent.\n\nLet me just check that for you. You're looking for dinner tomorrow evening, **Thursday, 16th May**, at **7:00 PM** for **3 people** at **180 House** in London, correct?\n\nWhile I don't have direct real-time booking access here in our chat, the best way to secure that reservation is usually through the **Soho House app** or the **website**. You can quickly check availability and book directly there.\n\nHowever, if you'd like me to take a look for you, I can certainly try! Could you please confirm the exact date for \"tomorrow\"? Once I have that, I can guide you on the best way to proceed or see if I can assist further.\n\nLooking forward to hearing from you!"
                    }
                ]
            },
            "finishReason": "STOP",
            "avgLogprobs": -0.55297500436956237
        }
    ],
    "usageMetadata": {
        "promptTokenCount": 34,
        "candidatesTokenCount": 176,
        "totalTokenCount": 395,
        "trafficType": "ON_DEMAND",
        "promptTokensDetails": [
            {
                "modality": "TEXT",
                "tokenCount": 34
            }
        ],
        "candidatesTokensDetails": [
            {
                "modality": "TEXT",
                "tokenCount": 176
            }
        ],
        "thoughtsTokenCount": 185
    },
    "modelVersion": "gemini-2.5-flash",
    "createTime": "2025-12-22T11:52:32.716635Z",
    "responseId": "ADFJadveK5j_2fMPvvDogAg"
}
*/

#[cfg(test)]
mod usage_tests {
    use super::*;

    #[test]
    fn maps_vertex_usage_modalities_reasoning_and_neutral_total() {
        let usage: GoogleUsageMetadata = serde_json::from_value(serde_json::json!({
            "promptTokenCount": 60,
            "cachedContentTokenCount": 10,
            "candidatesTokenCount": 45,
            "toolUsePromptTokenCount": 3,
            "thoughtsTokenCount": 7,
            "totalTokenCount": 115,
            "promptTokensDetails": [
                {"modality": "TEXT", "tokenCount": 20},
                {"modality": "IMAGE", "tokenCount": 15},
                {"modality": "AUDIO", "tokenCount": 25}
            ],
            "candidatesTokensDetails": [
                {"modality": "TEXT", "tokenCount": 15},
                {"modality": "IMAGE", "tokenCount": 10},
                {"modality": "AUDIO", "tokenCount": 20}
            ],
            "cacheTokensDetails": [{"modality": "IMAGE", "tokenCount": 10}],
            "toolUsePromptTokensDetails": [{"modality": "TEXT", "tokenCount": 3}]
        }))
        .unwrap();

        let mapped = map_google_usage(&usage);
        let input = mapped.input.unwrap();
        let output = mapped.output.unwrap();
        assert_eq!(input.get("text_tokens"), Some(&20));
        assert_eq!(input.get("image_tokens"), Some(&15));
        assert_eq!(input.get("audio_tokens"), Some(&25));
        assert_eq!(input.get("cached_image_tokens"), Some(&10));
        assert_eq!(output.get("text_tokens"), Some(&15));
        assert_eq!(output.get("image_tokens"), Some(&10));
        assert_eq!(output.get("audio_tokens"), Some(&20));
        assert_eq!(output.get("reasoning_tokens"), Some(&7));
        assert!(!output.contains_key("total_tokens"));
        assert_eq!(mapped.total.unwrap().get("total_tokens"), Some(&115));
    }

    #[test]
    fn returns_vertex_embedding_usage() {
        let response: GoogleEmbeddingsResponse = serde_json::from_value(serde_json::json!({
            "predictions": [{"embeddings": {"values": [0.1, 0.2]}}],
            "metadata": {"totalBillableCharacters": 12, "totalTokens": 4}
        }))
        .unwrap();
        let mapped = response.to_view();
        let usage = mapped.usage.unwrap();
        assert_eq!(usage.input.unwrap().get("input_tokens"), Some(&4));
        assert_eq!(usage.total.unwrap().get("total_tokens"), Some(&4));
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleChatCompletionResponse {
    pub candidates: Vec<GoogleCandidate>,
    #[serde(rename = "usageMetadata", default)]
    pub usage_metadata: Option<GoogleUsageMetadata>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleCandidate {
    pub content: GoogleContent,

    #[serde(rename = "finishReason")]
    pub finish_reason: Option<String>,
}

impl GoogleChatCompletionResponse {
    pub fn to_stream_view(&self) -> Vec<GaiseInstructStreamResponse> {
        let mut responses = Vec::new();

        if let Some(usage) = &self.usage_metadata {
            let mapped = map_google_usage(usage);
            if mapped.input.is_some() || mapped.output.is_some() || mapped.total.is_some() {
                responses.push(GaiseInstructStreamResponse {
                    chunk: GaiseStreamChunk::Usage(mapped),
                    external_id: None,
                });
            }
        }

        for candidate in &self.candidates {
            for (part_idx, part) in candidate.content.parts.iter().enumerate() {
                if let Some(text) = &part.text {
                    responses.push(GaiseInstructStreamResponse {
                        chunk: if part.thought.unwrap_or(false) {
                            GaiseStreamChunk::Content(GaiseContent::Reasoning {
                                text: text.clone(),
                                signature: part.thought_signature.clone(),
                            })
                        } else {
                            GaiseStreamChunk::Text(text.clone())
                        },
                        external_id: None,
                    });
                }
                if let Some(inline_data) = &part.inline_data
                    && let Some(content) = inline_data_to_gaise(inline_data)
                {
                    responses.push(GaiseInstructStreamResponse {
                        chunk: GaiseStreamChunk::Content(content),
                        external_id: None,
                    });
                }
                if let Some(tool_call) = &part.tool_call {
                    responses.push(GaiseInstructStreamResponse {
                        chunk: GaiseStreamChunk::ToolCall {
                            index: part_idx,
                            id: Some(
                                tool_call
                                    .id
                                    .clone()
                                    .unwrap_or_else(|| tool_call.name.clone()),
                            ),
                            name: Some(tool_call.name.clone()),
                            arguments: Some(tool_call.args.to_string()),
                            thought_signature: part.thought_signature.clone(),
                        },
                        external_id: None,
                    });
                }
            }
        }

        responses
    }

    pub fn to_view(&self) -> GaiseInstructResponse {
        let outputs = self
            .candidates
            .iter()
            .map(|candidate| {
                let mut contents = vec![];
                let mut tool_calls = vec![];

                for part in &candidate.content.parts {
                    if let Some(text) = &part.text {
                        if part.thought.unwrap_or(false) {
                            contents.push(GaiseContent::Reasoning {
                                text: text.clone(),
                                signature: part.thought_signature.clone(),
                            });
                        } else {
                            contents.push(GaiseContent::Text { text: text.clone() });
                        }
                    }
                    if let Some(inline_data) = &part.inline_data
                        && let Some(content) = inline_data_to_gaise(inline_data)
                    {
                        contents.push(content);
                    }
                    if let Some(tool_call) = &part.tool_call {
                        tool_calls.push(gaise_core::contracts::GaiseToolCall {
                            id: tool_call
                                .id
                                .clone()
                                .unwrap_or_else(|| tool_call.name.clone()),
                            r#type: "function".to_string(),
                            function: gaise_core::contracts::GaiseFunctionCall {
                                name: tool_call.name.clone(),
                                arguments: Some(tool_call.args.to_string()),
                            },
                            thought_signature: part.thought_signature.clone(),
                        });
                    }
                }

                GaiseMessage {
                    role: to_gaise_role(&candidate.content.role).unwrap_or("assistant".to_string()),
                    content: if contents.is_empty() {
                        None
                    } else {
                        Some(OneOrMany::Many(contents))
                    },
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    tool_call_id: None,
                    tool_name: None,
                }
            })
            .collect();

        let usage = self.usage_metadata.as_ref().map(map_google_usage);

        GaiseInstructResponse {
            output: OneOrMany::Many(outputs),
            external_id: None,
            usage,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleEmbeddingsResponse {
    pub predictions: Vec<GooglePrediction>,
    pub metadata: Option<GoogleEmbeddingsMetadata>,
}

impl GoogleEmbeddingsResponse {
    pub fn to_view(&self) -> GaiseEmbeddingsResponse {
        let usage = self.metadata.as_ref().and_then(|metadata| {
            let mut input = HashMap::new();
            if let Some(tokens) = metadata.total_tokens {
                input.insert("input_tokens".to_string(), tokens);
            }
            if let Some(characters) = metadata.total_billable_characters {
                input.insert("billable_characters".to_string(), characters);
            }
            let total = metadata
                .total_tokens
                .map(|tokens| HashMap::from([("total_tokens".to_string(), tokens)]));
            (!input.is_empty() || total.is_some()).then_some(GaiseUsage {
                input: (!input.is_empty()).then_some(input),
                output: None,
                total,
            })
        });
        GaiseEmbeddingsResponse {
            output: self
                .predictions
                .iter()
                .filter_map(|x| x.embeddings.as_ref().map(|e| e.values.clone()))
                .collect(),
            external_id: None,
            usage,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleEmbeddings {
    pub values: Vec<f32>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GooglePrediction {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "citationMetadata")]
    pub citation_metadata: Option<Vec<GoogleCitationMetadata>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "safetyAttributes")]
    pub safety_attributes: Option<Vec<GoogleSafetyAttributes>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates: Option<Vec<GoogleMessage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embeddings: Option<GoogleEmbeddings>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleCitationMetadata {
    pub citations: Vec<serde_json::Value>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleSafetyAttributes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<bool>,
    pub scores: Vec<f32>,
    pub categories: Vec<String>,
    #[serde(rename = "safetyRatings")]
    pub safety_ratings: Vec<GoogleSafetyRating>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleSafetyRating {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "probabilityScore")]
    pub probability_score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "severityScore")]
    pub severity_score: Option<f32>,
    pub category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleEmbeddingsMetadata {
    #[serde(rename = "totalBillableCharacters")]
    pub total_billable_characters: Option<usize>,
    #[serde(rename = "totalTokens")]
    pub total_tokens: Option<usize>,
}

/*
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename="tokenMetadata")]
    pub token_metadata: Option<GoogleTokenMetadata>,
}

    */

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleUsageMetadata {
    #[serde(rename = "candidatesTokenCount")]
    pub candidates_token_count: Option<usize>,
    #[serde(rename = "promptTokenCount")]
    pub prompt_token_count: Option<usize>,
    #[serde(rename = "totalTokenCount")]
    pub total_token_count: Option<usize>,
    #[serde(rename = "thoughtsTokenCount")]
    pub thoughts_token_count: Option<usize>,
    #[serde(rename = "cachedContentTokenCount")]
    pub cached_content_token_count: Option<usize>,
    #[serde(rename = "toolUsePromptTokenCount")]
    pub tool_use_prompt_token_count: Option<usize>,
    #[serde(rename = "promptTokensDetails")]
    pub prompt_tokens_details: Option<Vec<GoogleModalityTokenCount>>,
    #[serde(rename = "cacheTokensDetails")]
    pub cache_tokens_details: Option<Vec<GoogleModalityTokenCount>>,
    #[serde(rename = "candidatesTokensDetails")]
    pub candidates_tokens_details: Option<Vec<GoogleModalityTokenCount>>,
    #[serde(rename = "toolUsePromptTokensDetails")]
    pub tool_use_prompt_tokens_details: Option<Vec<GoogleModalityTokenCount>>,
    #[serde(rename = "trafficType")]
    pub traffic_type: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GoogleModalityTokenCount {
    pub modality: String,
    pub token_count: usize,
}

/*

    "usageMetadata": {
        "promptTokenCount": 34,
        "candidatesTokenCount": 176,
        "totalTokenCount": 395,
        "trafficType": "ON_DEMAND",
        "promptTokensDetails": [
            {
                "modality": "TEXT",
                "tokenCount": 34
            }
        ],
        "candidatesTokensDetails": [
            {
                "modality": "TEXT",
                "tokenCount": 176
            }
        ],
        "thoughtsTokenCount": 185
    },
*/
