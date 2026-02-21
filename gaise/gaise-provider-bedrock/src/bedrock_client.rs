use async_trait::async_trait;
use aws_sdk_bedrockruntime::Client as BedrockClient;
use gaise_core::GaiseClient;
use gaise_core::contracts::{
    GaiseInstructRequest, GaiseInstructResponse, GaiseInstructStreamResponse,
    GaiseEmbeddingsRequest, GaiseEmbeddingsResponse, GaiseMessage, GaiseContent,
    OneOrMany, GaiseToolCall, GaiseFunctionCall, GaiseStreamChunk,
};
use std::pin::Pin;
use futures_util::{Stream};
use std::error::Error;

pub struct GaiseClientBedrock {
    client: BedrockClient,
}

impl GaiseClientBedrock {
    pub async fn new() -> Self {
        let config = aws_config::load_from_env().await;
        let client = BedrockClient::new(&config);
        Self { client }
    }

    pub fn with_client(client: BedrockClient) -> Self {
        Self { client }
    }

    fn to_document(value: &serde_json::Value) -> aws_smithy_types::Document {
        match value {
            serde_json::Value::Null => aws_smithy_types::Document::Null,
            serde_json::Value::Bool(b) => aws_smithy_types::Document::Bool(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    aws_smithy_types::Document::Number(aws_smithy_types::Number::NegInt(i))
                } else if let Some(u) = n.as_u64() {
                    aws_smithy_types::Document::Number(aws_smithy_types::Number::PosInt(u))
                } else {
                    aws_smithy_types::Document::Number(aws_smithy_types::Number::Float(n.as_f64().unwrap_or(0.0)))
                }
            }
            serde_json::Value::String(s) => aws_smithy_types::Document::String(s.clone()),
            serde_json::Value::Array(a) => {
                aws_smithy_types::Document::Array(a.iter().map(Self::to_document).collect())
            }
            serde_json::Value::Object(o) => {
                aws_smithy_types::Document::Object(o.iter().map(|(k, v)| (k.clone(), Self::to_document(v))).collect())
            }
        }
    }

    fn map_gaise_content_to_bedrock(content: &GaiseContent) -> Vec<aws_sdk_bedrockruntime::types::ContentBlock> {
        match content {
            GaiseContent::Text { text } => vec![aws_sdk_bedrockruntime::types::ContentBlock::Text(text.clone())],
            GaiseContent::Image { data, format } => {
                let format = match format.as_deref() {
                    Some("png") => aws_sdk_bedrockruntime::types::ImageFormat::Png,
                    Some("jpeg") | Some("jpg") => aws_sdk_bedrockruntime::types::ImageFormat::Jpeg,
                    Some("webp") => aws_sdk_bedrockruntime::types::ImageFormat::Webp,
                    Some("gif") => aws_sdk_bedrockruntime::types::ImageFormat::Gif,
                    _ => aws_sdk_bedrockruntime::types::ImageFormat::Jpeg,
                };
                vec![aws_sdk_bedrockruntime::types::ContentBlock::Image(
                    aws_sdk_bedrockruntime::types::ImageBlock::builder()
                        .format(format)
                        .source(aws_sdk_bedrockruntime::types::ImageSource::Bytes(aws_smithy_types::Blob::new(data.clone())))
                        .build()
                        .expect("Failed to build ImageBlock")
                )]
            }
            GaiseContent::Parts { parts } => {
                parts.iter().flat_map(|p| Self::map_gaise_content_to_bedrock(p)).collect()
            }
            GaiseContent::File { data, name } => {
                let format = if let Some(n) = name {
                    if n.ends_with(".pdf") { aws_sdk_bedrockruntime::types::DocumentFormat::Pdf }
                    else if n.ends_with(".csv") { aws_sdk_bedrockruntime::types::DocumentFormat::Csv }
                    else if n.ends_with(".doc") { aws_sdk_bedrockruntime::types::DocumentFormat::Doc }
                    else if n.ends_with(".docx") { aws_sdk_bedrockruntime::types::DocumentFormat::Docx }
                    else if n.ends_with(".xls") { aws_sdk_bedrockruntime::types::DocumentFormat::Xls }
                    else if n.ends_with(".xlsx") { aws_sdk_bedrockruntime::types::DocumentFormat::Xlsx }
                    else if n.ends_with(".html") { aws_sdk_bedrockruntime::types::DocumentFormat::Html }
                    else if n.ends_with(".txt") { aws_sdk_bedrockruntime::types::DocumentFormat::Txt }
                    else if n.ends_with(".md") { aws_sdk_bedrockruntime::types::DocumentFormat::Md }
                    else { aws_sdk_bedrockruntime::types::DocumentFormat::Txt }
                } else {
                    aws_sdk_bedrockruntime::types::DocumentFormat::Txt
                };

                vec![aws_sdk_bedrockruntime::types::ContentBlock::Document(
                    aws_sdk_bedrockruntime::types::DocumentBlock::builder()
                        .name(name.clone().unwrap_or_else(|| "document".to_string()))
                        .format(format)
                        .source(aws_sdk_bedrockruntime::types::DocumentSource::Bytes(aws_smithy_types::Blob::new(data.clone())))
                        .build()
                        .expect("Failed to build DocumentBlock")
                )]
            }
             _ => vec![],
        }
    }

    fn map_gaise_message_to_bedrock(msg: &GaiseMessage) -> Option<aws_sdk_bedrockruntime::types::Message> {
        let role = match msg.role.as_str() {
            "user" => aws_sdk_bedrockruntime::types::ConversationRole::User,
            "assistant" => aws_sdk_bedrockruntime::types::ConversationRole::Assistant,
            _ => return None,
        };

        let mut content_blocks = Vec::new();

        if let Some(content) = &msg.content {
            match content {
                OneOrMany::One(c) => content_blocks.extend(Self::map_gaise_content_to_bedrock(c)),
                OneOrMany::Many(v) => {
                    for c in v {
                        content_blocks.extend(Self::map_gaise_content_to_bedrock(c));
                    }
                }
            }
        }

        Some(aws_sdk_bedrockruntime::types::Message::builder()
            .role(role)
            .set_content(Some(content_blocks))
            .build()
            .expect("Failed to build Message"))
    }
}

#[async_trait]
impl GaiseClient for GaiseClientBedrock {
    async fn instruct(&self, request: &GaiseInstructRequest) -> Result<GaiseInstructResponse, Box<dyn Error + Send + Sync>> {
        let mut messages = Vec::new();
        let mut system_messages = Vec::new();

        let inputs = match &request.input {
            OneOrMany::One(m) => vec![m],
            OneOrMany::Many(v) => v.iter().collect(),
        };

        for msg in inputs {
            if msg.role == "system" {
                if let Some(content) = &msg.content {
                     match content {
                         OneOrMany::One(c) => {
                             if let GaiseContent::Text { text } = c {
                                 system_messages.push(aws_sdk_bedrockruntime::types::SystemContentBlock::Text(text.clone()));
                             }
                         }
                         OneOrMany::Many(v) => {
                             for c in v {
                                 if let GaiseContent::Text { text } = c {
                                     system_messages.push(aws_sdk_bedrockruntime::types::SystemContentBlock::Text(text.clone()));
                                 }
                             }
                         }
                     }
                }
            } else if let Some(m) = Self::map_gaise_message_to_bedrock(msg) {
                messages.push(m);
            }
        }

        let mut builder = self.client.converse()
            .model_id(&request.model)
            .set_messages(Some(messages));

        if !system_messages.is_empty() {
            builder = builder.set_system(Some(system_messages));
        }

        if let Some(config) = &request.generation_config {
            let mut inf_cfg = aws_sdk_bedrockruntime::types::InferenceConfiguration::builder();
            if let Some(t) = config.temperature { inf_cfg = inf_cfg.temperature(t); }
            if let Some(p) = config.top_p { inf_cfg = inf_cfg.top_p(p); }
            if let Some(m) = config.max_tokens { inf_cfg = inf_cfg.max_tokens(m as i32); }
            builder = builder.inference_config(inf_cfg.build());
        }

        if let Some(tools) = &request.tools {
            let mut tool_list = Vec::new();
            for t in tools {
                let tool_spec = aws_sdk_bedrockruntime::types::ToolSpecification::builder()
                    .name(&t.name)
                    .set_description(t.description.clone())
                    .input_schema(aws_sdk_bedrockruntime::types::ToolInputSchema::Json(Self::to_document(&serde_json::to_value(&t.parameters).unwrap())))
                    .build()
                    .expect("Failed to build ToolSpec");
                tool_list.push(aws_sdk_bedrockruntime::types::Tool::ToolSpec(tool_spec));
            }
            builder = builder.tool_config(aws_sdk_bedrockruntime::types::ToolConfiguration::builder().set_tools(Some(tool_list)).build().expect("Failed to build ToolConfiguration"));
        }

        let response = builder.send().await?;

        let output = response.output.ok_or("No output from Bedrock")?;
        let message = match output {
            aws_sdk_bedrockruntime::types::ConverseOutput::Message(m) => m,
            _ => return Err("Unexpected output type from Bedrock".into()),
        };

        let mut gaise_content = Vec::new();
        let mut tool_calls = Vec::new();

        for block in message.content {
            match block {
                aws_sdk_bedrockruntime::types::ContentBlock::Text(t) => gaise_content.push(GaiseContent::Text { text: t }),
                aws_sdk_bedrockruntime::types::ContentBlock::ToolUse(tu) => {
                    tool_calls.push(GaiseToolCall {
                        id: tu.tool_use_id,
                        r#type: "function".to_string(),
                        function: GaiseFunctionCall {
                            name: tu.name,
                            arguments: Some(format!("{:?}", tu.input)),
                        },
                    });
                }
                _ => {}
            }
        }

        Ok(GaiseInstructResponse {
            output: OneOrMany::One(GaiseMessage {
                role: "assistant".to_string(),
                content: if gaise_content.is_empty() { None } else { Some(OneOrMany::Many(gaise_content)) },
                tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
                tool_call_id: None,
            }),
            external_id: None,
            usage: None,
        })
    }

    async fn instruct_stream(&self, request: &GaiseInstructRequest) -> Result<Pin<Box<dyn Stream<Item = Result<GaiseInstructStreamResponse, Box<dyn Error + Send + Sync>>> + Send>>, Box<dyn Error + Send + Sync>> {
        let mut messages = Vec::new();
        let mut system_messages = Vec::new();

        let inputs = match &request.input {
            OneOrMany::One(m) => vec![m],
            OneOrMany::Many(v) => v.iter().collect(),
        };

        for msg in inputs {
            if msg.role == "system" {
                if let Some(content) = &msg.content {
                     match content {
                         OneOrMany::One(c) => {
                             if let GaiseContent::Text { text } = c {
                                 system_messages.push(aws_sdk_bedrockruntime::types::SystemContentBlock::Text(text.clone()));
                             }
                         }
                         OneOrMany::Many(v) => {
                             for c in v {
                                 if let GaiseContent::Text { text } = c {
                                     system_messages.push(aws_sdk_bedrockruntime::types::SystemContentBlock::Text(text.clone()));
                                 }
                             }
                         }
                     }
                }
            } else if let Some(m) = Self::map_gaise_message_to_bedrock(msg) {
                messages.push(m);
            }
        }

        let mut builder = self.client.converse_stream()
            .model_id(&request.model)
            .set_messages(Some(messages));

        if !system_messages.is_empty() {
            builder = builder.set_system(Some(system_messages));
        }

        if let Some(config) = &request.generation_config {
            let mut inf_cfg = aws_sdk_bedrockruntime::types::InferenceConfiguration::builder();
            if let Some(t) = config.temperature { inf_cfg = inf_cfg.temperature(t); }
            if let Some(p) = config.top_p { inf_cfg = inf_cfg.top_p(p); }
            if let Some(m) = config.max_tokens { inf_cfg = inf_cfg.max_tokens(m as i32); }
            builder = builder.inference_config(inf_cfg.build());
        }

        if let Some(tools) = &request.tools {
             let mut tool_list = Vec::new();
            for t in tools {
                let tool_spec = aws_sdk_bedrockruntime::types::ToolSpecification::builder()
                    .name(&t.name)
                    .set_description(t.description.clone())
                    .input_schema(aws_sdk_bedrockruntime::types::ToolInputSchema::Json(Self::to_document(&serde_json::to_value(&t.parameters).unwrap())))
                    .build()
                    .expect("Failed to build ToolSpec");
                tool_list.push(aws_sdk_bedrockruntime::types::Tool::ToolSpec(tool_spec));
            }
            builder = builder.tool_config(aws_sdk_bedrockruntime::types::ToolConfiguration::builder().set_tools(Some(tool_list)).build().expect("Failed to build ToolConfiguration"));
        }

        let response = builder.send().await?;
        let mut stream = response.stream;

        let gaise_stream = async_stream::stream! {
            while let Ok(Some(event)) = stream.recv().await {
                match event {
                    aws_sdk_bedrockruntime::types::ConverseStreamOutput::ContentBlockDelta(delta) => {
                        if let Some(d) = delta.delta {
                            match d {
                                aws_sdk_bedrockruntime::types::ContentBlockDelta::Text(t) => {
                                    yield Ok(GaiseInstructStreamResponse {
                                        chunk: GaiseStreamChunk::Text(t),
                                        external_id: None,
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        };

        Ok(Box::pin(gaise_stream))
    }

    async fn embeddings(&self, request: &GaiseEmbeddingsRequest) -> Result<GaiseEmbeddingsResponse, Box<dyn Error + Send + Sync>> {
        let inputs = match &request.input {
            OneOrMany::One(s) => vec![s.clone()],
            OneOrMany::Many(v) => v.clone(),
        };

        let mut embeddings = Vec::new();

        for input in inputs {
            let body = if request.model.contains("titan") {
                serde_json::json!({
                    "inputText": input
                })
            } else if request.model.contains("cohere") {
                serde_json::json!({
                    "texts": [input],
                    "input_type": "search_document"
                })
            } else {
                return Err(format!("Unsupported embedding model: {}", request.model).into());
            };

            let response = self.client.invoke_model()
                .model_id(&request.model)
                .content_type("application/json")
                .body(aws_smithy_types::Blob::new(serde_json::to_vec(&body)?))
                .send()
                .await?;

            let response_body: serde_json::Value = serde_json::from_slice(response.body.as_ref())?;
            
            if request.model.contains("titan") {
                if let Some(embedding) = response_body["embedding"].as_array() {
                    let vec: Vec<f32> = embedding.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect();
                    embeddings.push(vec);
                }
            } else if request.model.contains("cohere") {
                if let Some(embeddings_arr) = response_body["embeddings"].as_array() {
                    if let Some(first) = embeddings_arr.first() {
                        if let Some(embedding) = first.as_array() {
                            let vec: Vec<f32> = embedding.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect();
                            embeddings.push(vec);
                        }
                    }
                }
            }
        }

        Ok(GaiseEmbeddingsResponse {
            output: embeddings,
            ..Default::default()
        })
    }
}
