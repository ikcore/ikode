use crate::contracts::*;
use async_trait::async_trait;
use base64::Engine;
use futures_util::{Stream, StreamExt};
use gaise_core::GaiseClient;
use gaise_core::contracts::{
    EmbeddingTaskControl, GaiseContent, GaiseEmbeddingsRequest, GaiseEmbeddingsResponse,
    GaiseFunctionCall, GaiseInstructRequest, GaiseInstructResponse, GaiseInstructStreamResponse,
    GaiseListModelsRequest, GaiseListModelsResponse, GaiseMessage, GaiseModel,
    GaiseReasoningEffort, GaiseStreamChunk, GaiseTool, GaiseToolCall, GaiseUsage, OneOrMany,
    ResolvedEmbedding, normalize_l2, resolve_embedding,
};
use std::collections::HashMap;
use std::pin::Pin;

pub struct GaiseClientOllama {
    api_url: String,
    client: reqwest::Client,
}

impl From<GaiseTool> for OllamaTool {
    fn from(t: GaiseTool) -> Self {
        fn map_param(p: &gaise_core::contracts::GaiseToolParameter) -> OllamaParameterProperty {
            let mut prop_type = p.r#type.clone().unwrap_or_else(|| "string".to_string());
            if prop_type == "text" {
                prop_type = "string".to_string();
            }
            OllamaParameterProperty {
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

        OllamaTool {
            r#type: "function".to_string(),
            function: OllamaFunction {
                name: t.name,
                description: t.description.unwrap_or_default(),
                parameters: OllamaParameters {
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

fn append_content(
    content: &mut String,
    thinking: &mut String,
    images: &mut Vec<String>,
    item: GaiseContent,
) {
    match item {
        GaiseContent::Text { text } => content.push_str(&text),
        GaiseContent::Image { data, .. } => {
            images.push(base64::prelude::BASE64_STANDARD.encode(data));
        }
        GaiseContent::File { data, name } => {
            let name = name.unwrap_or_else(|| "document".to_string());
            if let Ok(text) = String::from_utf8(data) {
                content.push_str(&format!(
                    "\n<attached_document name=\"{name}\">\n{text}\n</attached_document>"
                ));
            } else {
                // Ollama's chat schema has no binary-document field. Keep the
                // limitation explicit rather than silently dropping the block.
                content.push_str(&format!(
                    "\n[Unsupported binary document for Ollama chat API: {name}]"
                ));
            }
        }
        GaiseContent::Parts { parts } => {
            for item in parts {
                append_content(content, thinking, images, item);
            }
        }
        GaiseContent::Reasoning { text, .. } => thinking.push_str(&text),
        GaiseContent::RedactedReasoning { .. } => {
            content.push_str("[Encrypted reasoning retained only on its source provider]")
        }
        GaiseContent::Audio { format, .. } => content.push_str(&format!(
            "[Unsupported audio input for Ollama chat API: {}]",
            format.unwrap_or_else(|| "unknown".to_string())
        )),
    }
}

impl From<&GaiseInstructRequest> for OllamaChatRequest {
    fn from(request: &GaiseInstructRequest) -> Self {
        let messages = match &request.input {
            OneOrMany::One(m) => vec![m.clone()],
            OneOrMany::Many(ms) => ms.clone(),
        };

        let ollama_messages = messages
            .into_iter()
            .map(|m| {
                let mut content = String::new();
                let mut thinking = String::new();
                let mut images = Vec::new();

                if let Some(c) = m.content {
                    let contents = match c {
                        OneOrMany::One(item) => vec![item],
                        OneOrMany::Many(items) => items,
                    };

                    for item in contents {
                        append_content(&mut content, &mut thinking, &mut images, item);
                    }
                }

                let tool_calls = m.tool_calls.map(|tcs| {
                    tcs.into_iter()
                        .map(|tc| {
                            let arguments: HashMap<String, serde_json::Value> = tc
                                .function
                                .arguments
                                .and_then(|args| {
                                    if args.trim().starts_with('{') {
                                        serde_json::from_str(&args).ok()
                                    } else {
                                        // If it's not a JSON object, maybe it's just a string or empty
                                        None
                                    }
                                })
                                .unwrap_or_default();
                            OllamaToolCall {
                                function: OllamaFunctionCall {
                                    name: tc.function.name,
                                    arguments,
                                },
                            }
                        })
                        .collect()
                });

                OllamaMessage {
                    role: m.role,
                    content: if content.is_empty() {
                        None
                    } else {
                        Some(content)
                    },
                    images: if images.is_empty() {
                        None
                    } else {
                        Some(images)
                    },
                    tool_calls,
                    thinking: (!thinking.is_empty()).then_some(thinking),
                }
            })
            .collect();

        OllamaChatRequest {
            model: request.model.clone(),
            messages: ollama_messages,
            stream: false,
            options: request.generation_config.as_ref().map(|c| OllamaOptions {
                temperature: c.temperature,
                top_k: c.top_k,
                top_p: c.top_p,
                num_predict: c.max_tokens,
            }),
            tools: request
                .tools
                .as_ref()
                .map(|ts| ts.iter().map(|t| OllamaTool::from(t.clone())).collect()),
            format: None,
            think: request
                .generation_config
                .as_ref()
                .and_then(|config| ollama_think(&request.model, config)),
        }
    }
}

/// Map the canonical effort onto Ollama's `think` field. Most models take a
/// boolean; GPT-OSS accepts `low`/`medium`/`high` (`minimal` → low,
/// `xhigh`/`max`/`ultra` → high, `auto` → `true`, custom strings forwarded).
/// Build the `/api/embed` body through the shared embedding rules
/// (`model-registry.toml` profiles). Each local family's documented prefix
/// convention is applied from `task` (nomic `search_query: `, mxbai/Arctic
/// query instructions, EmbeddingGemma's `task: … | query: …`), `dimensions`
/// is snapped for Matryoshka tags and dropped for fixed-size ones, and
/// inputs longer than the context are truncated instead of erroring. Unknown
/// tags pass text and `dimensions` through untouched.
pub fn ollama_embed_request(
    request: &GaiseEmbeddingsRequest,
) -> (OllamaEmbedRequest, ResolvedEmbedding) {
    let profile = gaise_core::registry::embedding_profile("ollama", &request.model);
    let resolved = resolve_embedding(request, profile, &EmbeddingTaskControl::None);
    (
        OllamaEmbedRequest {
            model: request.model.clone(),
            input: resolved.texts.clone(),
            options: None,
            dimensions: resolved.dimensions,
            truncate: Some(true),
        },
        resolved,
    )
}

pub fn ollama_think(
    model: &str,
    config: &gaise_core::contracts::GaiseGenerationConfig,
) -> Option<OllamaThink> {
    const GPT_OSS_LEVELS: &[&str] = &["low", "medium", "high"];
    let gpt_oss = model.to_ascii_lowercase().contains("gpt-oss");
    if let Some(effort) = config.reasoning_effort() {
        return Some(match effort {
            GaiseReasoningEffort::None => OllamaThink::Enabled(false),
            GaiseReasoningEffort::Auto => OllamaThink::Enabled(true),
            GaiseReasoningEffort::Custom(raw) if gpt_oss => OllamaThink::Level(raw),
            GaiseReasoningEffort::Custom(_) => OllamaThink::Enabled(true),
            level if gpt_oss => OllamaThink::Level(
                level
                    .clamp_to(&GaiseReasoningEffort::levels(GPT_OSS_LEVELS))
                    .as_str()
                    .to_string(),
            ),
            _ => OllamaThink::Enabled(true),
        });
    }
    config.thinking_tokens.map(|tokens| {
        if tokens == 0 {
            OllamaThink::Enabled(false)
        } else if gpt_oss {
            OllamaThink::Level("medium".into())
        } else {
            OllamaThink::Enabled(true)
        }
    })
}

fn format_ollama_error(err_text: &str) -> String {
    if err_text.contains("error parsing tool call") {
        format!(
            "Ollama failed to parse the model's tool call output. \
            This usually means the model does not support tool calling. \
            Choose an installed tag that advertises tool support in Ollama's model metadata.\n\
            Raw error: {}",
            err_text
        )
    } else {
        format!("Ollama API error: {}", err_text)
    }
}

fn map_ollama_chat_usage(
    prompt_tokens: Option<usize>,
    completion_tokens: Option<usize>,
) -> Option<GaiseUsage> {
    let input = prompt_tokens.map(|tokens| HashMap::from([("prompt_tokens".to_string(), tokens)]));
    let output =
        completion_tokens.map(|tokens| HashMap::from([("completion_tokens".to_string(), tokens)]));
    let total = prompt_tokens
        .zip(completion_tokens)
        .and_then(|(prompt, completion)| prompt.checked_add(completion))
        .map(|tokens| HashMap::from([("total_tokens".to_string(), tokens)]));

    (input.is_some() || output.is_some()).then_some(GaiseUsage {
        input,
        output,
        total,
    })
}

fn map_ollama_embedding_usage(prompt_tokens: Option<usize>) -> Option<GaiseUsage> {
    prompt_tokens.map(|tokens| GaiseUsage {
        input: Some(HashMap::from([("prompt_tokens".to_string(), tokens)])),
        output: None,
        total: Some(HashMap::from([("total_tokens".to_string(), tokens)])),
    })
}

fn map_stream_response(chunk: OllamaChatResponse) -> Vec<GaiseInstructStreamResponse> {
    let mut events = Vec::new();

    if let Some(thinking) = chunk.message.thinking.filter(|value| !value.is_empty()) {
        events.push(GaiseInstructStreamResponse {
            chunk: GaiseStreamChunk::Content(GaiseContent::Reasoning {
                text: thinking,
                signature: None,
            }),
            external_id: None,
        });
    }
    if let Some(text) = chunk.message.content.filter(|value| !value.is_empty()) {
        events.push(GaiseInstructStreamResponse {
            chunk: GaiseStreamChunk::Text(text),
            external_id: None,
        });
    }
    if let Some(images) = chunk.message.images {
        for image in images {
            if let Ok(data) = base64::prelude::BASE64_STANDARD.decode(image) {
                events.push(GaiseInstructStreamResponse {
                    chunk: GaiseStreamChunk::Content(GaiseContent::Image { data, format: None }),
                    external_id: None,
                });
            }
        }
    }
    if let Some(tool_calls) = chunk.message.tool_calls {
        for (index, tool_call) in tool_calls.into_iter().enumerate() {
            events.push(GaiseInstructStreamResponse {
                chunk: GaiseStreamChunk::ToolCall {
                    index,
                    id: None,
                    name: Some(tool_call.function.name),
                    arguments: Some(
                        serde_json::to_string(&tool_call.function.arguments).unwrap_or_default(),
                    ),
                    thought_signature: None,
                },
                external_id: None,
            });
        }
    }
    if chunk.done
        && let Some(usage) = map_ollama_chat_usage(chunk.prompt_eval_count, chunk.eval_count)
    {
        events.push(GaiseInstructStreamResponse {
            chunk: GaiseStreamChunk::Usage(usage),
            external_id: None,
        });
    }

    events
}

impl GaiseClientOllama {
    pub fn new(api_url: String) -> Self {
        Self {
            api_url,
            client: reqwest::Client::new(),
        }
    }

    /// `GET /api/tags` — the locally installed catalog.
    pub async fn list_tags(
        &self,
    ) -> Result<OllamaTagList, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/tags", self.api_url);
        let response = self.client.get(url).send().await?;
        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format_ollama_error(&err_text).into());
        }
        let body = response.text().await?;
        serde_json::from_str(&body).map_err(|e| {
            let snippet: String = body.chars().take(400).collect();
            format!("failed to parse Ollama tags response: {e}; body starts: {snippet}").into()
        })
    }

    /// `POST /api/show` — capabilities and model info for one tag.
    pub async fn show_model(
        &self,
        model: &str,
    ) -> Result<OllamaShowResponse, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/show", self.api_url);
        let response = self
            .client
            .post(url)
            .json(&OllamaShowRequest {
                model: model.to_string(),
            })
            .send()
            .await?;
        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format_ollama_error(&err_text).into());
        }
        let body = response.text().await?;
        serde_json::from_str(&body).map_err(|e| {
            let snippet: String = body.chars().take(400).collect();
            format!("failed to parse Ollama show response: {e}; body starts: {snippet}").into()
        })
    }

    fn map_from_ollama_message(&self, msg: OllamaMessage) -> GaiseMessage {
        let tool_calls = msg.tool_calls.map(|tcs| {
            tcs.into_iter()
                .map(|tc| {
                    GaiseToolCall {
                        id: String::new(), // Ollama doesn't seem to provide IDs for tool calls in this format
                        r#type: "function".to_string(),
                        function: GaiseFunctionCall {
                            name: tc.function.name,
                            arguments: Some(
                                serde_json::to_string(&tc.function.arguments).unwrap_or_default(),
                            ),
                        },
                        thought_signature: None,
                    }
                })
                .collect()
        });

        let mut content = Vec::new();
        if let Some(thinking) = msg.thinking.filter(|value| !value.is_empty()) {
            content.push(GaiseContent::Reasoning {
                text: thinking,
                signature: None,
            });
        }
        if let Some(text) = msg.content.filter(|value| !value.is_empty()) {
            content.push(GaiseContent::Text { text });
        }
        if let Some(images) = msg.images {
            for image in images {
                if let Ok(data) = base64::prelude::BASE64_STANDARD.decode(image) {
                    content.push(GaiseContent::Image { data, format: None });
                }
            }
        }

        GaiseMessage {
            role: msg.role,
            content: match content.len() {
                0 => None,
                1 => Some(OneOrMany::One(content.remove(0))),
                _ => Some(OneOrMany::Many(content)),
            },
            tool_calls,
            tool_call_id: None,
            tool_name: None,
        }
    }
}

#[async_trait]
impl GaiseClient for GaiseClientOllama {
    async fn list_models(
        &self,
        request: &GaiseListModelsRequest,
    ) -> Result<GaiseListModelsResponse, Box<dyn std::error::Error + Send + Sync>> {
        let tags = self.list_tags().await?;
        let mut models: Vec<GaiseModel> = tags
            .models
            .iter()
            .map(|t| map_ollama_tag(t, request.include_raw))
            .collect();
        let mut errors = Vec::new();

        if request.include_details {
            // `/api/show` per tag, a few at a time so a large local catalog
            // does not hammer the daemon.
            const CONCURRENCY: usize = 4;
            let include_raw = request.include_raw;
            let detailed = futures_util::stream::iter(models)
                .map(|mut model| async move {
                    match self.show_model(&model.id).await {
                        Ok(show) => {
                            apply_ollama_show(&mut model, &show, include_raw);
                            (model, None)
                        }
                        Err(e) => (
                            model.clone(),
                            Some(gaise_core::contracts::GaiseProviderError {
                                provider: "ollama".to_string(),
                                message: format!("{}: {e}", model.id),
                            }),
                        ),
                    }
                })
                .buffered(CONCURRENCY)
                .collect::<Vec<_>>()
                .await;
            models = Vec::with_capacity(detailed.len());
            for (model, error) in detailed {
                models.push(model);
                if let Some(error) = error {
                    errors.push(error);
                }
            }
        }

        let mut response = GaiseListModelsResponse { models, errors };
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
        let url = format!("{}/api/chat", self.api_url);

        let mut ollama_request = OllamaChatRequest::from(request);
        ollama_request.stream = true;

        let response = self.client.post(url).json(&ollama_request).send().await?;

        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format_ollama_error(&err_text).into());
        }

        let stream = response.bytes_stream();

        // Ollama streams newline-delimited JSON, but HTTP chunks can split or
        // combine lines. Buffer until a complete JSON object is available.
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
                            if line.is_empty() {
                                continue;
                            }
                            match serde_json::from_str::<OllamaChatResponse>(line) {
                                Ok(chunk) => {
                                    events.extend(map_stream_response(chunk).into_iter().map(Ok));
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
        let url = format!("{}/api/chat", self.api_url);

        let ollama_request = OllamaChatRequest::from(request);

        let response = self.client.post(url).json(&ollama_request).send().await?;

        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format_ollama_error(&err_text).into());
        }

        let ollama_response: OllamaChatResponse = response.json().await?;

        let usage = map_ollama_chat_usage(
            ollama_response.prompt_eval_count,
            ollama_response.eval_count,
        );

        Ok(GaiseInstructResponse {
            output: OneOrMany::One(self.map_from_ollama_message(ollama_response.message)),
            external_id: None,
            usage,
        })
    }

    async fn embeddings(
        &self,
        request: &GaiseEmbeddingsRequest,
    ) -> Result<GaiseEmbeddingsResponse, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/embed", self.api_url);
        let (ollama_request, resolved) = ollama_embed_request(request);

        let response = self.client.post(url).json(&ollama_request).send().await?;

        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format_ollama_error(&err_text).into());
        }

        let ollama_response: OllamaEmbedResponse = response.json().await?;

        let usage = map_ollama_embedding_usage(ollama_response.prompt_eval_count);

        Ok(GaiseEmbeddingsResponse {
            external_id: None,
            output: {
                let mut output = ollama_response.embeddings;
                if resolved.normalize_locally {
                    output.iter_mut().for_each(|v| normalize_l2(v));
                }
                output
            },
            usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_ollama_usage_totals_without_inventing_modality_breakdowns() {
        let response: OllamaChatResponse = serde_json::from_value(serde_json::json!({
            "model": "qwen3",
            "created_at": "2026-01-01T00:00:00Z",
            "message": {"role": "assistant", "content": "done"},
            "done": true,
            "prompt_eval_count": 12,
            "eval_count": 8
        }))
        .unwrap();
        let events = map_stream_response(response);
        let GaiseStreamChunk::Usage(usage) = &events.last().unwrap().chunk else {
            panic!("expected usage event");
        };
        let input = usage.input.as_ref().unwrap();
        assert_eq!(input.get("prompt_tokens"), Some(&12));
        assert!(!input.contains_key("image_tokens"));
        assert!(!input.contains_key("audio_tokens"));
        assert_eq!(
            usage.output.as_ref().unwrap().get("completion_tokens"),
            Some(&8)
        );
        assert_eq!(usage.total.as_ref().unwrap().get("total_tokens"), Some(&20));
        assert!(map_ollama_chat_usage(None, None).is_none());
        assert!(map_ollama_embedding_usage(None).is_none());
    }
}
