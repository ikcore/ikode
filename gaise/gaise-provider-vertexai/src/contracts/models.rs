use gaise_core::contracts::{GaiseContent, GaiseEmbeddingsRequest, GaiseEmbeddingsResponse, GaiseInstructRequest, GaiseInstructResponse, GaiseMessage, OneOrMany, GaiseInstructStreamResponse, GaiseStreamChunk, GaiseUsage};
use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleAccessToken {
    pub access_token:String,
    pub token_type:String,
    pub expires_in:usize
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleInstructRequest {
    pub contents: Vec<GoogleContent>,

    #[serde(rename="system_instruction", skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GoogleContent>,

    #[serde(rename="generationConfig", skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GoogleParameters>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GoogleTool>>,

    #[serde(rename="toolConfig", skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<GoogleToolConfig>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleTool {
    #[serde(rename="functionDeclarations")]
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
    pub items: Option<Box<GoogleSchema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleToolConfig {
    #[serde(rename="functionCallingConfig")]
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
    #[serde(rename = "toolCall", skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<GoogleFunctionCall>,
    #[serde(rename = "toolResponse", skip_serializing_if = "Option::is_none")]
    pub tool_response: Option<GoogleToolResponse>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleFunctionCall {
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleToolResponse {
    pub name: String,
    pub response: serde_json::Value,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleInlineData {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub data: String,
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
    pub fn from_gaise(gaise: &GaiseContent) -> Vec<GooglePart> {
        match gaise {
            GaiseContent::Text { text } => vec![GooglePart {
                text: Some(text.clone()),
                inline_data: None,
                tool_call: None,
                tool_response: None,
            }],
            GaiseContent::Audio { data, format } => vec![GooglePart {
                text: None,
                inline_data: Some(GoogleInlineData {
                    mime_type: format.clone().unwrap_or("audio/mpeg".to_string()),
                    data: base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        data,
                    ),
                }),
                tool_call: None,
                tool_response: None,
            }],
            GaiseContent::Image { data, format } => vec![GooglePart {
                text: None,
                inline_data: Some(GoogleInlineData {
                    mime_type: format.clone().unwrap_or("image/jpeg".to_string()),
                    data: base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        data,
                    ),
                }),
                tool_call: None,
                tool_response: None,
            }],
            GaiseContent::File { data, name } => {
                let mime_type = match name.as_deref() {
                    Some(n) if n.ends_with(".pdf") => "application/pdf",
                    _ => "application/octet-stream",
                };
                vec![GooglePart {
                    text: None,
                    inline_data: Some(GoogleInlineData {
                        mime_type: mime_type.to_string(),
                        data: base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            data,
                        ),
                    }),
                    tool_call: None,
                    tool_response: None,
                }]
            }
            GaiseContent::Parts { parts } => {
                parts.iter().flat_map(GooglePart::from_gaise).collect()
            }
        }
    }

    pub fn from(gaise: &GaiseContent) -> GooglePart {
        match gaise {
            GaiseContent::Text { text } => GooglePart {
                text: Some(text.clone()),
                inline_data: None,
                tool_call: None,
                tool_response: None,
            },
            GaiseContent::Audio { data, format } => GooglePart {
                text: None,
                inline_data: Some(GoogleInlineData {
                    mime_type: format.clone().unwrap_or("audio/mpeg".to_string()),
                    data: base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        data,
                    ),
                }),
                tool_call: None,
                tool_response: None,
            },
            GaiseContent::Image { data, format } => GooglePart {
                text: None,
                inline_data: Some(GoogleInlineData {
                    mime_type: format.clone().unwrap_or("image/jpeg".to_string()),
                    data: base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        data,
                    ),
                }),
                tool_call: None,
                tool_response: None,
            },
            GaiseContent::File { data, name } => {
                let mime_type = match name.as_deref() {
                    Some(n) if n.ends_with(".pdf") => "application/pdf",
                    _ => "application/octet-stream",
                };
                GooglePart {
                    text: None,
                    inline_data: Some(GoogleInlineData {
                        mime_type: mime_type.to_string(),
                        data: base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            data,
                        ),
                    }),
                    tool_call: None,
                    tool_response: None,
                }
            }
            GaiseContent::Parts { .. } => {
                // If it's a collection of parts, we can't represent it as a single GooglePart easily
                // without losing structure, but Google expects a flat list of parts anyway.
                // We'll return the first one or a default if empty to satisfy the signature.
                // Callers should ideally use from_gaise to get multiple parts.
                GooglePart::from_gaise(gaise).into_iter().next().unwrap_or(GooglePart {
                    text: Some(String::new()),
                    inline_data: None,
                    tool_call: None,
                    tool_response: None,
                })
            }
        }
    }
}

impl GoogleInstructRequest {

    pub fn add_content(&mut self, msg: GaiseMessage) {
        if msg.role == "system" {
            if let Some(ref content) = msg.content {
                let prompt = match content {
                    OneOrMany::One(GaiseContent::Text { text }) => text.clone(),
                    _ => String::new(),
                };
                let part = GooglePart {
                    text: Some(prompt),
                    inline_data: None,
                    tool_call: None,
                    tool_response: None,
                };
                self.system_instruction = Some(GoogleContent {
                    role: "system".to_owned(),
                    parts: vec![part],
                })
            }
        } else {
            let mut parts = vec![];

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
                            name: tc.function.name.clone(),
                            args: tc.function.arguments.as_ref().and_then(|a| serde_json::from_str(a).ok()).unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
                        }),
                        tool_response: None,
                    });
                }
            }

            if let Some(ref tool_call_id) = msg.tool_call_id
                && let Some(ref content) = msg.content {
                    let response_val = match content {
                        OneOrMany::One(GaiseContent::Text { text }) => serde_json::from_str(text).unwrap_or(serde_json::Value::String(text.clone())),
                        _ => serde_json::Value::Null,
                    };
                    parts.push(GooglePart {
                        text: None,
                        inline_data: None,
                        tool_call: None,
                        tool_response: Some(GoogleToolResponse {
                            name: tool_call_id.clone(),
                            response: response_val,
                        }),
                    });
                }

            if !parts.is_empty() {
                self.contents.push(GoogleContent {
                    role: to_google_role(&msg.role).unwrap_or(msg.role),
                    parts,
                });
            }
        }
    }

    pub fn from(source: &GaiseInstructRequest) -> GoogleInstructRequest {
        let mut request = GoogleInstructRequest {
            contents: vec![],
            system_instruction: None,
            generation_config: source.generation_config.as_ref().map(|gc| GoogleParameters {
                temperature: gc.temperature,
                max_output_tokens: gc.max_tokens,
                top_p: gc.top_p,
                top_k: gc.top_k,
                ..Default::default()
            }),
            tools: source.tools.as_ref().map(|tools| {
                vec![GoogleTool {
                    function_declarations: tools
                        .iter()
                        .map(|t| GoogleFunctionDeclaration {
                            name: t.name.clone(),
                            description: t.description.clone().unwrap_or_default(),
                            parameters: t.parameters.as_ref().map(GoogleSchema::from).unwrap_or(GoogleSchema {
                                r#type: "object".to_string(),
                                description: None,
                                properties: Some(std::collections::HashMap::new()),
                                items: None,
                                required: None,
                            }),
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
            items: source.items.as_ref().map(|i| Box::new(GoogleSchema::from(i))),
            required: source.required.clone(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleEmbeddingsRequest {
    pub instances: Vec<GoogleInstance>,
    pub parameters: GoogleParameters
}

impl GoogleEmbeddingsRequest {
    pub fn from(model:&GaiseEmbeddingsRequest) -> GoogleEmbeddingsRequest {

        let instances = match &model.input  {
            OneOrMany::One(x) => {
               vec![GoogleInstance { content: Some(x.to_string()), ..Default::default() }]
            },
            OneOrMany::Many(vx) => {
                vx.iter().map(|x| GoogleInstance { content: Some(x.to_string()), ..Default::default() }).collect()
            }
        };

        GoogleEmbeddingsRequest {
            instances,
            parameters: GoogleParameters {
                auto_truncate: Some(true),
                ..Default::default()
            }
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct GoogleInstance {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<GoogleMessage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>
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

pub fn to_google_role(input:&str) -> Option<String> {
    let result = match input {
        "assistant" => "model".to_owned(),
        _ => input.to_string()
    };
    Some(result)
}

pub fn to_gaise_role(input:&str) -> Option<String> {
    let result = match input {
        "model" => "assistant".to_owned(),
        _ => input.to_string()
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


#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleChatCompletionResponse {
    pub candidates: Vec<GoogleCandidate>,
    #[serde(rename="usageMetadata")]
    pub usage_metadata: GoogleUsageMetadata,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleCandidate {
    pub content: GoogleContent,

    #[serde(rename = "finishReason")]
    pub finish_reason: Option<String>
}

impl GoogleChatCompletionResponse {
    pub fn to_stream_view(&self) -> Vec<GaiseInstructStreamResponse> {
        let mut responses = Vec::new();

        let usage = &self.usage_metadata;
        let mut input = std::collections::HashMap::new();
        if let Some(v) = usage.prompt_token_count {
            input.insert("prompt_tokens".to_string(), v);
        }

        let mut output = std::collections::HashMap::new();
        if let Some(v) = usage.candidates_token_count {
            output.insert("candidates_tokens".to_string(), v);
        }
        if let Some(v) = usage.total_token_count {
            output.insert("total_tokens".to_string(), v);
        }

        if !input.is_empty() || !output.is_empty() {
            responses.push(GaiseInstructStreamResponse {
                chunk: GaiseStreamChunk::Usage(GaiseUsage {
                    input: if input.is_empty() { None } else { Some(input) },
                    output: if output.is_empty() { None } else { Some(output) },
                }),
                external_id: None,
            });
        }

        for candidate in &self.candidates {
            for (part_idx, part) in candidate.content.parts.iter().enumerate() {
                if let Some(text) = &part.text {
                    responses.push(GaiseInstructStreamResponse {
                        chunk: GaiseStreamChunk::Text(text.clone()),
                        external_id: None,
                    });
                }
                if let Some(tool_call) = &part.tool_call {
                    responses.push(GaiseInstructStreamResponse {
                        chunk: GaiseStreamChunk::ToolCall {
                            index: part_idx,
                            id: None,
                            name: Some(tool_call.name.clone()),
                            arguments: Some(tool_call.args.to_string()),
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
                        contents.push(GaiseContent::Text { text: text.clone() });
                    }
                    if let Some(tool_call) = &part.tool_call {
                        tool_calls.push(gaise_core::contracts::GaiseToolCall {
                            id: "".to_string(), // Vertex AI doesn't always provide an ID in the same way, but it uses the name for response
                            r#type: "function".to_string(),
                            function: gaise_core::contracts::GaiseFunctionCall {
                                name: tool_call.name.clone(),
                                arguments: Some(tool_call.args.to_string()),
                            },
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
                }
            })
            .collect();

        GaiseInstructResponse {
            output: OneOrMany::Many(outputs),
            external_id: None,
            usage: None,
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
        GaiseEmbeddingsResponse {
            output: self.predictions.clone().into_iter().map(|x| x.embeddings.unwrap().values ).collect(),
            external_id: None,
            usage: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleEmbeddings {
    pub values:Vec<f32>
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GooglePrediction {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename="citationMetadata")]
    pub citation_metadata: Option<Vec<GoogleCitationMetadata>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename="safetyAttributes")]
    pub safety_attributes: Option<Vec<GoogleSafetyAttributes>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates:Option<Vec<GoogleMessage>>,
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
    #[serde(rename="safetyRatings")]
    pub safety_ratings: Vec<GoogleSafetyRating>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleSafetyRating {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename="probabilityScore")]
    pub probability_score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename="severityScore")]
    pub severity_score: Option<f32>,
    pub category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GoogleEmbeddingsMetadata {
    #[serde(rename="totalBillableCharacters")]
    pub total_billable_characters: Option<usize>,
    #[serde(rename="totalTokens")]
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
    #[serde(rename="candidatesTokenCount")]
    pub candidates_token_count: Option<usize>,
    #[serde(rename="promptTokenCount")]
    pub prompt_token_count: Option<usize>,
    #[serde(rename="totalTokenCount")]
    pub total_token_count: Option<usize>,
    #[serde(rename="thoughtsTokenCount")]
    pub thoughts_token_count: Option<usize>,
    #[serde(rename="trafficType")]
    pub traffic_type: Option<String>,
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