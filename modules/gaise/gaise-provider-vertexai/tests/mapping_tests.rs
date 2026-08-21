use gaise_core::contracts::{
    GaiseContent, GaiseFunctionCall, GaiseGenerationConfig, GaiseImageConfig, GaiseInstructRequest,
    GaiseMessage, GaiseStreamChunk, GaiseTool, GaiseToolCall, GaiseToolParameter, OneOrMany,
};
use gaise_provider_vertexai::contracts::models::{
    GoogleChatCompletionResponse, GoogleInstructRequest,
};
use std::collections::BTreeMap;

#[test]
fn test_mapping_tool_request() {
    let mut properties = BTreeMap::new();
    properties.insert(
        "location".to_string(),
        GaiseToolParameter {
            r#type: Some("string".to_string()),
            description: Some("The city and state, e.g. San Francisco, CA".to_string()),
            ..Default::default()
        },
    );

    let request = GaiseInstructRequest {
        model: "gemini-1.5-pro".to_string(),
        tools: Some(vec![GaiseTool {
            name: "get_current_weather".to_string(),
            description: Some("Get the current weather in a given location".to_string()),
            parameters: Some(GaiseToolParameter {
                r#type: Some("object".to_string()),
                properties: Some(properties),
                required: Some(vec!["location".to_string()]),
                ..Default::default()
            }),
        }]),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "What's the weather like in Boston?".to_string(),
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let google_request = GoogleInstructRequest::from(&request);

    let tools = google_request.tools.expect("Missing tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].function_declarations.len(), 1);
    assert_eq!(
        tools[0].function_declarations[0].name,
        "get_current_weather"
    );
    assert_eq!(
        tools[0].function_declarations[0].parameters.r#type,
        "object"
    );
    assert!(
        tools[0].function_declarations[0]
            .parameters
            .properties
            .as_ref()
            .unwrap()
            .contains_key("location")
    );
}

#[test]
fn test_mapping_text_request() {
    let request = GaiseInstructRequest {
        model: "gemini-1.5-pro".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Hello".to_string(),
            })),
            ..Default::default()
        }),
        generation_config: Some(GaiseGenerationConfig {
            temperature: Some(0.7),
            max_tokens: Some(100),
            ..Default::default()
        }),
        ..Default::default()
    };

    let google_request = GoogleInstructRequest::from(&request);

    assert_eq!(google_request.contents.len(), 1);
    assert_eq!(google_request.contents[0].role, "user");
    assert_eq!(google_request.contents[0].parts.len(), 1);
    assert_eq!(
        google_request.contents[0].parts[0].text,
        Some("Hello".to_string())
    );

    let gen_config = google_request
        .generation_config
        .as_ref()
        .expect("Missing generation config");
    assert_eq!(gen_config.temperature, Some(0.7));
    assert_eq!(gen_config.max_output_tokens, Some(100));
}

#[test]
fn test_mapping_multimodal_request() {
    let request = GaiseInstructRequest {
        model: "gemini-1.5-flash".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::Many(vec![
                GaiseContent::Text {
                    text: "What is in this image?".to_string(),
                },
                GaiseContent::Image {
                    data: vec![1, 2, 3],
                    format: Some("image/png".to_string()),
                },
            ])),
            ..Default::default()
        }),
        ..Default::default()
    };

    let google_request = GoogleInstructRequest::from(&request);

    assert_eq!(google_request.contents.len(), 1);
    assert_eq!(google_request.contents[0].parts.len(), 2);

    assert_eq!(
        google_request.contents[0].parts[0].text,
        Some("What is in this image?".to_string())
    );

    let image_part = &google_request.contents[0].parts[1];
    assert!(image_part.text.is_none());
    let inline_data = image_part
        .inline_data
        .as_ref()
        .expect("Missing inline data");
    assert_eq!(inline_data.mime_type, "image/png");
    assert_eq!(inline_data.data, "AQID"); // base64 for [1, 2, 3]
}

#[test]
fn test_mapping_multipart_multimodal_request() {
    let request = GaiseInstructRequest {
        model: "gemini-1.5-flash".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::Many(vec![
                GaiseContent::Text {
                    text: "can you take this image".to_string(),
                },
                GaiseContent::Image {
                    data: vec![1, 2, 3],
                    format: Some("image/png".to_string()),
                },
                GaiseContent::Text {
                    text: "and make it look like this other images style".to_string(),
                },
                GaiseContent::Image {
                    data: vec![4, 5, 6],
                    format: Some("image/jpeg".to_string()),
                },
            ])),
            ..Default::default()
        }),
        ..Default::default()
    };

    let google_request = GoogleInstructRequest::from(&request);

    assert_eq!(google_request.contents.len(), 1);
    assert_eq!(google_request.contents[0].parts.len(), 4);

    assert_eq!(
        google_request.contents[0].parts[0].text,
        Some("can you take this image".to_string())
    );
    assert_eq!(
        google_request.contents[0].parts[1]
            .inline_data
            .as_ref()
            .unwrap()
            .data,
        "AQID"
    );
    assert_eq!(
        google_request.contents[0].parts[2].text,
        Some("and make it look like this other images style".to_string())
    );
    assert_eq!(
        google_request.contents[0].parts[3]
            .inline_data
            .as_ref()
            .unwrap()
            .data,
        "BAUG"
    );
}

#[test]
fn test_mapping_nested_parts_request() {
    let request = GaiseInstructRequest {
        model: "gemini-1.5-flash".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Parts {
                parts: vec![
                    GaiseContent::Text {
                        text: "Combined parts:".to_string(),
                    },
                    GaiseContent::Image {
                        data: vec![1, 1, 1],
                        format: Some("image/png".to_string()),
                    },
                ],
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let google_request = GoogleInstructRequest::from(&request);

    assert_eq!(google_request.contents.len(), 1);
    assert_eq!(google_request.contents[0].parts.len(), 2);
    assert_eq!(
        google_request.contents[0].parts[0].text,
        Some("Combined parts:".to_string())
    );
    assert_eq!(
        google_request.contents[0].parts[1]
            .inline_data
            .as_ref()
            .unwrap()
            .data,
        "AQEB"
    );
}

#[test]
fn test_mapping_system_instruction() {
    let request = GaiseInstructRequest {
        model: "gemini-1.5-pro".to_string(),
        input: OneOrMany::Many(vec![
            GaiseMessage {
                role: "system".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text {
                    text: "You are a helpful assistant.".to_string(),
                })),
                ..Default::default()
            },
            GaiseMessage {
                role: "user".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text {
                    text: "Hi".to_string(),
                })),
                ..Default::default()
            },
        ]),
        ..Default::default()
    };

    let google_request = GoogleInstructRequest::from(&request);

    let system_instruction = google_request
        .system_instruction
        .expect("Missing system instruction");
    assert_eq!(
        system_instruction.parts[0].text,
        Some("You are a helpful assistant.".to_string())
    );

    assert_eq!(google_request.contents.len(), 1);
    assert_eq!(google_request.contents[0].role, "user");
    assert_eq!(
        google_request.contents[0].parts[0].text,
        Some("Hi".to_string())
    );
}

#[test]
fn test_mapping_thinking_fields() {
    let request = GaiseInstructRequest {
        model: "gemini-2.5-flash".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Hello".to_string(),
            })),
            ..Default::default()
        }),
        generation_config: Some(GaiseGenerationConfig {
            thinking_effort: Some("high".to_string()),
            thinking_tokens: Some(10000),
            max_tokens: Some(16000),
            temperature: Some(0.5),
            ..Default::default()
        }),
        ..Default::default()
    };

    let google_request = GoogleInstructRequest::from(&request);

    let gen_config = google_request
        .generation_config
        .as_ref()
        .expect("Missing generation config");
    assert_eq!(gen_config.max_output_tokens, Some(16000));
    assert_eq!(gen_config.temperature, Some(0.5));
    let thinking = gen_config
        .thinking_config
        .as_ref()
        .expect("Missing thinking config");
    assert_eq!(thinking.thinking_level, None);
    assert_eq!(thinking.thinking_budget, Some(10000));
    assert_eq!(thinking.include_thoughts, Some(true));

    let json = serde_json::to_value(&google_request).unwrap();
    assert!(json["generationConfig"]["thinkingConfig"]["thinkingLevel"].is_null());
    assert_eq!(
        json["generationConfig"]["thinkingConfig"]["thinkingBudget"],
        10000
    );

    assert_eq!(google_request.contents.len(), 1);
    assert_eq!(
        google_request.contents[0].parts[0].text,
        Some("Hello".to_string())
    );
}

#[test]
fn test_mapping_thinking_levels_and_budget_only_without_http() {
    for (effort, expected) in [("low", "LOW"), ("medium", "MEDIUM"), ("high", "HIGH")] {
        let request = GaiseInstructRequest {
            model: "gemini-3-pro-preview".to_string(),
            input: OneOrMany::One(GaiseMessage {
                role: "user".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text {
                    text: "reason locally".to_string(),
                })),
                ..Default::default()
            }),
            generation_config: Some(GaiseGenerationConfig {
                thinking_effort: Some(effort.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mapped = GoogleInstructRequest::from(&request);
        let thinking = mapped.generation_config.unwrap().thinking_config.unwrap();
        assert_eq!(thinking.thinking_level.as_deref(), Some(expected));
        assert_eq!(thinking.thinking_budget, None);
        assert_eq!(thinking.include_thoughts, Some(true));
    }

    let budget_only = GaiseInstructRequest {
        model: "gemini-2.5-flash".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "budgeted reasoning".to_string(),
            })),
            ..Default::default()
        }),
        generation_config: Some(GaiseGenerationConfig {
            thinking_tokens: Some(4096),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mapped = GoogleInstructRequest::from(&budget_only);
    let thinking = mapped.generation_config.unwrap().thinking_config.unwrap();
    assert_eq!(thinking.thinking_level, None);
    assert_eq!(thinking.thinking_budget, Some(4096));
    assert_eq!(thinking.include_thoughts, Some(true));
}

#[test]
fn test_mapping_pdf_file_uses_inline_data_without_http() {
    let request = GaiseInstructRequest {
        model: "gemini-2.5-pro".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::Many(vec![
                GaiseContent::Text {
                    text: "Summarize".to_string(),
                },
                GaiseContent::File {
                    data: vec![1, 2, 3],
                    name: Some("REPORT.PDF".to_string()),
                },
            ])),
            ..Default::default()
        }),
        ..Default::default()
    };

    let mapped = GoogleInstructRequest::from(&request);
    let file = mapped.contents[0].parts[1].inline_data.as_ref().unwrap();
    assert_eq!(file.mime_type, "application/pdf");
    assert_eq!(file.data, "AQID");
    let json = serde_json::to_value(mapped).unwrap();
    assert_eq!(
        json["contents"][0]["parts"][1]["inlineData"]["mimeType"],
        "application/pdf"
    );
}

#[test]
fn test_mapping_image_generation_config_and_fixed_sampling_without_http() {
    let request = GaiseInstructRequest {
        model: "gemini-3.6-flash".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Create a landscape".to_string(),
            })),
            ..Default::default()
        }),
        generation_config: Some(GaiseGenerationConfig {
            temperature: Some(0.8),
            top_p: Some(0.9),
            top_k: Some(40),
            response_modalities: Some(vec!["text".to_string(), "image".to_string()]),
            image_config: Some(GaiseImageConfig {
                aspect_ratio: Some("16:9".to_string()),
                image_size: Some("2K".to_string()),
            }),
            input_media_resolution: Some("high".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let mapped = GoogleInstructRequest::from(&request);
    let config = mapped.generation_config.expect("Missing generation config");
    assert_eq!(config.temperature, None);
    assert_eq!(config.top_p, None);
    assert_eq!(config.top_k, None);
    assert_eq!(
        config.response_modalities,
        Some(vec!["TEXT".to_string(), "IMAGE".to_string()])
    );
    assert!(config.image_config.is_none());
    let image = config
        .response_format
        .expect("Missing response format")
        .image
        .expect("Missing image config");
    assert_eq!(image.aspect_ratio.as_deref(), Some("16:9"));
    assert_eq!(image.image_size.as_deref(), Some("2K"));
    assert_eq!(
        config.media_resolution.as_deref(),
        Some("MEDIA_RESOLUTION_HIGH")
    );
}

#[test]
fn test_mapping_generated_image_reasoning_and_tool_signature_without_http() {
    let response: GoogleChatCompletionResponse = serde_json::from_value(serde_json::json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [
                    {
                        "text": "visual plan",
                        "thought": true,
                        "thoughtSignature": "reasoning-signature"
                    },
                    {
                        "inlineData": {
                            "mimeType": "image/png",
                            "data": "AQID"
                        }
                    },
                    {
                        "functionCall": {
                            "name": "store_asset",
                            "args": {"name": "landscape.png"}
                        },
                        "thoughtSignature": "tool-signature"
                    }
                ]
            },
            "finishReason": "STOP"
        }]
    }))
    .expect("Valid local Vertex response fixture");

    let mapped = response.to_view();
    let OneOrMany::Many(outputs) = mapped.output else {
        panic!("Expected candidate list");
    };
    let output = &outputs[0];
    let OneOrMany::Many(contents) = output.content.as_ref().expect("Missing content") else {
        panic!("Expected ordered multimodal content");
    };
    assert_eq!(
        contents,
        &vec![
            GaiseContent::Reasoning {
                text: "visual plan".to_string(),
                signature: Some("reasoning-signature".to_string()),
            },
            GaiseContent::Image {
                data: vec![1, 2, 3],
                format: Some("image/png".to_string()),
            },
        ]
    );
    assert_eq!(
        output.tool_calls.as_ref().unwrap()[0]
            .thought_signature
            .as_deref(),
        Some("tool-signature")
    );

    let chunks = response.to_stream_view();
    assert!(matches!(
        &chunks[0].chunk,
        GaiseStreamChunk::Content(GaiseContent::Reasoning { signature, .. })
            if signature.as_deref() == Some("reasoning-signature")
    ));
    assert!(matches!(
        &chunks[1].chunk,
        GaiseStreamChunk::Content(GaiseContent::Image { data, format })
            if data == &vec![1, 2, 3] && format.as_deref() == Some("image/png")
    ));
    assert!(matches!(
        &chunks[2].chunk,
        GaiseStreamChunk::ToolCall { thought_signature, .. }
            if thought_signature.as_deref() == Some("tool-signature")
    ));
}

#[test]
fn test_mapping_tool_result_preserves_call_id_and_function_name_without_http() {
    let request = GaiseInstructRequest {
        model: "gemini-3.5-flash".to_string(),
        input: OneOrMany::Many(vec![
            GaiseMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(vec![GaiseToolCall {
                    id: "call-123".to_string(),
                    r#type: "function".to_string(),
                    function: GaiseFunctionCall {
                        name: "get-weather".to_string(),
                        arguments: Some("{\"city\":\"London\"}".to_string()),
                    },
                    thought_signature: Some("opaque-tool-signature".to_string()),
                }]),
                tool_call_id: None,
                tool_name: None,
            },
            GaiseMessage {
                role: "tool".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text {
                    text: "{\"temperature\":18}".to_string(),
                })),
                tool_calls: None,
                tool_call_id: Some("call-123".to_string()),
                tool_name: Some("get-weather".to_string()),
            },
        ]),
        ..Default::default()
    };

    let mapped = GoogleInstructRequest::from(&request);
    assert_eq!(mapped.contents[0].role, "model");
    let call = mapped.contents[0].parts[0].tool_call.as_ref().unwrap();
    assert_eq!(call.id.as_deref(), Some("call-123"));
    assert_eq!(call.name, "get-weather");
    assert_eq!(
        mapped.contents[0].parts[0].thought_signature.as_deref(),
        Some("opaque-tool-signature")
    );

    assert_eq!(mapped.contents[1].role, "user");
    assert_eq!(mapped.contents[1].parts.len(), 1);
    let result = mapped.contents[1].parts[0].tool_response.as_ref().unwrap();
    assert_eq!(result.id.as_deref(), Some("call-123"));
    assert_eq!(result.name, "get-weather");
    assert_eq!(result.response["temperature"], 18);
}

#[test]
fn test_mapping_multimodal_tool_result_without_http() {
    let request = GaiseInstructRequest {
        model: "gemini-3.6-flash".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "tool".to_string(),
            content: Some(OneOrMany::Many(vec![
                GaiseContent::Text {
                    text: "{\"status\":\"ok\"}".to_string(),
                },
                GaiseContent::Parts {
                    parts: vec![
                        GaiseContent::Image {
                            data: vec![1, 2, 3],
                            format: Some("png".to_string()),
                        },
                        GaiseContent::File {
                            data: vec![4, 5, 6],
                            name: Some("report.pdf".to_string()),
                        },
                    ],
                },
            ])),
            tool_call_id: Some("call-123".to_string()),
            tool_name: Some("inspect_result".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let mapped = GoogleInstructRequest::from(&request);
    let response = mapped.contents[0].parts[0].tool_response.as_ref().unwrap();
    assert_eq!(response.response["status"], "ok");
    let media = response.parts.as_ref().unwrap();
    assert_eq!(media.len(), 2);
    assert_eq!(media[0].inline_data.mime_type, "image/png");
    assert_eq!(media[0].inline_data.display_name, "tool-result-1.png");
    assert_eq!(media[0].inline_data.data, "AQID");
    assert_eq!(media[1].inline_data.mime_type, "application/pdf");
    assert_eq!(media[1].inline_data.display_name, "report.pdf");

    let json = serde_json::to_value(mapped).unwrap();
    assert_eq!(
        json["contents"][0]["parts"][0]["functionResponse"]["parts"][0]["inlineData"]["mimeType"],
        "image/png"
    );
}

#[test]
fn test_mapping_nested_and_multiple_system_messages_without_http() {
    let request = GaiseInstructRequest {
        model: "gemini-3.5-flash".to_string(),
        input: OneOrMany::Many(vec![
            GaiseMessage {
                role: "system".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Parts {
                    parts: vec![
                        GaiseContent::Text {
                            text: "First".to_string(),
                        },
                        GaiseContent::Parts {
                            parts: vec![GaiseContent::Text {
                                text: "Second".to_string(),
                            }],
                        },
                    ],
                })),
                ..Default::default()
            },
            GaiseMessage {
                role: "system".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text {
                    text: "Third".to_string(),
                })),
                ..Default::default()
            },
            GaiseMessage {
                role: "user".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text {
                    text: "Hello".to_string(),
                })),
                ..Default::default()
            },
        ]),
        ..Default::default()
    };

    let mapped = GoogleInstructRequest::from(&request);
    let system = mapped.system_instruction.unwrap();
    assert_eq!(system.parts.len(), 3);
    assert_eq!(system.parts[0].text.as_deref(), Some("First"));
    assert_eq!(system.parts[1].text.as_deref(), Some("Second"));
    assert_eq!(system.parts[2].text.as_deref(), Some("Third"));
    assert_eq!(mapped.contents.len(), 1);
}
