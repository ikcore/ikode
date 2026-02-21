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

pub struct GaiseClientOpenAI {
    api_url: String,
    api_key: String,
    client: reqwest::Client,
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
            }
        }

        OpenAITool {
            r#type: "function".to_string(),
            function: OpenAIFunction {
                name: t.name,
                description: t.description,
                parameters: OpenAIParameters {
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
}

impl From<&GaiseInstructRequest> for OpenAIChatRequest {
    fn from(request: &GaiseInstructRequest) -> Self {
        let messages = match &request.input {
            OneOrMany::One(m) => vec![m.clone()],
            OneOrMany::Many(ms) => ms.clone(),
        };

        let openai_messages = messages.into_iter().map(|m| {
            let content = m.content.map(|c| {
                let items = match c {
                    OneOrMany::One(item) => vec![item],
                    OneOrMany::Many(items) => items,
                };

                let parts: Vec<OpenAIContentPart> = items.into_iter().filter_map(|item| {
                    match item {
                        GaiseContent::Text { text } => Some(OpenAIContentPart::Text { text }),
                        GaiseContent::Image { data, format } => {
                            let base64_data = base64::prelude::BASE64_STANDARD.encode(data);
                            let mime_type = format.unwrap_or_else(|| "image/jpeg".to_string());
                            let url = format!("data:{};base64,{}", mime_type, base64_data);
                            Some(OpenAIContentPart::ImageUrl { image_url: OpenAIImageUrl { url } })
                        }
                        GaiseContent::Audio { data, format } => {
                            let base64_data = base64::prelude::BASE64_STANDARD.encode(data);
                            let audio_format = format.unwrap_or_else(|| "mp3".to_string());
                            Some(OpenAIContentPart::InputAudio { input_audio: OpenAIInputAudio { data: base64_data, format: audio_format } })
                        }
                        GaiseContent::Parts { parts } => {
                            // Recursively flattening might be complex, typically GaiseContent::Parts is not nested this way in practice for basic mappings
                            // but for completeness we'd handle it. Here we just take text from parts.
                            let mut text_acc = String::new();
                            for part in parts {
                                if let GaiseContent::Text { text } = part {
                                    text_acc.push_str(&text);
                                }
                            }
                            if text_acc.is_empty() { None } else { Some(OpenAIContentPart::Text { text: text_acc }) }
                        }
                        _ => None
                    }
                }).collect();

                if parts.len() == 1 && matches!(parts.first(), Some(OpenAIContentPart::Text { .. })) {
                    if let Some(OpenAIContentPart::Text { text }) = parts.first() {
                        return OpenAIContent::Text(text.clone());
                    }
                }
                OpenAIContent::Parts(parts)
            });

            let tool_calls = m.tool_calls.map(|tcs| {
                tcs.into_iter().map(|tc| {
                    OpenAIToolCall {
                        id: tc.id,
                        r#type: tc.r#type,
                        function: OpenAIFunctionCall {
                            name: tc.function.name,
                            arguments: tc.function.arguments.unwrap_or_default(),
                        }
                    }
                }).collect()
            });

            OpenAIMessage {
                role: m.role,
                content,
                tool_calls,
                tool_call_id: m.tool_call_id,
            }
        }).collect();

        OpenAIChatRequest {
            model: request.model.clone(),
            messages: openai_messages,
            stream: false,
            temperature: request.generation_config.as_ref().and_then(|c| c.temperature),
            top_p: request.generation_config.as_ref().and_then(|c| c.top_p),
            max_tokens: request.generation_config.as_ref().and_then(|c| c.max_tokens),
            prompt_cache_key: request.generation_config.as_ref().and_then(|c| c.cache_key.clone()),
            tools: request.tools.as_ref().map(|ts| ts.iter().map(|t| OpenAITool::from(t.clone())).collect()),
        }
    }
}

impl GaiseClientOpenAI {
    pub fn new(api_url: String, api_key: String) -> Self {
        Self {
            api_url,
            api_key,
            client: reqwest::Client::new(),
        }
    }

    fn map_from_openai_message(&self, msg: OpenAIMessage) -> GaiseMessage {
        let content = msg.content.map(|c| match c {
            OpenAIContent::Text(text) => OneOrMany::One(GaiseContent::Text { text }),
            OpenAIContent::Parts(parts) => OneOrMany::Many(parts.into_iter().filter_map(|p| match p {
                OpenAIContentPart::Text { text } => Some(GaiseContent::Text { text }),
                OpenAIContentPart::ImageUrl { .. } => {
                    // This is lossy as we don't easily get back raw bytes from URL here if it's external,
                    // but if it's data URI we could. For now, just placeholder or skip.
                    None 
                }
                OpenAIContentPart::InputAudio { .. } => None,
            }).collect())
        });

        let tool_calls = msg.tool_calls.map(|tcs| {
            tcs.into_iter().map(|tc| {
                GaiseToolCall {
                    id: tc.id,
                    r#type: tc.r#type,
                    function: GaiseFunctionCall {
                        name: tc.function.name,
                        arguments: Some(tc.function.arguments),
                    }
                }
            }).collect()
        });

        GaiseMessage {
            role: msg.role,
            content,
            tool_calls,
            tool_call_id: msg.tool_call_id,
        }
    }
}

#[async_trait]
impl GaiseClient for GaiseClientOpenAI {
    async fn instruct_stream(
        &self,
        request: &GaiseInstructRequest,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<GaiseInstructStreamResponse, Box<dyn std::error::Error + Send + Sync>>> + Send>>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let url = format!("{}/chat/completions", self.api_url);
        
        let mut openai_request = OpenAIChatRequest::from(request);
        openai_request.stream = true;

        let response = self.client.post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&openai_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format!("OpenAI API error: {}", err_text).into());
        }

        let stream = response.bytes_stream();
        
        let mapped_stream = stream.map(|res| {
            res.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>).and_then(|bytes| {
                let line = std::str::from_utf8(&bytes)?;
                if line.trim() == "data: [DONE]" {
                    // End of stream
                    return Err("EOF".into()); 
                }
                
                if !line.starts_with("data: ") {
                     return Err("Unexpected stream format".into());
                }

                let json_str = &line[6..];
                let chunk: OpenAIChatStreamResponse = serde_json::from_str(json_str)?;
                
                if let Some(choice) = chunk.choices.first() {
                    if let Some(tool_calls) = &choice.delta.tool_calls {
                        if let Some(tc) = tool_calls.first() {
                            return Ok(GaiseInstructStreamResponse {
                                chunk: GaiseStreamChunk::ToolCall {
                                    index: tc.index,
                                    id: tc.id.clone(),
                                    name: tc.function.as_ref().and_then(|f| f.name.clone()),
                                    arguments: tc.function.as_ref().and_then(|f| f.arguments.clone()),
                                },
                                external_id: Some(chunk.id.clone()),
                            });
                        }
                    }

                    if let Some(content) = &choice.delta.content {
                        return Ok(GaiseInstructStreamResponse {
                            chunk: GaiseStreamChunk::Text(content.clone()),
                            external_id: Some(chunk.id.clone()),
                        });
                    }
                }

                // If no content or tool calls, it might be an empty chunk or just metadata
                // Returning a dummy or filtering it out would be better.
                Err("Empty chunk".into())
            })
        })
        .filter(|res| {
            // Filter out EOF and Empty chunks
            match res {
                Err(e) if e.to_string() == "EOF" || e.to_string() == "Empty chunk" => futures_util::future::ready(false),
                _ => futures_util::future::ready(true),
            }
        });

        Ok(Box::pin(mapped_stream))
    }

    async fn instruct(&self, request: &GaiseInstructRequest) -> Result<GaiseInstructResponse, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/chat/completions", self.api_url);
        
        let openai_request = OpenAIChatRequest::from(request);

        let response = self.client.post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&openai_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format!("OpenAI API error: {}", err_text).into());
        }

        let openai_response: OpenAIChatResponse = response.json().await?;

        let usage = openai_response.usage.map(|u| {
            let mut input = HashMap::new();
            input.insert("prompt_tokens".to_string(), u.prompt_tokens);
            let mut output = HashMap::new();
            output.insert("completion_tokens".to_string(), u.completion_tokens);
            GaiseUsage {
                input: Some(input),
                output: Some(output),
            }
        });

        Ok(GaiseInstructResponse {
            output: OneOrMany::Many(openai_response.choices.into_iter().map(|c| self.map_from_openai_message(c.message)).collect()),
            external_id: Some(openai_response.id),
            usage,
        })
    }

    async fn embeddings(&self, request: &GaiseEmbeddingsRequest) -> Result<GaiseEmbeddingsResponse, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/embeddings", self.api_url);
        
        let input = match &request.input {
            OneOrMany::One(s) => OpenAIEmbedInput::String(s.clone()),
            OneOrMany::Many(ss) => OpenAIEmbedInput::Array(ss.clone()),
        };

        let openai_request = OpenAIEmbedRequest {
            model: request.model.clone(),
            input,
        };

        let response = self.client.post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&openai_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format!("OpenAI API error: {}", err_text).into());
        }

        let openai_response: OpenAIEmbedResponse = response.json().await?;

        let mut input_usage = HashMap::new();
        input_usage.insert("prompt_tokens".to_string(), openai_response.usage.prompt_tokens);

        Ok(GaiseEmbeddingsResponse {
            external_id: Some(openai_response.object),
            output: openai_response.data.into_iter().map(|d| d.embedding).collect(),
            usage: Some(GaiseUsage {
                input: Some(input_usage),
                output: None,
            }),
        })
    }
}
