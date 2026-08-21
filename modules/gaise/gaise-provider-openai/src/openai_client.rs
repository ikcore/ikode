use crate::contracts::*;
use async_trait::async_trait;
use base64::Engine;
use futures_util::{Stream, StreamExt};
use gaise_core::GaiseClient;
use gaise_core::contracts::{
    EmbeddingTaskControl, GaiseContent, GaiseEmbeddingsRequest, GaiseEmbeddingsResponse,
    GaiseFunctionCall, GaiseInstructRequest, GaiseInstructResponse, GaiseInstructStreamResponse,
    GaiseListModelsRequest, GaiseListModelsResponse, GaiseMessage, GaiseReasoningEffort,
    GaiseStreamChunk, GaiseTool, GaiseToolCall, GaiseToolParameter, GaiseUsage, OneOrMany,
    ResolvedEmbedding, image_media_type, normalize_l2, resolve_embedding,
};
use std::collections::HashMap;
use std::pin::Pin;

fn map_usage(usage: &OpenAIUsage) -> GaiseUsage {
    let mut input = HashMap::new();
    input.insert("prompt_tokens".to_string(), usage.prompt_tokens);
    if let Some(details) = &usage.prompt_tokens_details {
        if let Some(tokens) = details.audio_tokens {
            input.insert("audio_tokens".to_string(), tokens);
        }
        if let Some(tokens) = details.cached_tokens {
            input.insert("cached_tokens".to_string(), tokens);
        }
        if let Some(tokens) = details.cache_write_tokens {
            input.insert("cache_write_tokens".to_string(), tokens);
        }
    }

    let mut output = HashMap::new();
    output.insert("completion_tokens".to_string(), usage.completion_tokens);
    if let Some(details) = &usage.completion_tokens_details {
        if let Some(tokens) = details.audio_tokens {
            output.insert("audio_tokens".to_string(), tokens);
        }
        if let Some(tokens) = details.reasoning_tokens {
            output.insert("reasoning_tokens".to_string(), tokens);
        }
        if let Some(tokens) = details.accepted_prediction_tokens {
            output.insert("accepted_prediction_tokens".to_string(), tokens);
        }
        if let Some(tokens) = details.rejected_prediction_tokens {
            output.insert("rejected_prediction_tokens".to_string(), tokens);
        }
    }

    GaiseUsage {
        input: Some(input),
        output: Some(output),
        total: Some(HashMap::from([(
            "total_tokens".to_string(),
            usage.total_tokens,
        )])),
    }
}

/// Map one parsed OpenAI streaming chunk to zero or more GAISe events. OpenAI can
/// place deltas for several parallel tool calls in a single SSE event, so returning
/// a vector is required to avoid silently dropping every call after the first.
fn map_stream_chunk(chunk: OpenAIChatStreamResponse) -> Vec<GaiseInstructStreamResponse> {
    // The final usage-only chunk (sent when `stream_options.include_usage` is set)
    // carries an empty `choices` array, so handle it before the per-choice logic.
    if let Some(usage) = &chunk.usage {
        return vec![GaiseInstructStreamResponse {
            chunk: GaiseStreamChunk::Usage(map_usage(usage)),
            external_id: Some(chunk.id.clone()),
        }];
    }
    let Some(choice) = chunk.choices.first() else {
        return Vec::new();
    };
    let mut events = Vec::new();
    if let Some(tool_calls) = &choice.delta.tool_calls {
        events.extend(tool_calls.iter().map(|tc| GaiseInstructStreamResponse {
            chunk: GaiseStreamChunk::ToolCall {
                index: tc.index,
                id: tc.id.clone(),
                name: tc.function.as_ref().and_then(|f| f.name.clone()),
                arguments: tc.function.as_ref().and_then(|f| f.arguments.clone()),
                thought_signature: None,
            },
            external_id: Some(chunk.id.clone()),
        }));
    }
    if let Some(content) = &choice.delta.content {
        events.push(GaiseInstructStreamResponse {
            chunk: GaiseStreamChunk::Text(content.clone()),
            external_id: Some(chunk.id.clone()),
        });
    }
    events
}

pub struct GaiseClientOpenAI {
    api_url: String,
    api_key: String,
    client: reqwest::Client,
    /// Optional processing tier ("flex", "priority", …) read once from the
    /// `OPENAI_API_TIER` env var at construction. Stamped onto every chat request
    /// when set; left off entirely when unset so OpenAI applies its own default.
    service_tier: Option<String>,
    /// Models for which this client has seen OpenAI's "function tools with
    /// reasoning_effort are not supported" error. Once learned, later
    /// tool-bearing requests to that model go straight to `none` instead of
    /// paying a failed first attempt. Scoped per model so other models are
    /// unaffected; scoped per client instance so a fix upstream is picked up
    /// on restart.
    learned_tool_none_models: tokio::sync::RwLock<std::collections::HashSet<String>>,
}

impl From<GaiseTool> for OpenAITool {
    fn from(t: GaiseTool) -> Self {
        fn map_param(p: &GaiseToolParameter) -> OpenAIParameterProperty {
            let mut prop_type = p.r#type.clone().unwrap_or_else(|| "string".to_string());
            if prop_type == "text" {
                prop_type = "string".to_string();
            }
            OpenAIParameterProperty {
                r#type: prop_type,
                description: p.description.clone().unwrap_or_default(),
                items: p.items.as_ref().map(|i| Box::new(map_param(i))),
                properties: p.properties.as_ref().map(|properties| {
                    properties
                        .iter()
                        .map(|(name, property)| (name.clone(), map_param(property)))
                        .collect()
                }),
                required: p.required.clone(),
            }
        }

        OpenAITool {
            r#type: "function".to_string(),
            function: OpenAIFunction {
                name: t.name,
                description: t.description,
                parameters: OpenAIParameters {
                    r#type: "object".to_string(),
                    properties: t
                        .parameters
                        .as_ref()
                        .and_then(|p| p.properties.as_ref())
                        .map(|props| {
                            props
                                .iter()
                                .map(|(k, v)| (k.clone(), map_param(v)))
                                .collect()
                        })
                        .unwrap_or_default(),
                    required: t
                        .parameters
                        .as_ref()
                        .and_then(|p| p.required.clone())
                        .unwrap_or_default(),
                },
            },
        }
    }
}

fn openai_audio_format(format: Option<&str>) -> String {
    match format
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "wav" | "wave" | "audio/wav" | "audio/wave" => "wav".to_string(),
        _ => "mp3".to_string(),
    }
}

/// Chat Completions parameter rules per model family, audited 2026-08-20
/// against the OpenAI model pages, the latest-model guide, and the Chat
/// Completions reference (see `wiki/vendor-openai.md#model-family-rules`).
///
/// Unknown models get a pass-through profile so new releases keep working;
/// known families get exact constraints so invalid combinations are never
/// sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenAIChatRules {
    /// The model is served by Chat Completions at all (`-pro` and gated
    /// Daybreak models are Responses-only).
    pub chat_supported: bool,
    /// `reasoning_effort` is meaningful; non-reasoning families never receive it.
    pub reasoning: bool,
    /// Accepted `reasoning_effort` values, lowest to highest. Empty = forward as given.
    pub effort_levels: &'static [&'static str],
    /// Effort applied by the API when none is sent.
    pub default_effort: Option<&'static str>,
    /// `temperature` / `top_p` are accepted only while the effective effort is `none`.
    pub sampling_requires_none: bool,
    /// `temperature` / `top_p` are never accepted.
    pub sampling_never: bool,
    /// `image_url.detail: "original"` is accepted; otherwise it degrades to `high`.
    pub original_image_detail: bool,
}

const EFFORT_56: &[&str] = &["none", "low", "medium", "high", "xhigh", "max"];
const EFFORT_55: &[&str] = &["none", "low", "medium", "high", "xhigh"];
const EFFORT_51: &[&str] = &["none", "low", "medium", "high"];
const EFFORT_5: &[&str] = &["minimal", "low", "medium", "high"];
const EFFORT_CODEX: &[&str] = &["low", "medium", "high", "xhigh"];
const EFFORT_O: &[&str] = &["low", "medium", "high"];
const EFFORT_ANY: &[&str] = &[];

const PASS_THROUGH: OpenAIChatRules = OpenAIChatRules {
    chat_supported: true,
    reasoning: true,
    effort_levels: EFFORT_ANY,
    default_effort: None,
    sampling_requires_none: false,
    sampling_never: false,
    original_image_detail: true,
};

const NON_REASONING: OpenAIChatRules = OpenAIChatRules {
    chat_supported: true,
    reasoning: false,
    effort_levels: EFFORT_ANY,
    default_effort: None,
    sampling_requires_none: false,
    sampling_never: false,
    original_image_detail: false,
};

fn gpt5_rules(
    levels: &'static [&'static str],
    default: &'static str,
    original: bool,
) -> OpenAIChatRules {
    OpenAIChatRules {
        chat_supported: true,
        reasoning: true,
        effort_levels: levels,
        default_effort: Some(default),
        sampling_requires_none: levels.contains(&"none"),
        sampling_never: !levels.contains(&"none"),
        original_image_detail: original,
    }
}

pub fn openai_chat_rules(model: &str) -> OpenAIChatRules {
    let m = model.to_ascii_lowercase();
    let m = m.strip_prefix("ft:").unwrap_or(&m);
    let base = m.split(':').next().unwrap_or(m); // fine-tune suffixes
    let starts = |p: &str| base.starts_with(p);
    let has = |p: &str| base.contains(p);

    if has("-pro") && (starts("gpt-5") || starts("o1") || starts("o3"))
        || has("cyber")
        || starts("daybreak")
    {
        return OpenAIChatRules {
            chat_supported: false,
            ..PASS_THROUGH
        };
    }
    if has("chat-latest") || starts("chat-latest") {
        return NON_REASONING;
    }
    if starts("gpt-audio")
        || has("-audio")
        || starts("gpt-4.1")
        || starts("gpt-4o")
        || starts("gpt-4-")
        || base == "gpt-4"
        || starts("gpt-3.5")
    {
        return NON_REASONING;
    }
    if has("codex") {
        return gpt5_rules(EFFORT_CODEX, "medium", false);
    }
    if starts("gpt-5.6") {
        return gpt5_rules(EFFORT_56, "medium", true);
    }
    if starts("gpt-5.5") {
        return gpt5_rules(EFFORT_55, "medium", true);
    }
    if starts("gpt-5.4-mini") || starts("gpt-5.4-nano") {
        return gpt5_rules(EFFORT_55, "none", false);
    }
    if starts("gpt-5.4") {
        return gpt5_rules(EFFORT_55, "none", true);
    }
    if starts("gpt-5.3") || starts("gpt-5.2") {
        return gpt5_rules(EFFORT_55, "none", false);
    }
    if starts("gpt-5.1") {
        return gpt5_rules(EFFORT_51, "none", false);
    }
    if starts("gpt-5") {
        return gpt5_rules(EFFORT_5, "medium", false);
    }
    if starts("o1") || starts("o3") || starts("o4") {
        return gpt5_rules(EFFORT_O, "medium", false);
    }
    PASS_THROUGH
}

/// Resolve a provider-neutral effort onto the family's accepted
/// `reasoning_effort` values using the canonical vocabulary
/// ([`GaiseReasoningEffort`]): `auto` omits the field so OpenAI applies the
/// family default, `ultra` becomes the family's highest level, other levels
/// snap to the nearest accepted one, and custom strings pass through.
pub fn normalize_chat_effort(rules: &OpenAIChatRules, effort: &str) -> Option<String> {
    if !rules.reasoning {
        return None;
    }
    match GaiseReasoningEffort::parse(effort) {
        GaiseReasoningEffort::Auto => None,
        GaiseReasoningEffort::Custom(raw) => Some(raw),
        level => {
            let accepted = GaiseReasoningEffort::levels(rules.effort_levels);
            Some(level.clamp_to(&accepted).as_str().to_string())
        }
    }
}

/// Build the Embeddings request through the shared embedding rules
/// (`model-registry.toml` profiles): `dimensions` is clamped for
/// `text-embedding-3-*` and dropped for fixed-size models, `task` is ignored
/// because the API has no task concept. Unknown models pass `dimensions`
/// through for the API to validate.
pub fn openai_embed_request(
    request: &GaiseEmbeddingsRequest,
) -> (OpenAIEmbedRequest, ResolvedEmbedding) {
    let profile = gaise_core::registry::embedding_profile("openai", &request.model);
    let resolved = resolve_embedding(request, profile, &EmbeddingTaskControl::None);
    let input = match &request.input {
        OneOrMany::One(_) if resolved.texts.len() == 1 => {
            OpenAIEmbedInput::String(resolved.texts[0].clone())
        }
        _ => OpenAIEmbedInput::Array(resolved.texts.clone()),
    };
    (
        OpenAIEmbedRequest {
            model: request.model.clone(),
            input,
            dimensions: resolved.dimensions,
        },
        resolved,
    )
}

/// Fail fast for models OpenAI serves only through the Responses API.
fn ensure_chat_supported(model: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if openai_chat_rules(model).chat_supported {
        Ok(())
    } else {
        Err(format!(
            "OpenAI model '{model}' is not available on Chat Completions; it requires the Responses API, which the GAISe OpenAI instruct client does not implement yet"
        )
        .into())
    }
}

/// GPT-5.6 models currently reject function tools on Chat Completions unless
/// reasoning is disabled. Keep this workaround surface-specific: these models
/// support other reasoning efforts when tools are absent, and the Responses API
/// supports reasoning with tools.
fn chat_tools_require_none_reasoning(model: &str) -> bool {
    model == "gpt-5.6"
        || model
            .strip_prefix("gpt-5.6")
            .is_some_and(|suffix| suffix.starts_with('-'))
}

fn has_function_tools(request: &GaiseInstructRequest) -> bool {
    request
        .tools
        .as_ref()
        .is_some_and(|tools| !tools.is_empty())
}

fn chat_reasoning_effort(request: &GaiseInstructRequest) -> Option<String> {
    let rules = openai_chat_rules(&request.model);
    if has_function_tools(request) && chat_tools_require_none_reasoning(&request.model) {
        return Some("none".to_string());
    }

    request
        .generation_config
        .as_ref()
        .and_then(|config| config.thinking_effort.as_deref())
        .and_then(|effort| normalize_chat_effort(&rules, effort))
}

/// `(temperature, top_p)` after the family's sampling rules.
fn chat_sampling(request: &GaiseInstructRequest) -> (Option<f32>, Option<f32>) {
    let Some(config) = request.generation_config.as_ref() else {
        return (None, None);
    };
    let rules = openai_chat_rules(&request.model);
    if rules.sampling_never {
        return (None, None);
    }
    if rules.sampling_requires_none {
        let effective =
            chat_reasoning_effort(request).or_else(|| rules.default_effort.map(str::to_string));
        if effective.as_deref() != Some("none") {
            return (None, None);
        }
    }
    (config.temperature, config.top_p)
}

/// Image detail after the family rule for `original`.
fn chat_image_detail(request: &GaiseInstructRequest) -> Option<String> {
    let detail = request
        .generation_config
        .as_ref()
        .and_then(|config| config.input_image_detail.as_deref())?
        .to_ascii_lowercase();
    if detail == "original" && !openai_chat_rules(&request.model).original_image_detail {
        return Some("high".to_string());
    }
    Some(detail)
}

fn map_content_parts(content: GaiseContent, image_detail: Option<&str>) -> Vec<OpenAIContentPart> {
    match content {
        GaiseContent::Text { text } => vec![OpenAIContentPart::Text { text }],
        GaiseContent::Image { data, format } => {
            let encoded = base64::prelude::BASE64_STANDARD.encode(data);
            let media_type = image_media_type(format.as_deref());
            vec![OpenAIContentPart::ImageUrl {
                image_url: OpenAIImageUrl {
                    url: format!("data:{media_type};base64,{encoded}"),
                    detail: image_detail.map(str::to_ascii_lowercase),
                },
            }]
        }
        GaiseContent::Audio { data, format } => vec![OpenAIContentPart::InputAudio {
            input_audio: OpenAIInputAudio {
                data: base64::prelude::BASE64_STANDARD.encode(data),
                format: openai_audio_format(format.as_deref()),
            },
        }],
        GaiseContent::File { data, name } => {
            let name = name.unwrap_or_else(|| "document".to_string());
            let text = match String::from_utf8(data) {
                Ok(text) => {
                    format!("<attached_document name=\"{name}\">\n{text}\n</attached_document>")
                }
                Err(_) => format!(
                    "[Unsupported binary document for OpenAI Chat Completions; use the Responses API input_file feature: {name}]"
                ),
            };
            vec![OpenAIContentPart::Text { text }]
        }
        GaiseContent::Reasoning { text, .. } => vec![OpenAIContentPart::Text {
            text: format!("<reasoning_summary>\n{text}\n</reasoning_summary>"),
        }],
        GaiseContent::RedactedReasoning { .. } => vec![OpenAIContentPart::Text {
            text: "[Encrypted reasoning retained only on its source provider]".to_string(),
        }],
        GaiseContent::Parts { parts } => parts
            .into_iter()
            .flat_map(|part| map_content_parts(part, image_detail))
            .collect(),
    }
}

impl From<&GaiseInstructRequest> for OpenAIChatRequest {
    fn from(request: &GaiseInstructRequest) -> Self {
        let messages = match &request.input {
            OneOrMany::One(m) => vec![m.clone()],
            OneOrMany::Many(ms) => ms.clone(),
        };
        let image_detail = chat_image_detail(request);
        let image_detail = image_detail.as_deref();

        let openai_messages = messages
            .into_iter()
            .map(|m| {
                let content = m.content.map(|c| {
                    let items = match c {
                        OneOrMany::One(item) => vec![item],
                        OneOrMany::Many(items) => items,
                    };

                    let parts: Vec<OpenAIContentPart> = items
                        .into_iter()
                        .flat_map(|item| map_content_parts(item, image_detail))
                        .collect();

                    if parts.len() == 1
                        && let Some(OpenAIContentPart::Text { text }) = parts.first()
                    {
                        return OpenAIContent::Text(text.clone());
                    }
                    OpenAIContent::Parts(parts)
                });

                let tool_calls = m.tool_calls.map(|tcs| {
                    tcs.into_iter()
                        .map(|tc| OpenAIToolCall {
                            id: tc.id,
                            r#type: tc.r#type,
                            function: OpenAIFunctionCall {
                                name: tc.function.name,
                                arguments: tc.function.arguments.unwrap_or_default(),
                            },
                        })
                        .collect()
                });

                OpenAIMessage {
                    role: m.role,
                    content,
                    tool_calls,
                    tool_call_id: m.tool_call_id,
                }
            })
            .collect();

        OpenAIChatRequest {
            model: request.model.clone(),
            messages: openai_messages,
            stream: false,
            stream_options: None,
            temperature: chat_sampling(request).0,
            top_p: chat_sampling(request).1,
            max_completion_tokens: request
                .generation_config
                .as_ref()
                .and_then(|c| c.max_tokens),
            reasoning_effort: chat_reasoning_effort(request),
            prompt_cache_key: request
                .generation_config
                .as_ref()
                .and_then(|c| c.cache_key.clone()),
            // Defaulted here; the client stamps the resolved tier in `instruct`/`instruct_stream`,
            // which is the only place that has access to the env-sourced value.
            service_tier: None,
            tools: request
                .tools
                .as_ref()
                .map(|ts| ts.iter().map(|t| OpenAITool::from(t.clone())).collect()),
        }
    }
}

impl GaiseClientOpenAI {
    pub fn new(api_url: String, api_key: String) -> Self {
        // Resolve the processing tier once. An empty/whitespace value is treated as
        // unset so a blank `OPENAI_API_TIER=` doesn't send `service_tier: ""`.
        let service_tier = std::env::var("OPENAI_API_TIER")
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        Self {
            api_url,
            api_key,
            client: reqwest::Client::new(),
            service_tier,
            learned_tool_none_models: tokio::sync::RwLock::new(Default::default()),
        }
    }

    fn map_from_openai_message(&self, msg: OpenAIMessage) -> GaiseMessage {
        let content = msg.content.map(|c| match c {
            OpenAIContent::Text(text) => OneOrMany::One(GaiseContent::Text { text }),
            OpenAIContent::Parts(parts) => OneOrMany::Many(
                parts
                    .into_iter()
                    .filter_map(|p| match p {
                        OpenAIContentPart::Text { text } => Some(GaiseContent::Text { text }),
                        OpenAIContentPart::ImageUrl { .. } => {
                            // This is lossy as we don't easily get back raw bytes from URL here if it's external,
                            // but if it's data URI we could. For now, just placeholder or skip.
                            None
                        }
                        OpenAIContentPart::InputAudio { .. } => None,
                    })
                    .collect(),
            ),
        });

        let tool_calls = msg.tool_calls.map(|tcs| {
            tcs.into_iter()
                .map(|tc| GaiseToolCall {
                    id: tc.id,
                    r#type: tc.r#type,
                    function: GaiseFunctionCall {
                        name: tc.function.name,
                        arguments: Some(tc.function.arguments),
                    },
                    thought_signature: None,
                })
                .collect()
        });

        GaiseMessage {
            role: msg.role,
            content,
            tool_calls,
            tool_call_id: msg.tool_call_id,
            tool_name: None,
        }
    }
}

/// Total attempts (1 initial + retries) for a transient failure.
const MAX_ATTEMPTS: u32 = 4;

/// Transient HTTP statuses worth retrying: 429 (rate limit) and any 5xx
/// (OpenAI's `server_error`, plus gateway/timeout codes). Other 4xx are caller
/// errors and must not be retried.
fn is_transient_status(status: reqwest::StatusCode) -> bool {
    status.as_u16() == 429 || status.is_server_error()
}

/// Match only OpenAI's structured Chat Completions compatibility error. This
/// provides a forward-compatible fallback for new model aliases without
/// retrying unrelated 400 responses or hiding invalid tool schemas.
fn should_retry_chat_tools_with_none(
    status: reqwest::StatusCode,
    error_text: &str,
    request: &OpenAIChatRequest,
) -> bool {
    if status != reqwest::StatusCode::BAD_REQUEST
        || request.reasoning_effort.as_deref() == Some("none")
        || request.tools.as_ref().is_none_or(|tools| tools.is_empty())
    {
        return false;
    }

    let Ok(error) = serde_json::from_str::<serde_json::Value>(error_text) else {
        return false;
    };
    let detail = &error["error"];
    let Some(message) = detail["message"].as_str() else {
        return false;
    };
    let message = message.to_ascii_lowercase();

    detail["type"] == "invalid_request_error"
        && detail["param"] == "reasoning_effort"
        && message.contains("function tools")
        && message.contains("reasoning_effort")
        && message.contains("none")
}

impl GaiseClientOpenAI {
    /// Send `builder`, retrying transient failures — 429/5xx responses and
    /// network errors — with exponential backoff. OpenAI's 500 `server_error` is
    /// explicitly retryable and is common under the `flex` service tier; without
    /// this a single random 500 aborts the whole chat turn or enrich item. The
    /// last attempt's outcome is returned verbatim (success or not), so a genuine
    /// persistent failure still surfaces through the caller's existing handling.
    async fn send_with_retry(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, Box<dyn std::error::Error + Send + Sync>> {
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let outcome = match builder.try_clone() {
                Some(rb) => rb.send().await,
                // JSON bodies always clone; if one somehow can't, send the original
                // (no retry) rather than failing to send at all.
                None => return builder.send().await.map_err(Into::into),
            };
            let retryable = match &outcome {
                Ok(resp) => is_transient_status(resp.status()),
                Err(_) => true, // network/timeout — worth another attempt
            };
            if !retryable || attempt >= MAX_ATTEMPTS {
                return outcome.map_err(Into::into);
            }
            match outcome {
                // Drain the body to free the connection before retrying.
                Ok(resp) => {
                    let status = resp.status();
                    let _ = resp.bytes().await;
                    eprintln!(
                        "⚠️  OpenAI {status} — retrying ({attempt}/{})",
                        MAX_ATTEMPTS - 1
                    );
                }
                Err(_) => {
                    eprintln!(
                        "⚠️  OpenAI request error — retrying ({attempt}/{})",
                        MAX_ATTEMPTS - 1
                    );
                }
            }
            // Exponential backoff: 400ms, 800ms, 1600ms.
            let delay = std::time::Duration::from_millis(400u64 * (1u64 << (attempt - 1)));
            tokio::time::sleep(delay).await;
        }
    }

    fn chat_request_builder(
        &self,
        url: &str,
        request: &OpenAIChatRequest,
    ) -> reqwest::RequestBuilder {
        self.client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(request)
    }

    /// Send a Chat Completions request and retry once with reasoning disabled
    /// when OpenAI explicitly reports the function-tools compatibility error.
    async fn send_chat_with_reasoning_fallback(
        &self,
        url: &str,
        request: &mut OpenAIChatRequest,
    ) -> Result<reqwest::Response, Box<dyn std::error::Error + Send + Sync>> {
        self.apply_learned_tool_fallback(request).await;
        let response = self
            .send_with_retry(self.chat_request_builder(url, request))
            .await?;
        if response.status().is_success() {
            return Ok(response);
        }

        let status = response.status();
        let err_text = response.text().await?;
        if should_retry_chat_tools_with_none(status, &err_text, request) {
            if self.remember_tool_fallback(&request.model).await {
                eprintln!(
                    "OpenAI Chat Completions rejected function tools with reasoning_effort for {}; \
                     retrying with reasoning_effort=none and using none for later tool requests to this model",
                    request.model
                );
            }
            request.reasoning_effort = Some("none".to_string());
            let response = self
                .send_with_retry(self.chat_request_builder(url, request))
                .await?;
            if response.status().is_success() {
                return Ok(response);
            }
            let err_text = response.text().await?;
            return Err(format!("OpenAI API error: {err_text}").into());
        }

        Err(format!("OpenAI API error: {err_text}").into())
    }

    /// Apply a previously learned tools-require-`none` rule for this model.
    async fn apply_learned_tool_fallback(&self, request: &mut OpenAIChatRequest) {
        if request.reasoning_effort.as_deref() == Some("none")
            || request.tools.as_ref().is_none_or(|tools| tools.is_empty())
        {
            return;
        }
        if self
            .learned_tool_none_models
            .read()
            .await
            .contains(&request.model)
        {
            request.reasoning_effort = Some("none".to_string());
        }
    }

    /// Record that `model` needs `none` with tools. Returns `true` the first time.
    async fn remember_tool_fallback(&self, model: &str) -> bool {
        self.learned_tool_none_models
            .write()
            .await
            .insert(model.to_string())
    }
}

#[async_trait]
impl GaiseClient for GaiseClientOpenAI {
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
        ensure_chat_supported(&request.model)?;
        let url = format!("{}/chat/completions", self.api_url);

        let mut openai_request = OpenAIChatRequest::from(request);
        openai_request.stream = true;
        openai_request.stream_options = Some(OpenAIStreamOptions {
            include_usage: true,
        });
        openai_request.service_tier = self.service_tier.clone();

        let response = self
            .send_chat_with_reasoning_fallback(&url, &mut openai_request)
            .await?;

        let stream = response.bytes_stream();

        // `bytes_stream()` yields arbitrary network chunks, NOT line-aligned SSE events:
        // a single chunk may hold several `data:` lines, or split one line (and its JSON)
        // across reads. So buffer the raw bytes and only parse complete, `\n`-terminated
        // lines. This reassembles a large tool-call argument payload that arrives across
        // several reads, instead of parsing it truncated ("EOF while parsing a string") or
        // parsing two concatenated events as one ("trailing characters").
        let mapped_stream = stream
            .scan(Vec::<u8>::new(), |buf, res| {
                let events: Vec<
                    Result<GaiseInstructStreamResponse, Box<dyn std::error::Error + Send + Sync>>,
                > = match res {
                    Err(e) => vec![Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>)],
                    Ok(bytes) => {
                        buf.extend_from_slice(&bytes);
                        let mut out = Vec::new();
                        // Drain every complete line now in the buffer; leave any partial
                        // trailing line for the next chunk to complete.
                        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                            let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
                            let line = String::from_utf8_lossy(&line_bytes);
                            let line = line.trim();
                            let json_str = match line.strip_prefix("data:").map(str::trim_start) {
                                Some(j) => j,
                                None => continue, // blank line, SSE comment, or other field
                            };
                            if json_str == "[DONE]" {
                                continue; // end-of-stream marker — stream ends naturally
                            }
                            match serde_json::from_str::<OpenAIChatStreamResponse>(json_str) {
                                Ok(chunk) => {
                                    out.extend(map_stream_chunk(chunk).into_iter().map(Ok));
                                }
                                // A *complete* line that still won't parse is provider
                                // metadata we don't model; skip it rather than abort the
                                // whole stream.
                                Err(_) => continue,
                            }
                        }
                        out
                    }
                };
                futures_util::future::ready(Some(futures_util::stream::iter(events)))
            })
            .flatten();

        Ok(Box::pin(mapped_stream))
    }

    async fn instruct(
        &self,
        request: &GaiseInstructRequest,
    ) -> Result<GaiseInstructResponse, Box<dyn std::error::Error + Send + Sync>> {
        ensure_chat_supported(&request.model)?;
        let url = format!("{}/chat/completions", self.api_url);

        let mut openai_request = OpenAIChatRequest::from(request);
        openai_request.service_tier = self.service_tier.clone();

        let response = self
            .send_chat_with_reasoning_fallback(&url, &mut openai_request)
            .await?;

        let openai_response: OpenAIChatResponse = response.json().await?;

        let usage = openai_response.usage.as_ref().map(map_usage);

        Ok(GaiseInstructResponse {
            output: OneOrMany::Many(
                openai_response
                    .choices
                    .into_iter()
                    .map(|c| self.map_from_openai_message(c.message))
                    .collect(),
            ),
            external_id: Some(openai_response.id),
            usage,
        })
    }

    async fn embeddings(
        &self,
        request: &GaiseEmbeddingsRequest,
    ) -> Result<GaiseEmbeddingsResponse, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/embeddings", self.api_url);
        let (openai_request, resolved) = openai_embed_request(request);

        let builder = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&openai_request);
        let response = self.send_with_retry(builder).await?;

        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format!("OpenAI API error: {}", err_text).into());
        }

        // Parse from the raw body so a schema/shape mismatch reports the offending
        // field and a body snippet, instead of reqwest's opaque "error decoding
        // response body". (The body is embedding vectors, not secrets.)
        let body = response.text().await?;
        let openai_response: OpenAIEmbedResponse = serde_json::from_str(&body).map_err(|e| {
            let snippet: String = body.chars().take(400).collect();
            format!("failed to parse OpenAI embeddings response: {e}; body starts: {snippet}")
        })?;

        let mut input_usage = HashMap::new();
        input_usage.insert(
            "prompt_tokens".to_string(),
            openai_response.usage.prompt_tokens,
        );

        let mut output: Vec<Vec<f32>> = openai_response
            .data
            .into_iter()
            .map(|d| d.embedding)
            .collect();
        if resolved.normalize_locally {
            output.iter_mut().for_each(|v| normalize_l2(v));
        }
        Ok(GaiseEmbeddingsResponse {
            external_id: Some(openai_response.object),
            output,
            usage: Some(GaiseUsage {
                input: Some(input_usage),
                output: None,
                total: Some(HashMap::from([(
                    "total_tokens".to_string(),
                    openai_response.usage.total_tokens,
                )])),
            }),
        })
    }

    async fn list_models(
        &self,
        request: &GaiseListModelsRequest,
    ) -> Result<GaiseListModelsResponse, Box<dyn std::error::Error + Send + Sync>> {
        let list = self.list_models_raw().await?;
        let models = list
            .data
            .iter()
            .map(|m| map_openai_model(m, request.include_raw))
            .collect();
        let mut response = GaiseListModelsResponse::from_models(models);
        response.retain_operation(request.operation);
        Ok(response)
    }
}

impl GaiseClientOpenAI {
    /// `GET /v1/models`. OpenAI reports only identity and lifecycle; the
    /// mapped capabilities are name heuristics (see `contracts::catalog`).
    pub async fn list_models_raw(
        &self,
    ) -> Result<OpenAIModelList, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/models", self.api_url);
        let builder = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", self.api_key));
        let response = self.send_with_retry(builder).await?;
        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format!("OpenAI API error: {}", err_text).into());
        }
        let body = response.text().await?;
        serde_json::from_str(&body).map_err(|e| {
            let snippet: String = body.chars().take(400).collect();
            format!("failed to parse OpenAI models response: {e}; body starts: {snippet}").into()
        })
    }
}

#[cfg(test)]
mod retry_tests {
    use super::{
        GaiseClientOpenAI, is_transient_status, map_stream_chunk, map_usage,
        should_retry_chat_tools_with_none,
    };
    use crate::contracts::{OpenAIChatRequest, OpenAIChatStreamResponse, OpenAIUsage};
    use gaise_core::contracts::GaiseStreamChunk;
    use reqwest::StatusCode;

    #[tokio::test]
    async fn learned_tool_fallback_is_scoped_to_model_and_tool_requests() {
        let client = GaiseClientOpenAI::new("http://unused.invalid".into(), "k".into());
        let tool_request = |model: &str, effort: &str| -> OpenAIChatRequest {
            serde_json::from_value(serde_json::json!({
                "model": model,
                "messages": [],
                "tools": [{"type": "function", "function": {"name": "inspect", "parameters": {"type": "object", "properties": {}, "required": []}}}],
                "reasoning_effort": effort,
                "stream": false
            }))
            .unwrap()
        };
        assert!(
            client.remember_tool_fallback("gpt-future").await,
            "first time is new"
        );
        assert!(!client.remember_tool_fallback("gpt-future").await);

        let mut same = tool_request("gpt-future", "high");
        client.apply_learned_tool_fallback(&mut same).await;
        assert_eq!(same.reasoning_effort.as_deref(), Some("none"));

        let mut other = tool_request("gpt-other", "high");
        client.apply_learned_tool_fallback(&mut other).await;
        assert_eq!(
            other.reasoning_effort.as_deref(),
            Some("high"),
            "other models untouched"
        );

        let mut no_tools = tool_request("gpt-future", "high");
        no_tools.tools = None;
        client.apply_learned_tool_fallback(&mut no_tools).await;
        assert_eq!(
            no_tools.reasoning_effort.as_deref(),
            Some("high"),
            "only tool requests are affected"
        );
    }

    #[test]
    fn retries_429_and_5xx_only() {
        // OpenAI's intermittent 500 (and gateway/timeout 5xx) and 429 are retried.
        assert!(is_transient_status(StatusCode::INTERNAL_SERVER_ERROR)); // 500
        assert!(is_transient_status(StatusCode::BAD_GATEWAY)); // 502
        assert!(is_transient_status(StatusCode::SERVICE_UNAVAILABLE)); // 503
        assert!(is_transient_status(StatusCode::GATEWAY_TIMEOUT)); // 504
        assert!(is_transient_status(StatusCode::TOO_MANY_REQUESTS)); // 429
        // Caller errors and success are never retried.
        assert!(!is_transient_status(StatusCode::BAD_REQUEST)); // 400
        assert!(!is_transient_status(StatusCode::UNAUTHORIZED)); // 401
        assert!(!is_transient_status(StatusCode::OK)); // 200
    }

    #[test]
    fn retries_only_the_structured_function_tool_reasoning_error() {
        let mut request: OpenAIChatRequest = serde_json::from_value(serde_json::json!({
            "model": "gpt-future",
            "messages": [{"role": "user", "content": "Call ping"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "ping",
                    "parameters": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                }
            }],
            "reasoning_effort": "high",
            "stream": false
        }))
        .unwrap();
        let compatibility_error = serde_json::json!({
            "error": {
                "message": "Function tools with reasoning_effort are not supported for gpt-future in /v1/chat/completions. To use function tools, use /v1/responses or set reasoning_effort to 'none'.",
                "type": "invalid_request_error",
                "param": "reasoning_effort",
                "code": null
            }
        })
        .to_string();

        assert!(should_retry_chat_tools_with_none(
            StatusCode::BAD_REQUEST,
            &compatibility_error,
            &request
        ));
        assert!(!should_retry_chat_tools_with_none(
            StatusCode::UNPROCESSABLE_ENTITY,
            &compatibility_error,
            &request
        ));

        request.reasoning_effort = Some("none".to_string());
        assert!(!should_retry_chat_tools_with_none(
            StatusCode::BAD_REQUEST,
            &compatibility_error,
            &request
        ));

        request.reasoning_effort = Some("high".to_string());
        let unrelated_error = serde_json::json!({
            "error": {
                "message": "Invalid function schema.",
                "type": "invalid_request_error",
                "param": "tools",
                "code": null
            }
        })
        .to_string();
        assert!(!should_retry_chat_tools_with_none(
            StatusCode::BAD_REQUEST,
            &unrelated_error,
            &request
        ));
    }

    #[test]
    fn maps_every_parallel_tool_delta_without_http() {
        let chunk: OpenAIChatStreamResponse = serde_json::from_value(serde_json::json!({
            "id": "response-1",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "gpt-5.6",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": null,
                    "content": null,
                    "tool_calls": [
                        {
                            "index": 0,
                            "id": "call-1",
                            "type": "function",
                            "function": {"name": "first", "arguments": "{}"}
                        },
                        {
                            "index": 1,
                            "id": "call-2",
                            "type": "function",
                            "function": {"name": "second", "arguments": "{}"}
                        }
                    ]
                },
                "finish_reason": null
            }]
        }))
        .unwrap();

        let events = map_stream_chunk(chunk);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0].chunk,
            GaiseStreamChunk::ToolCall { index: 0, name, .. }
                if name.as_deref() == Some("first")
        ));
        assert!(matches!(
            &events[1].chunk,
            GaiseStreamChunk::ToolCall { index: 1, name, .. }
                if name.as_deref() == Some("second")
        ));
    }

    #[test]
    fn maps_chat_usage_to_the_correct_side_without_inventing_modalities() {
        let usage: OpenAIUsage = serde_json::from_value(serde_json::json!({
            "prompt_tokens": 50,
            "completion_tokens": 100,
            "total_tokens": 150,
            "prompt_tokens_details": {
                "audio_tokens": 7,
                "cached_tokens": 10,
                "cache_write_tokens": 4
            },
            "completion_tokens_details": {
                "audio_tokens": 30,
                "reasoning_tokens": 20,
                "accepted_prediction_tokens": 3,
                "rejected_prediction_tokens": 2
            }
        }))
        .unwrap();

        let mapped = map_usage(&usage);
        let input = mapped.input.unwrap();
        let output = mapped.output.unwrap();
        assert_eq!(input.get("prompt_tokens"), Some(&50));
        assert_eq!(input.get("audio_tokens"), Some(&7));
        assert_eq!(input.get("cached_tokens"), Some(&10));
        assert_eq!(input.get("cache_write_tokens"), Some(&4));
        assert!(!input.contains_key("text_tokens"));
        assert!(!input.contains_key("image_tokens"));
        assert_eq!(output.get("completion_tokens"), Some(&100));
        assert_eq!(output.get("audio_tokens"), Some(&30));
        assert_eq!(output.get("reasoning_tokens"), Some(&20));
        assert!(!output.contains_key("total_tokens"));
        assert_eq!(mapped.total.unwrap().get("total_tokens"), Some(&150));
    }
}
