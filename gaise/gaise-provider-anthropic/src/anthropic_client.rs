use async_trait::async_trait;
use gaise_core::GaiseClient;
use gaise_core::contracts::{
    GaiseContent, GaiseEmbeddingsRequest, GaiseEmbeddingsResponse, GaiseInstructRequest,
    GaiseInstructResponse, GaiseInstructStreamResponse, GaiseMessage, GaiseStreamChunk,
    GaiseUsage, OneOrMany, GaiseToolCall, GaiseFunctionCall, GaiseTool, GaiseToolParameter
};
use crate::contracts::*;
use futures_util::{Stream, StreamExt};
use std::collections::HashMap;
use std::pin::Pin;
use base64::Engine;

pub struct GaiseClientAnthropic {
    api_url: String,
    api_key: String,
    api_version: String,
    client: reqwest::Client,
}

impl From<GaiseTool> for AnthropicTool {
    fn from(t: GaiseTool) -> Self {
        fn map_param(p: &GaiseToolParameter) -> AnthropicProperty {
            let mut prop_type = p.r#type.clone().unwrap_or_else(|| "string".to_string());
            if prop_type == "text" {
                prop_type = "string".to_string();
            }
            AnthropicProperty {
                r#type: prop_type,
                description: p.description.clone().unwrap_or_default(),
                items: p.items.as_ref().map(|i| Box::new(map_param(i))),
            }
        }

        AnthropicTool {
            name: t.name,
            description: t.description,
            input_schema: AnthropicInputSchema {
                r#type: "object".to_string(),
                properties: t.parameters.as_ref().and_then(|p| p.properties.as_ref()).map(|props| {
                    props.iter().map(|(k, v)| {
                        (k.clone(), map_param(v))
                    }).collect()
                }).unwrap_or_default(),
                required: t.parameters.as_ref().and_then(|p| p.required.clone()).unwrap_or_default(),
            }
        }
    }
}

impl From<&GaiseInstructRequest> for AnthropicRequest {
    fn from(request: &GaiseInstructRequest) -> Self {
        let messages = match &request.input {
            OneOrMany::One(m) => vec![m.clone()],
            OneOrMany::Many(ms) => ms.clone(),
        };

        let mut system_prompt = None;
        let mut anthropic_messages = Vec::new();

        for m in messages {
            // Extract system message
            if m.role == "system" {
                if let Some(content) = &m.content {
                    match content {
                        OneOrMany::One(GaiseContent::Text { text }) => {
                            system_prompt = Some(text.clone());
                        }
                        _ => {}
                    }
                }
                continue;
            }

            let content = match &m.content {
                Some(c) => {
                    let items = match c {
                        OneOrMany::One(item) => vec![item.clone()],
                        OneOrMany::Many(items) => items.clone(),
                    };

                    let blocks: Vec<AnthropicContentBlock> = items.into_iter().filter_map(|item| {
                        match item {
                            GaiseContent::Text { text } => Some(AnthropicContentBlock::Text { text }),
                            GaiseContent::Image { data, format } => {
                                let base64_data = base64::prelude::BASE64_STANDARD.encode(&data);
                                let media_type = format.unwrap_or_else(|| "image/jpeg".to_string());
                                Some(AnthropicContentBlock::Image {
                                    source: AnthropicImageSource {
                                        r#type: "base64".to_string(),
                                        media_type,
                                        data: base64_data,
                                    }
                                })
                            }
                            _ => None
                        }
                    }).collect();

                    if blocks.len() == 1 && matches!(blocks.first(), Some(AnthropicContentBlock::Text { .. })) {
                        if let Some(AnthropicContentBlock::Text { text }) = blocks.first() {
                            AnthropicContent::Text(text.clone())
                        } else {
                            AnthropicContent::Blocks(blocks)
                        }
                    } else {
                        AnthropicContent::Blocks(blocks)
                    }
                }
                None => AnthropicContent::Text(String::new()),
            };

            // Handle tool calls - convert to tool_use blocks
            let final_content = if let Some(tool_calls) = &m.tool_calls {
                let mut blocks = match content {
                    AnthropicContent::Text(t) => vec![AnthropicContentBlock::Text { text: t }],
                    AnthropicContent::Blocks(b) => b,
                };

                for tc in tool_calls {
                    let input: serde_json::Value = if let Some(args) = &tc.function.arguments {
                        serde_json::from_str(args).unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
                    } else {
                        serde_json::Value::Object(serde_json::Map::new())
                    };

                    blocks.push(AnthropicContentBlock::ToolUse {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        input,
                    });
                }
                AnthropicContent::Blocks(blocks)
            } else if let Some(tool_call_id) = &m.tool_call_id {
                // This is a tool result message
                let text = match content {
                    AnthropicContent::Text(t) => t,
                    AnthropicContent::Blocks(blocks) => {
                        blocks.into_iter()
                            .filter_map(|b| match b {
                                AnthropicContentBlock::Text { text } => Some(text),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                };
                AnthropicContent::Blocks(vec![AnthropicContentBlock::ToolResult {
                    tool_use_id: tool_call_id.clone(),
                    content: text,
                }])
            } else {
                content
            };

            anthropic_messages.push(AnthropicMessage {
                role: m.role.clone(),
                content: final_content,
            });
        }

        AnthropicRequest {
            model: request.model.clone(),
            messages: anthropic_messages,
            max_tokens: request.generation_config.as_ref()
                .and_then(|c| c.max_tokens)
                .unwrap_or(4096),
            system: system_prompt,
            temperature: request.generation_config.as_ref().and_then(|c| c.temperature),
            top_p: request.generation_config.as_ref().and_then(|c| c.top_p),
            tools: request.tools.as_ref().map(|ts| ts.iter().map(|t| AnthropicTool::from(t.clone())).collect()),
            stream: Some(false),
        }
    }
}

impl GaiseClientAnthropic {
    pub fn new(api_url: String, api_key: String) -> Self {
        Self {
            api_url,
            api_key,
            api_version: "2023-06-01".to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub fn with_version(mut self, version: String) -> Self {
        self.api_version = version;
        self
    }

    fn map_from_anthropic_content(&self, content: Vec<AnthropicContentBlock>) -> (Option<OneOrMany<GaiseContent>>, Option<Vec<GaiseToolCall>>) {
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for block in content {
            match block {
                AnthropicContentBlock::Text { text } => {
                    text_parts.push(GaiseContent::Text { text });
                }
                AnthropicContentBlock::ToolUse { id, name, input } => {
                    tool_calls.push(GaiseToolCall {
                        id,
                        r#type: "function".to_string(),
                        function: GaiseFunctionCall {
                            name,
                            arguments: Some(input.to_string()),
                        }
                    });
                }
                _ => {}
            }
        }

        let content_result = if !text_parts.is_empty() {
            if text_parts.len() == 1 {
                Some(OneOrMany::One(text_parts.into_iter().next().unwrap()))
            } else {
                Some(OneOrMany::Many(text_parts))
            }
        } else {
            None
        };

        let tool_calls_result = if !tool_calls.is_empty() {
            Some(tool_calls)
        } else {
            None
        };

        (content_result, tool_calls_result)
    }
}

#[async_trait]
impl GaiseClient for GaiseClientAnthropic {
    async fn instruct_stream(
        &self,
        request: &GaiseInstructRequest,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<GaiseInstructStreamResponse, Box<dyn std::error::Error + Send + Sync>>> + Send>>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let url = format!("{}/messages", self.api_url);

        let mut anthropic_request = AnthropicRequest::from(request);
        anthropic_request.stream = Some(true);

        let response = self.client.post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", &self.api_version)
            .header("content-type", "application/json")
            .json(&anthropic_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format!("Anthropic API error: {}", err_text).into());
        }

        let stream = response.bytes_stream();

        let mapped_stream = stream.map(|res| {
            res.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>).and_then(|bytes| {
                let line = std::str::from_utf8(&bytes)?;

                // Anthropic uses SSE format: "data: {...}"
                if !line.starts_with("data: ") {
                    return Err("Unexpected stream format".into());
                }

                let json_str = &line[6..];
                let chunk: AnthropicStreamResponse = serde_json::from_str(json_str)?;

                match chunk.r#type.as_str() {
                    "content_block_delta" => {
                        if let Some(delta) = chunk.delta {
                            if let Some(text) = delta.text {
                                return Ok(GaiseInstructStreamResponse {
                                    chunk: GaiseStreamChunk::Text(text),
                                    external_id: chunk.message.as_ref().map(|m| m.id.clone()),
                                });
                            }
                            if let Some(partial_json) = delta.partial_json {
                                return Ok(GaiseInstructStreamResponse {
                                    chunk: GaiseStreamChunk::ToolCall {
                                        index: chunk.index.unwrap_or(0),
                                        id: None,
                                        name: None,
                                        arguments: Some(partial_json),
                                    },
                                    external_id: chunk.message.as_ref().map(|m| m.id.clone()),
                                });
                            }
                        }
                    }
                    "content_block_start" => {
                        if let Some(content_block) = chunk.content_block {
                            if let AnthropicContentBlock::ToolUse { id, name, .. } = content_block {
                                return Ok(GaiseInstructStreamResponse {
                                    chunk: GaiseStreamChunk::ToolCall {
                                        index: chunk.index.unwrap_or(0),
                                        id: Some(id),
                                        name: Some(name),
                                        arguments: None,
                                    },
                                    external_id: chunk.message.as_ref().map(|m| m.id.clone()),
                                });
                            }
                        }
                    }
                    _ => {}
                }

                Err("Empty chunk".into())
            })
        })
        .filter(|res| {
            match res {
                Err(e) if e.to_string() == "Empty chunk" => futures_util::future::ready(false),
                _ => futures_util::future::ready(true),
            }
        });

        Ok(Box::pin(mapped_stream))
    }

    async fn instruct(&self, request: &GaiseInstructRequest) -> Result<GaiseInstructResponse, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/messages", self.api_url);

        let anthropic_request = AnthropicRequest::from(request);

        let response = self.client.post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", &self.api_version)
            .header("content-type", "application/json")
            .json(&anthropic_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format!("Anthropic API error: {}", err_text).into());
        }

        let anthropic_response: AnthropicResponse = response.json().await?;

        let (content, tool_calls) = self.map_from_anthropic_content(anthropic_response.content);

        let message = GaiseMessage {
            role: anthropic_response.role,
            content,
            tool_calls,
            tool_call_id: None,
        };

        let mut input = HashMap::new();
        input.insert("input_tokens".to_string(), anthropic_response.usage.input_tokens);
        let mut output = HashMap::new();
        output.insert("output_tokens".to_string(), anthropic_response.usage.output_tokens);

        Ok(GaiseInstructResponse {
            output: OneOrMany::One(message),
            external_id: Some(anthropic_response.id),
            usage: Some(GaiseUsage {
                input: Some(input),
                output: Some(output),
            }),
        })
    }

    async fn embeddings(&self, _request: &GaiseEmbeddingsRequest) -> Result<GaiseEmbeddingsResponse, Box<dyn std::error::Error + Send + Sync>> {
        Err("Anthropic does not support embeddings API".into())
    }
}
