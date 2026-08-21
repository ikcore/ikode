use crate::contracts::*;
use async_trait::async_trait;
use base64::Engine;
use futures_util::{Stream, StreamExt};
use gaise_core::GaiseClient;
use gaise_core::contracts::{
    GaiseContent, GaiseEmbeddingsRequest, GaiseEmbeddingsResponse, GaiseFunctionCall,
    GaiseInstructRequest, GaiseInstructResponse, GaiseInstructStreamResponse,
    GaiseListModelsRequest, GaiseListModelsResponse, GaiseMessage, GaiseReasoningEffort,
    GaiseStreamChunk, GaiseTool, GaiseToolCall, GaiseToolParameter, GaiseUsage, OneOrMany,
    audio_media_type, file_media_type, image_media_type, normalize_l2,
};
use std::collections::HashMap;
use std::pin::Pin;

fn modality_key(modality: &str, prefix: &str) -> String {
    let name = modality
        .strip_prefix("MODALITY_")
        .unwrap_or(modality)
        .to_ascii_lowercase();
    format!("{prefix}{name}_tokens")
}

fn insert_modality_usage(
    target: &mut HashMap<String, usize>,
    details: Option<&Vec<GeminiModalityTokenCount>>,
    prefix: &str,
) {
    if let Some(details) = details {
        for detail in details {
            *target
                .entry(modality_key(&detail.modality, prefix))
                .or_insert(0) += detail.token_count;
        }
    }
}

fn map_usage(usage: &GeminiUsageMetadata) -> GaiseUsage {
    let mut input = HashMap::new();
    if let Some(value) = usage.prompt_token_count {
        input.insert("prompt_tokens".to_string(), value);
    }
    // prompt_token_count already includes cached tokens; these are breakdowns.
    if let Some(value) = usage.cached_content_token_count {
        input.insert("cached_tokens".to_string(), value);
    }
    if let Some(value) = usage.tool_use_prompt_token_count {
        input.insert("tool_prompt_tokens".to_string(), value);
    }
    insert_modality_usage(&mut input, usage.prompt_tokens_details.as_ref(), "");
    insert_modality_usage(&mut input, usage.cache_tokens_details.as_ref(), "cached_");
    insert_modality_usage(
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
    insert_modality_usage(&mut output, usage.candidates_tokens_details.as_ref(), "");

    GaiseUsage {
        input: (!input.is_empty()).then_some(input),
        output: (!output.is_empty()).then_some(output),
        total: usage
            .total_token_count
            .map(|value| HashMap::from([("total_tokens".to_string(), value)])),
    }
}

pub struct GaiseClientGemini {
    api_url: String,
    api_key: String,
    client: reqwest::Client,
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

fn map_inline_data(inline: &GeminiInlineData) -> Option<GaiseContent> {
    let data = base64::prelude::BASE64_STANDARD.decode(&inline.data).ok()?;
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

fn map_stream_response(chunk: GeminiResponse) -> Vec<GaiseInstructStreamResponse> {
    let mut events = Vec::new();

    for candidate in &chunk.candidates {
        let Some(content) = &candidate.content else {
            continue;
        };
        for (index, part) in content.parts.iter().enumerate() {
            if let Some(function_call) = &part.function_call {
                events.push(GaiseInstructStreamResponse {
                    chunk: GaiseStreamChunk::ToolCall {
                        index,
                        id: Some(
                            function_call
                                .id
                                .clone()
                                .unwrap_or_else(|| function_call.name.clone()),
                        ),
                        name: Some(function_call.name.clone()),
                        arguments: function_call.args.as_ref().map(ToString::to_string),
                        thought_signature: part.thought_signature.clone(),
                    },
                    external_id: None,
                });
            }
            if let Some(text) = &part.text {
                let chunk = if part.thought.unwrap_or(false) {
                    GaiseStreamChunk::Content(GaiseContent::Reasoning {
                        text: text.clone(),
                        signature: part.thought_signature.clone(),
                    })
                } else {
                    GaiseStreamChunk::Text(text.clone())
                };
                events.push(GaiseInstructStreamResponse {
                    chunk,
                    external_id: None,
                });
            }
            if let Some(inline_data) = &part.inline_data
                && let Some(content) = map_inline_data(inline_data)
            {
                events.push(GaiseInstructStreamResponse {
                    chunk: GaiseStreamChunk::Content(content),
                    external_id: None,
                });
            }
        }
    }

    if let Some(usage) = &chunk.usage_metadata {
        events.push(GaiseInstructStreamResponse {
            chunk: GaiseStreamChunk::Usage(map_usage(usage)),
            external_id: None,
        });
    }

    events
}

fn append_content_part(parts: &mut Vec<GeminiPart>, content: GaiseContent) {
    match content {
        GaiseContent::Text { text } => parts.push(GeminiPart {
            text: Some(text),
            ..Default::default()
        }),
        GaiseContent::Image { data, format } => parts.push(GeminiPart {
            inline_data: Some(GeminiInlineData {
                mime_type: image_media_type(format.as_deref()),
                data: base64::prelude::BASE64_STANDARD.encode(data),
            }),
            ..Default::default()
        }),
        GaiseContent::Audio { data, format } => parts.push(GeminiPart {
            inline_data: Some(GeminiInlineData {
                mime_type: audio_media_type(format.as_deref()),
                data: base64::prelude::BASE64_STANDARD.encode(data),
            }),
            ..Default::default()
        }),
        GaiseContent::File { data, name } => {
            let media_type = file_media_type(name.as_deref());
            if supports_inline_file(media_type) {
                parts.push(GeminiPart {
                    inline_data: Some(GeminiInlineData {
                        mime_type: media_type.to_string(),
                        data: base64::prelude::BASE64_STANDARD.encode(data),
                    }),
                    ..Default::default()
                });
            } else if let Ok(text) = String::from_utf8(data) {
                let name = name.unwrap_or_else(|| "document".to_string());
                parts.push(GeminiPart {
                    text: Some(format!(
                        "<attached_document name=\"{name}\">\n{text}\n</attached_document>"
                    )),
                    ..Default::default()
                });
            } else {
                let name = name.unwrap_or_else(|| "document".to_string());
                parts.push(GeminiPart {
                    text: Some(format!(
                        "[Unsupported inline binary document for Gemini generateContent: {name}]"
                    )),
                    ..Default::default()
                });
            }
        }
        GaiseContent::Reasoning { text, signature } => parts.push(GeminiPart {
            text: Some(text),
            thought: Some(true),
            thought_signature: signature,
            ..Default::default()
        }),
        GaiseContent::RedactedReasoning { .. } => parts.push(GeminiPart {
            text: Some("[Encrypted reasoning retained only on its source provider]".to_string()),
            ..Default::default()
        }),
        GaiseContent::Parts { parts: nested } => {
            for content in nested {
                append_content_part(parts, content);
            }
        }
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
    media: &mut Vec<GeminiFunctionResponsePart>,
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
                media.push(GeminiFunctionResponsePart {
                    inline_data: GeminiFunctionResponseBlob {
                        display_name: function_response_media_name(None, &media_type, index),
                        mime_type: media_type,
                        data: base64::prelude::BASE64_STANDARD.encode(data),
                    },
                });
            } else {
                text.push(format!(
                    "[Unsupported Gemini function-response image type: {media_type}]"
                ));
            }
        }
        GaiseContent::File { data, name } => {
            let media_type = file_media_type(name.as_deref());
            if matches!(media_type, "application/pdf" | "text/plain") {
                let index = media.len() + 1;
                media.push(GeminiFunctionResponsePart {
                    inline_data: GeminiFunctionResponseBlob {
                        display_name: function_response_media_name(
                            name.as_deref(),
                            media_type,
                            index,
                        ),
                        mime_type: media_type.to_string(),
                        data: base64::prelude::BASE64_STANDARD.encode(data),
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
                    "[Unsupported binary Gemini function-response document: {name}]"
                ));
            }
        }
        GaiseContent::Audio { format, .. } => text.push(format!(
            "[Unsupported Gemini function-response audio type: {}]",
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

fn append_system_text(parts: &mut Vec<GeminiPart>, content: &GaiseContent) {
    match content {
        GaiseContent::Text { text } => parts.push(GeminiPart {
            text: Some(text.clone()),
            ..Default::default()
        }),
        GaiseContent::Parts { parts: nested } => {
            for part in nested {
                append_system_text(parts, part);
            }
        }
        _ => {}
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

fn map_gaise_role_to_gemini(role: &str) -> String {
    match role {
        "assistant" => "model".to_string(),
        "tool" => "user".to_string(),
        _ => role.to_string(), // "user" stays "user", "system" handled separately
    }
}

fn map_gemini_role_to_gaise(role: &str) -> String {
    match role {
        "model" => "assistant".to_string(),
        _ => role.to_string(),
    }
}

fn map_tool_parameter(param: &GaiseToolParameter) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    if let Some(t) = &param.r#type {
        let mapped = if t == "text" { "string" } else { t.as_str() };
        obj.insert("type".into(), serde_json::Value::String(mapped.to_string()));
    }
    if let Some(desc) = &param.description {
        obj.insert(
            "description".into(),
            serde_json::Value::String(desc.clone()),
        );
    }
    if let Some(props) = &param.properties {
        let mut properties = serde_json::Map::new();
        for (k, v) in props {
            properties.insert(k.clone(), map_tool_parameter(v));
        }
        obj.insert("properties".into(), serde_json::Value::Object(properties));
    }
    if let Some(items) = &param.items {
        obj.insert("items".into(), map_tool_parameter(items));
    }
    if let Some(req) = &param.required {
        obj.insert(
            "required".into(),
            serde_json::Value::Array(
                req.iter()
                    .map(|r| serde_json::Value::String(r.clone()))
                    .collect(),
            ),
        );
    }
    serde_json::Value::Object(obj)
}

impl From<&Vec<GaiseTool>> for GeminiToolSet {
    fn from(tools: &Vec<GaiseTool>) -> Self {
        GeminiToolSet {
            function_declarations: tools
                .iter()
                .map(|t| GeminiFunctionDeclaration {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.as_ref().map(map_tool_parameter),
                })
                .collect(),
        }
    }
}

impl From<&GaiseInstructRequest> for GeminiRequest {
    fn from(request: &GaiseInstructRequest) -> Self {
        let messages = match &request.input {
            OneOrMany::One(m) => vec![m.clone()],
            OneOrMany::Many(ms) => ms.clone(),
        };

        // Extract system messages → systemInstruction, rest → contents
        let mut system_parts: Vec<GeminiPart> = Vec::new();
        let mut contents: Vec<GeminiContent> = Vec::new();

        for msg in messages {
            if msg.role == "system" {
                // System messages become top-level systemInstruction
                if let Some(c) = &msg.content {
                    let items = match c {
                        OneOrMany::One(item) => vec![item.clone()],
                        OneOrMany::Many(items) => items.clone(),
                    };
                    for item in &items {
                        append_system_text(&mut system_parts, item);
                    }
                }
                continue;
            }

            let role = map_gaise_role_to_gemini(&msg.role);
            let mut parts: Vec<GeminiPart> = Vec::new();

            // Gemini function responses use the user role and require the function
            // name in addition to the optional provider call ID.
            if msg.role == "tool" || msg.tool_call_id.is_some() {
                let mut text_parts = Vec::new();
                let mut media_parts = Vec::new();
                if let Some(c) = &msg.content {
                    match c {
                        OneOrMany::One(item) => collect_function_response_content(
                            item,
                            &mut text_parts,
                            &mut media_parts,
                        ),
                        OneOrMany::Many(items) => {
                            for item in items {
                                collect_function_response_content(
                                    item,
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
                parts.push(GeminiPart {
                    function_response: Some(GeminiFunctionResponse {
                        id: call_id,
                        name,
                        response: function_response_value(text_parts.join("\n")),
                        parts: (!media_parts.is_empty()).then_some(media_parts),
                    }),
                    ..Default::default()
                });
                contents.push(GeminiContent {
                    role: Some(role),
                    parts,
                });
                continue;
            }

            // Handle regular content
            if let Some(c) = &msg.content {
                let items = match c {
                    OneOrMany::One(item) => vec![item.clone()],
                    OneOrMany::Many(items) => items.clone(),
                };
                for item in items {
                    append_content_part(&mut parts, item);
                }
            }

            // Handle assistant tool calls → functionCall parts
            if let Some(tcs) = &msg.tool_calls {
                for tc in tcs {
                    let args: Option<serde_json::Value> = tc
                        .function
                        .arguments
                        .as_ref()
                        .and_then(|a| serde_json::from_str(a).ok());
                    parts.push(GeminiPart {
                        function_call: Some(GeminiFunctionCall {
                            id: (!tc.id.is_empty()).then_some(tc.id.clone()),
                            name: tc.function.name.clone(),
                            args,
                        }),
                        thought_signature: tc.thought_signature.clone(),
                        ..Default::default()
                    });
                }
            }

            contents.push(GeminiContent {
                role: Some(role),
                parts,
            });
        }

        let system_instruction = if system_parts.is_empty() {
            None
        } else {
            Some(GeminiSystemInstruction {
                parts: system_parts,
            })
        };

        let generation_config = request.generation_config.as_ref().map(|gc| {
            let thinking_config = if model_uses_thinking_level(&request.model) {
                gc.thinking_effort
                    .as_deref()
                    .map(|effort| normalize_thinking_level(&request.model, effort))
                    .or_else(|| {
                        gc.thinking_tokens
                            .map(|tokens| Some(thinking_level_from_tokens(&request.model, tokens)))
                    })
                    .map(|thinking_level| GeminiThinkingConfig {
                        thinking_budget: None,
                        thinking_level,
                        include_thoughts: Some(gc.include_thoughts.unwrap_or(true)),
                    })
            } else {
                thinking_budget_for(
                    &request.model,
                    gc.thinking_tokens,
                    gc.thinking_effort.as_deref(),
                )
                .map(|budget| GeminiThinkingConfig {
                    thinking_budget: Some(budget),
                    thinking_level: None,
                    include_thoughts: Some(gc.include_thoughts.unwrap_or(true)),
                })
            };

            let fixed_sampling = model_uses_fixed_sampling(&request.model);

            GeminiGenerationConfig {
                temperature: (!fixed_sampling).then_some(gc.temperature).flatten(),
                top_p: (!fixed_sampling).then_some(gc.top_p).flatten(),
                top_k: (!fixed_sampling).then_some(gc.top_k).flatten(),
                max_output_tokens: gc.max_tokens,
                candidate_count: None,
                thinking_config,
                response_modalities: gc.response_modalities.as_ref().map(|modalities| {
                    modalities
                        .iter()
                        .map(|value| value.to_uppercase())
                        .collect()
                }),
                // responseFormat is the current REST shape; imageConfig remains
                // in the wire type only for deserializing older fixtures.
                image_config: None,
                response_format: gc.image_config.as_ref().map(|config| {
                    GeminiResponseFormatConfig {
                        image: Some(GeminiImageConfig {
                            aspect_ratio: config.aspect_ratio.clone(),
                            image_size: config.image_size.clone(),
                        }),
                    }
                }),
                media_resolution: gc
                    .input_media_resolution
                    .as_ref()
                    .map(|resolution| normalize_media_resolution(resolution)),
            }
        });

        let tools = request
            .tools
            .as_ref()
            .map(|ts| vec![GeminiToolSet::from(ts)]);

        GeminiRequest {
            contents,
            system_instruction,
            generation_config,
            tools,
            safety_settings: Some(GeminiSafetySetting::defaults()),
        }
    }
}

impl GaiseClientGemini {
    pub fn new(api_url: String, api_key: String) -> Self {
        Self {
            api_url,
            api_key,
            client: reqwest::Client::new(),
        }
    }

    /// One page of `GET /models`.
    pub async fn list_models_page(
        &self,
        page_token: Option<&str>,
    ) -> Result<GeminiModelList, Box<dyn std::error::Error + Send + Sync>> {
        let mut url = format!("{}/models?pageSize=1000&key={}", self.api_url, self.api_key);
        if let Some(token) = page_token {
            url.push_str("&pageToken=");
            url.push_str(token);
        }
        let response = self.client.get(url).send().await?;
        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format!("Gemini API error: {}", err_text).into());
        }
        let body = response.text().await?;
        serde_json::from_str(&body).map_err(|e| {
            let snippet: String = body.chars().take(400).collect();
            format!("failed to parse Gemini models response: {e}; body starts: {snippet}").into()
        })
    }

    fn map_from_gemini_content(&self, content: &GeminiContent) -> GaiseMessage {
        let role = content
            .role
            .as_deref()
            .map(map_gemini_role_to_gaise)
            .unwrap_or_else(|| "assistant".to_string());

        let mut content_parts: Vec<GaiseContent> = Vec::new();
        let mut tool_calls: Vec<GaiseToolCall> = Vec::new();

        for part in &content.parts {
            if let Some(text) = &part.text {
                if part.thought.unwrap_or(false) {
                    content_parts.push(GaiseContent::Reasoning {
                        text: text.clone(),
                        signature: part.thought_signature.clone(),
                    });
                } else {
                    content_parts.push(GaiseContent::Text { text: text.clone() });
                }
            }
            if let Some(inline_data) = &part.inline_data
                && let Some(content) = map_inline_data(inline_data)
            {
                content_parts.push(content);
            }
            if let Some(fc) = &part.function_call {
                tool_calls.push(GaiseToolCall {
                    id: fc.id.clone().unwrap_or_else(|| fc.name.clone()),
                    r#type: "function".to_string(),
                    function: GaiseFunctionCall {
                        name: fc.name.clone(),
                        arguments: fc.args.as_ref().map(|a| a.to_string()),
                    },
                    thought_signature: part.thought_signature.clone(),
                });
            }
        }

        let content = if content_parts.is_empty() {
            None
        } else if content_parts.len() == 1 {
            Some(OneOrMany::One(content_parts.remove(0)))
        } else {
            Some(OneOrMany::Many(content_parts))
        };

        let tool_calls = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        };

        GaiseMessage {
            role,
            content,
            tool_calls,
            tool_call_id: None,
            tool_name: None,
        }
    }
}

#[async_trait]
impl GaiseClient for GaiseClientGemini {
    async fn list_models(
        &self,
        request: &GaiseListModelsRequest,
    ) -> Result<GaiseListModelsResponse, Box<dyn std::error::Error + Send + Sync>> {
        let mut models = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let page = self.list_models_page(token.as_deref()).await?;
            models.extend(
                page.models
                    .iter()
                    .map(|m| map_gemini_model(m, request.include_raw)),
            );
            match page.next_page_token {
                Some(next) if !next.is_empty() && token.as_deref() != Some(next.as_str()) => {
                    token = Some(next)
                }
                _ => break,
            }
        }
        let mut response = GaiseListModelsResponse::from_models(models);
        response.retain_operation(request.operation);
        Ok(response)
    }

    async fn instruct_stream(
        &self,
        request: &GaiseInstructRequest,
    ) -> Result<
        Pin<
            Box<
                dyn Stream<
                        Item = Result<
                            GaiseInstructStreamResponse,
                            Box<dyn std::error::Error + Send + Sync>,
                        >,
                    > + Send,
            >,
        >,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            self.api_url, request.model, self.api_key
        );

        let gemini_request = GeminiRequest::from(request);

        let response = self.client.post(&url).json(&gemini_request).send().await?;

        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format!("Gemini API error: {}", err_text).into());
        }

        let stream = response.bytes_stream();

        // Network chunks are not aligned to SSE lines, so preserve partial lines
        // and emit every content part from every complete event.
        let mapped_stream = stream
            .scan(Vec::<u8>::new(), |buffer, result| {
                let mut events: Vec<
                    Result<GaiseInstructStreamResponse, Box<dyn std::error::Error + Send + Sync>>,
                > = Vec::new();
                match result {
                    Err(error) => events.push(Err(Box::new(error))),
                    Ok(bytes) => {
                        buffer.extend_from_slice(&bytes);
                        while let Some(position) = buffer.iter().position(|byte| *byte == b'\n') {
                            let line = buffer.drain(..=position).collect::<Vec<_>>();
                            let line = String::from_utf8_lossy(&line);
                            let line = line.trim();
                            let Some(json) = line.strip_prefix("data:").map(str::trim_start) else {
                                continue;
                            };
                            match serde_json::from_str::<GeminiResponse>(json) {
                                Ok(chunk) => {
                                    events.extend(map_stream_response(chunk).into_iter().map(Ok))
                                }
                                Err(error) => events.push(Err(Box::new(error))),
                            }
                        }
                    }
                }
                futures_util::future::ready(Some(futures_util::stream::iter(events)))
            })
            .flatten();

        Ok(Box::pin(mapped_stream))
    }

    async fn instruct(
        &self,
        request: &GaiseInstructRequest,
    ) -> Result<GaiseInstructResponse, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!(
            "{}/models/{}:generateContent?key={}",
            self.api_url, request.model, self.api_key
        );

        let gemini_request = GeminiRequest::from(request);

        let response = self.client.post(&url).json(&gemini_request).send().await?;

        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format!("Gemini API error: {}", err_text).into());
        }

        let gemini_response: GeminiResponse = response.json().await?;

        let usage = gemini_response.usage_metadata.as_ref().map(map_usage);

        let messages: Vec<GaiseMessage> = gemini_response
            .candidates
            .iter()
            .filter_map(|c| c.content.as_ref())
            .map(|content| self.map_from_gemini_content(content))
            .collect();

        let output = if messages.len() == 1 {
            OneOrMany::One(messages.into_iter().next().unwrap())
        } else {
            OneOrMany::Many(messages)
        };

        Ok(GaiseInstructResponse {
            output,
            external_id: None,
            usage,
        })
    }

    async fn embeddings(
        &self,
        request: &GaiseEmbeddingsRequest,
    ) -> Result<GaiseEmbeddingsResponse, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!(
            "{}/models/{}:batchEmbedContents?key={}",
            self.api_url, request.model, self.api_key
        );

        let (batch_request, resolved) = gemini_embed_request(request);

        let response = self.client.post(&url).json(&batch_request).send().await?;

        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format!("Gemini API error: {}", err_text).into());
        }

        let gemini_response: GeminiBatchEmbedResponse = response.json().await?;

        let mut output: Vec<Vec<f32>> = gemini_response
            .embeddings
            .into_iter()
            .map(|e| e.values)
            .collect();
        if resolved.normalize_locally {
            output.iter_mut().for_each(|v| normalize_l2(v));
        }
        Ok(GaiseEmbeddingsResponse {
            external_id: None,
            output,
            usage: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_generated_image_reasoning_and_tool_signature_without_http() {
        let response = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: Some(GeminiContent {
                    role: Some("model".to_string()),
                    parts: vec![
                        GeminiPart {
                            text: Some("summary".to_string()),
                            thought: Some(true),
                            thought_signature: Some("reasoning-signature".to_string()),
                            ..Default::default()
                        },
                        GeminiPart {
                            inline_data: Some(GeminiInlineData {
                                mime_type: "image/png".to_string(),
                                data: "AQID".to_string(),
                            }),
                            ..Default::default()
                        },
                        GeminiPart {
                            function_call: Some(GeminiFunctionCall {
                                id: Some("call-123".to_string()),
                                name: "get_weather".to_string(),
                                args: Some(serde_json::json!({"city": "London"})),
                            }),
                            thought_signature: Some("tool-signature".to_string()),
                            ..Default::default()
                        },
                    ],
                }),
                ..Default::default()
            }],
            ..Default::default()
        };

        let events = map_stream_response(response);
        assert!(matches!(
            &events[0].chunk,
            GaiseStreamChunk::Content(GaiseContent::Reasoning { text, signature })
                if text == "summary" && signature.as_deref() == Some("reasoning-signature")
        ));
        assert!(matches!(
            &events[1].chunk,
            GaiseStreamChunk::Content(GaiseContent::Image { data, format })
                if data == &[1, 2, 3] && format.as_deref() == Some("image/png")
        ));
        assert!(matches!(
            &events[2].chunk,
            GaiseStreamChunk::ToolCall { thought_signature, .. }
                if thought_signature.as_deref() == Some("tool-signature")
        ));
    }

    #[test]
    fn maps_usage_totals_modalities_cache_tools_and_reasoning() {
        let usage: GeminiUsageMetadata = serde_json::from_value(serde_json::json!({
            "promptTokenCount": 100,
            "cachedContentTokenCount": 20,
            "candidatesTokenCount": 80,
            "toolUsePromptTokenCount": 5,
            "thoughtsTokenCount": 12,
            "totalTokenCount": 197,
            "promptTokensDetails": [
                {"modality": "TEXT", "tokenCount": 40},
                {"modality": "IMAGE", "tokenCount": 25},
                {"modality": "AUDIO", "tokenCount": 35}
            ],
            "cacheTokensDetails": [{"modality": "TEXT", "tokenCount": 20}],
            "candidatesTokensDetails": [
                {"modality": "TEXT", "tokenCount": 30},
                {"modality": "IMAGE", "tokenCount": 20},
                {"modality": "AUDIO", "tokenCount": 30}
            ],
            "toolUsePromptTokensDetails": [{"modality": "TEXT", "tokenCount": 5}]
        }))
        .unwrap();

        let mapped = map_usage(&usage);
        let input = mapped.input.unwrap();
        let output = mapped.output.unwrap();
        assert_eq!(input.get("text_tokens"), Some(&40));
        assert_eq!(input.get("image_tokens"), Some(&25));
        assert_eq!(input.get("audio_tokens"), Some(&35));
        assert_eq!(input.get("cached_text_tokens"), Some(&20));
        assert_eq!(input.get("tool_text_tokens"), Some(&5));
        assert_eq!(output.get("text_tokens"), Some(&30));
        assert_eq!(output.get("image_tokens"), Some(&20));
        assert_eq!(output.get("audio_tokens"), Some(&30));
        assert_eq!(output.get("reasoning_tokens"), Some(&12));
        assert!(!output.contains_key("total_tokens"));
        assert_eq!(mapped.total.unwrap().get("total_tokens"), Some(&197));
    }
}
