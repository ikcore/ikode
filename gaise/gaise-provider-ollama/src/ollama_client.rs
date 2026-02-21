use async_trait::async_trait;
use gaise_core::GaiseClient;
use gaise_core::contracts::{
    GaiseContent, GaiseEmbeddingsRequest, GaiseEmbeddingsResponse, GaiseInstructRequest,
    GaiseInstructResponse, GaiseInstructStreamResponse, GaiseMessage, GaiseStreamChunk,
    GaiseUsage, OneOrMany, GaiseToolCall, GaiseFunctionCall, GaiseTool
};
use crate::contracts::*;
use futures_util::{Stream, StreamExt};
use std::collections::HashMap;
use std::pin::Pin;
use base64::Engine;

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
            }
        }

        OllamaTool {
            r#type: "function".to_string(),
            function: OllamaFunction {
                name: t.name,
                description: t.description.unwrap_or_default(),
                parameters: OllamaParameters {
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

impl From<&GaiseInstructRequest> for OllamaChatRequest {
    fn from(request: &GaiseInstructRequest) -> Self {
        let messages = match &request.input {
            OneOrMany::One(m) => vec![m.clone()],
            OneOrMany::Many(ms) => ms.clone(),
        };

        let ollama_messages = messages.into_iter().map(|m| {
            let mut content = String::new();
            let mut images = Vec::new();

            if let Some(c) = m.content {
                let contents = match c {
                    OneOrMany::One(item) => vec![item],
                    OneOrMany::Many(items) => items,
                };

                for item in contents {
                    match item {
                        GaiseContent::Text { text } => content.push_str(&text),
                        GaiseContent::Image { data, .. } => {
                            images.push(base64::prelude::BASE64_STANDARD.encode(data));
                        }
                        GaiseContent::Parts { parts } => {
                            for part in parts {
                                if let GaiseContent::Text { text } = part {
                                    content.push_str(&text);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            let tool_calls = m.tool_calls.map(|tcs| {
                tcs.into_iter().map(|tc| {
                    let arguments: HashMap<String, serde_json::Value> = tc.function.arguments
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
                        }
                    }
                }).collect()
            });

            OllamaMessage {
                role: m.role,
                content,
                images: if images.is_empty() { None } else { Some(images) },
                tool_calls,
            }
        }).collect();

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
            tools: request.tools.as_ref().map(|ts| ts.iter().map(|t| OllamaTool::from(t.clone())).collect()),
            format: None,
        }
    }
}

impl GaiseClientOllama {
    pub fn new(api_url: String) -> Self {
        Self {
            api_url,
            client: reqwest::Client::new(),
        }
    }

    fn map_from_ollama_message(&self, msg: OllamaMessage) -> GaiseMessage {
        let tool_calls = msg.tool_calls.map(|tcs| {
            tcs.into_iter().map(|tc| {
                GaiseToolCall {
                    id: String::new(), // Ollama doesn't seem to provide IDs for tool calls in this format
                    r#type: "function".to_string(),
                    function: GaiseFunctionCall {
                        name: tc.function.name,
                        arguments: Some(serde_json::to_string(&tc.function.arguments).unwrap_or_default()),
                    }
                }
            }).collect()
        });

        GaiseMessage {
            role: msg.role,
            content: if msg.content.is_empty() { None } else { Some(OneOrMany::One(GaiseContent::Text { text: msg.content })) },
            tool_calls,
            tool_call_id: None,
        }
    }
}

#[async_trait]
impl GaiseClient for GaiseClientOllama {
    async fn instruct_stream(
        &self,
        request: &GaiseInstructRequest,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<GaiseInstructStreamResponse, Box<dyn std::error::Error + Send + Sync>>> + Send>>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let url = format!("{}/api/chat", self.api_url);
        
        let mut ollama_request = OllamaChatRequest::from(request);
        ollama_request.stream = true;

        let response = self.client.post(url)
            .json(&ollama_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format!("Ollama API error: {}", err_text).into());
        }

        let stream = response.bytes_stream();
        
        let mapped_stream = stream.map(|res| {
            res.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>).and_then(|bytes| {
                let chunk: OllamaChatResponse = serde_json::from_slice(&bytes)?;
                
                // Ollama stream chunks usually contain one message piece or tool call
                if let Some(tool_calls) = chunk.message.tool_calls {
                    if let Some((index, tc)) = tool_calls.into_iter().enumerate().next() {
                        return Ok(GaiseInstructStreamResponse {
                            chunk: GaiseStreamChunk::ToolCall {
                                index,
                                id: None,
                                name: Some(tc.function.name),
                                arguments: Some(serde_json::to_string(&tc.function.arguments).unwrap_or_default()),
                            },
                            external_id: None,
                        });
                    }
                }

                if chunk.done {
                    // Could emit usage here
                    let mut input_usage = HashMap::new();
                    input_usage.insert("prompt_tokens".to_string(), chunk.prompt_eval_count.unwrap_or(0));
                    let mut output_usage = HashMap::new();
                    output_usage.insert("completion_tokens".to_string(), chunk.eval_count.unwrap_or(0));
                    
                    return Ok(GaiseInstructStreamResponse {
                        chunk: GaiseStreamChunk::Usage(GaiseUsage {
                            input: Some(input_usage),
                            output: Some(output_usage),
                        }),
                        external_id: None,
                    });
                }

                Ok(GaiseInstructStreamResponse {
                    chunk: GaiseStreamChunk::Text(chunk.message.content),
                    external_id: None,
                })
            })
        });

        Ok(Box::pin(mapped_stream))
    }

    async fn instruct(&self, request: &GaiseInstructRequest) -> Result<GaiseInstructResponse, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/chat", self.api_url);
        
        let ollama_request = OllamaChatRequest::from(request);

        let response = self.client.post(url)
            .json(&ollama_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format!("Ollama API error: {}", err_text).into());
        }

        let ollama_response: OllamaChatResponse = response.json().await?;

        let mut input_usage = HashMap::new();
        input_usage.insert("prompt_tokens".to_string(), ollama_response.prompt_eval_count.unwrap_or(0));
        let mut output_usage = HashMap::new();
        output_usage.insert("completion_tokens".to_string(), ollama_response.eval_count.unwrap_or(0));

        Ok(GaiseInstructResponse {
            output: OneOrMany::One(self.map_from_ollama_message(ollama_response.message)),
            external_id: None,
            usage: Some(GaiseUsage {
                input: Some(input_usage),
                output: Some(output_usage),
            }),
        })
    }

    async fn embeddings(&self, request: &GaiseEmbeddingsRequest) -> Result<GaiseEmbeddingsResponse, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/embed", self.api_url);
        
        let inputs = match &request.input {
            OneOrMany::One(s) => vec![s.clone()],
            OneOrMany::Many(ss) => ss.clone(),
        };

        let ollama_request = OllamaEmbedRequest {
            model: request.model.clone(),
            input: inputs,
            options: None,
        };

        let response = self.client.post(url)
            .json(&ollama_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(format!("Ollama API error: {}", err_text).into());
        }

        let ollama_response: OllamaEmbedResponse = response.json().await?;

        let mut input_usage = HashMap::new();
        input_usage.insert("prompt_tokens".to_string(), ollama_response.prompt_eval_count.unwrap_or(0));

        Ok(GaiseEmbeddingsResponse {
            external_id: None,
            output: ollama_response.embeddings,
            usage: Some(GaiseUsage {
                input: Some(input_usage),
                output: None,
            }),
        })
    }
}
