use gaise_core::contracts::{
    GaiseContent, GaiseFunctionCall, GaiseGenerationConfig, GaiseImageConfig, GaiseInstructRequest,
    GaiseMessage, GaiseTool, GaiseToolCall, GaiseToolParameter, OneOrMany,
};
use gaise_provider_gemini::contracts::models::GeminiRequest;
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
        model: "gemini-2.5-flash".to_string(),
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

    let gemini_request = GeminiRequest::from(&request);

    let tools = gemini_request.tools.expect("Missing tools");
    assert_eq!(tools.len(), 1);
    let decls = &tools[0].function_declarations;
    assert_eq!(decls.len(), 1);
    assert_eq!(decls[0].name, "get_current_weather");
    assert_eq!(
        decls[0].description.as_deref(),
        Some("Get the current weather in a given location")
    );

    let params = decls[0].parameters.as_ref().expect("Missing parameters");
    assert_eq!(params["type"], "object");
    assert!(params["properties"]["location"].is_object());
    assert_eq!(params["properties"]["location"]["type"], "string");
}

#[test]
fn test_mapping_current_image_generation_and_input_resolution_shape() {
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
            response_modalities: Some(vec!["text".to_string(), "image".to_string()]),
            image_config: Some(GaiseImageConfig {
                aspect_ratio: Some("16:9".to_string()),
                image_size: Some("2K".to_string()),
            }),
            input_media_resolution: Some("medium".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let mapped = GeminiRequest::from(&request);
    let config = mapped.generation_config.expect("missing generation config");
    assert_eq!(
        config.response_modalities,
        Some(vec!["TEXT".to_string(), "IMAGE".to_string()])
    );
    assert!(config.image_config.is_none());
    let image = config
        .response_format
        .expect("missing response format")
        .image
        .expect("missing image response config");
    assert_eq!(image.aspect_ratio.as_deref(), Some("16:9"));
    assert_eq!(image.image_size.as_deref(), Some("2K"));
    assert_eq!(
        config.media_resolution.as_deref(),
        Some("MEDIA_RESOLUTION_MEDIUM")
    );
}

#[test]
fn test_mapping_array_tool_request() {
    let mut properties = BTreeMap::new();
    properties.insert(
        "tasks".to_string(),
        GaiseToolParameter {
            r#type: Some("array".to_string()),
            description: Some("Array of tasks".to_string()),
            items: Some(Box::new(GaiseToolParameter {
                r#type: Some("string".to_string()),
                ..Default::default()
            })),
            ..Default::default()
        },
    );

    let request = GaiseInstructRequest {
        model: "gemini-2.5-flash".to_string(),
        tools: Some(vec![GaiseTool {
            name: "todo_add".to_string(),
            description: Some("Add tasks".to_string()),
            parameters: Some(GaiseToolParameter {
                r#type: Some("object".to_string()),
                properties: Some(properties),
                required: Some(vec!["tasks".to_string()]),
                ..Default::default()
            }),
        }]),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Add some tasks".to_string(),
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let gemini_request = GeminiRequest::from(&request);

    let tools = gemini_request.tools.expect("Missing tools");
    let params = tools[0].function_declarations[0]
        .parameters
        .as_ref()
        .expect("Missing params");
    let tasks = &params["properties"]["tasks"];
    assert_eq!(tasks["type"], "array");
    assert_eq!(tasks["items"]["type"], "string");
}

#[test]
fn test_mapping_text_request() {
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
            temperature: Some(0.7),
            max_tokens: Some(100),
            ..Default::default()
        }),
        ..Default::default()
    };

    let gemini_request = GeminiRequest::from(&request);

    assert_eq!(gemini_request.contents.len(), 1);
    assert_eq!(gemini_request.contents[0].role.as_deref(), Some("user"));
    assert_eq!(gemini_request.contents[0].parts.len(), 1);
    assert_eq!(
        gemini_request.contents[0].parts[0].text.as_deref(),
        Some("Hello")
    );

    let gen_config = gemini_request
        .generation_config
        .expect("Missing generation_config");
    assert_eq!(gen_config.temperature, Some(0.7));
    assert_eq!(gen_config.max_output_tokens, Some(100));
}

#[test]
fn test_mapping_system_message_extraction() {
    let request = GaiseInstructRequest {
        model: "gemini-2.5-flash".to_string(),
        input: OneOrMany::Many(vec![
            GaiseMessage {
                role: "system".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Parts {
                    parts: vec![
                        GaiseContent::Text {
                            text: "You are helpful.".to_string(),
                        },
                        GaiseContent::Parts {
                            parts: vec![GaiseContent::Text {
                                text: "Be concise.".to_string(),
                            }],
                        },
                    ],
                })),
                ..Default::default()
            },
            GaiseMessage {
                role: "system".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text {
                    text: "Use British English.".to_string(),
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

    let gemini_request = GeminiRequest::from(&request);

    // System message extracted to systemInstruction
    let sys = gemini_request
        .system_instruction
        .expect("Missing system_instruction");
    assert_eq!(sys.parts.len(), 3);
    assert_eq!(sys.parts[0].text.as_deref(), Some("You are helpful."));
    assert_eq!(sys.parts[1].text.as_deref(), Some("Be concise."));
    assert_eq!(sys.parts[2].text.as_deref(), Some("Use British English."));

    // Only user message in contents
    assert_eq!(gemini_request.contents.len(), 1);
    assert_eq!(gemini_request.contents[0].role.as_deref(), Some("user"));
}

#[test]
fn test_mapping_multimodal_request() {
    let request = GaiseInstructRequest {
        model: "gemini-2.5-flash".to_string(),
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

    let gemini_request = GeminiRequest::from(&request);

    assert_eq!(gemini_request.contents.len(), 1);
    assert_eq!(gemini_request.contents[0].parts.len(), 2);

    // First part: text
    assert_eq!(
        gemini_request.contents[0].parts[0].text.as_deref(),
        Some("What is in this image?")
    );

    // Second part: inline_data with base64
    let inline = gemini_request.contents[0].parts[1]
        .inline_data
        .as_ref()
        .expect("Missing inline_data");
    assert_eq!(inline.mime_type, "image/png");
    assert_eq!(inline.data, "AQID"); // base64 of [1, 2, 3]
}

#[test]
fn test_mapping_tool_response_request() {
    let request = GaiseInstructRequest {
        model: "gemini-2.5-flash".to_string(),
        input: OneOrMany::Many(vec![
            GaiseMessage {
                role: "user".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text {
                    text: "What's the weather?".to_string(),
                })),
                ..Default::default()
            },
            GaiseMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(vec![GaiseToolCall {
                    id: "call_123".to_string(),
                    r#type: "function".to_string(),
                    function: GaiseFunctionCall {
                        name: "get_weather".to_string(),
                        arguments: Some("{\"location\": \"London\"}".to_string()),
                    },
                    thought_signature: None,
                }]),
                tool_call_id: None,
                tool_name: None,
            },
            GaiseMessage {
                role: "tool".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text {
                    text: "{\"temp\": 15}".to_string(),
                })),
                tool_call_id: Some("call_123".to_string()),
                tool_name: Some("get_weather".to_string()),
                ..Default::default()
            },
        ]),
        ..Default::default()
    };

    let gemini_request = GeminiRequest::from(&request);

    assert_eq!(gemini_request.contents.len(), 3);

    // User message
    assert_eq!(gemini_request.contents[0].role.as_deref(), Some("user"));

    // Assistant → model with functionCall
    assert_eq!(gemini_request.contents[1].role.as_deref(), Some("model"));
    let fc = gemini_request.contents[1].parts[0]
        .function_call
        .as_ref()
        .expect("Missing function_call");
    assert_eq!(fc.name, "get_weather");
    assert_eq!(fc.id.as_deref(), Some("call_123"));

    // Tool → user with functionResponse
    assert_eq!(gemini_request.contents[2].role.as_deref(), Some("user"));
    let fr = gemini_request.contents[2].parts[0]
        .function_response
        .as_ref()
        .expect("Missing function_response");
    assert_eq!(fr.name, "get_weather");
    assert_eq!(fr.id.as_deref(), Some("call_123"));
    assert_eq!(fr.response["temp"], 15);
}

#[test]
fn test_mapping_multimodal_tool_response_without_http() {
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

    let mapped = GeminiRequest::from(&request);
    let response = mapped.contents[0].parts[0]
        .function_response
        .as_ref()
        .unwrap();
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
fn test_tool_name_preserves_provider_supported_punctuation() {
    let request = GaiseInstructRequest {
        model: "gemini-2.5-flash".to_string(),
        tools: Some(vec![GaiseTool {
            name: "get-current-weather".to_string(),
            description: Some("Get weather".to_string()),
            parameters: None,
        }]),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Weather?".to_string(),
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let gemini_request = GeminiRequest::from(&request);

    let tools = gemini_request.tools.expect("Missing tools");
    assert_eq!(
        tools[0].function_declarations[0].name,
        "get-current-weather"
    );
}

#[test]
fn test_mapping_role_conversion() {
    let request = GaiseInstructRequest {
        model: "gemini-2.5-flash".to_string(),
        input: OneOrMany::Many(vec![
            GaiseMessage {
                role: "user".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text {
                    text: "Hi".to_string(),
                })),
                ..Default::default()
            },
            GaiseMessage {
                role: "assistant".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text {
                    text: "Hello!".to_string(),
                })),
                ..Default::default()
            },
        ]),
        ..Default::default()
    };

    let gemini_request = GeminiRequest::from(&request);

    assert_eq!(gemini_request.contents[0].role.as_deref(), Some("user"));
    assert_eq!(gemini_request.contents[1].role.as_deref(), Some("model"));
}

#[test]
fn test_mapping_thinking_effort() {
    let request = GaiseInstructRequest {
        model: "gemini-3-flash-preview".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Prove √2 is irrational".to_string(),
            })),
            ..Default::default()
        }),
        generation_config: Some(GaiseGenerationConfig {
            thinking_effort: Some("high".to_string()),
            max_tokens: Some(32000),
            ..Default::default()
        }),
        ..Default::default()
    };

    let gemini_request = GeminiRequest::from(&request);

    let gen_config = gemini_request
        .generation_config
        .expect("Missing generation_config");
    assert_eq!(gen_config.max_output_tokens, Some(32000));

    let thinking = gen_config.thinking_config.expect("Missing thinking_config");
    assert_eq!(thinking.thinking_level, Some("HIGH".to_string()));
    assert_eq!(thinking.include_thoughts, Some(true));
}

#[test]
fn test_mapping_all_supported_thinking_levels_without_http() {
    for (effort, expected) in [("low", "LOW"), ("medium", "MEDIUM"), ("high", "HIGH")] {
        let request = GaiseInstructRequest {
            model: "gemini-3-flash-preview".to_string(),
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
        let mapped = GeminiRequest::from(&request);
        let thinking = mapped.generation_config.unwrap().thinking_config.unwrap();
        assert_eq!(thinking.thinking_level.as_deref(), Some(expected));
        assert_eq!(thinking.include_thoughts, Some(true));
    }
}

#[test]
fn test_mapping_thinking_with_budget() {
    let request = GaiseInstructRequest {
        model: "gemini-2.5-flash".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Complex task".to_string(),
            })),
            ..Default::default()
        }),
        generation_config: Some(GaiseGenerationConfig {
            thinking_effort: Some("medium".to_string()),
            thinking_tokens: Some(8192),
            max_tokens: Some(16000),
            ..Default::default()
        }),
        ..Default::default()
    };

    let gemini_request = GeminiRequest::from(&request);

    let gen_config = gemini_request
        .generation_config
        .expect("Missing generation_config");
    let thinking = gen_config.thinking_config.expect("Missing thinking_config");
    // Gemini 2.5 uses a numeric thinking budget. Thinking levels are a
    // Gemini 3.x+ control and must not be mixed into the same request.
    assert_eq!(thinking.thinking_level, None);
    assert_eq!(thinking.thinking_budget, Some(8192));
    assert_eq!(thinking.include_thoughts, Some(true));
}

#[test]
fn test_mapping_thinking_tokens_only() {
    let request = GaiseInstructRequest {
        model: "gemini-2.5-pro".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Task".to_string(),
            })),
            ..Default::default()
        }),
        generation_config: Some(GaiseGenerationConfig {
            thinking_tokens: Some(4096),
            ..Default::default()
        }),
        ..Default::default()
    };

    let gemini_request = GeminiRequest::from(&request);

    let gen_config = gemini_request
        .generation_config
        .expect("Missing generation_config");
    let thinking = gen_config.thinking_config.expect("Missing thinking_config");
    assert_eq!(thinking.thinking_budget, Some(4096));
    assert_eq!(thinking.thinking_level, None);
}

#[test]
fn test_mapping_no_thinking() {
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
            max_tokens: Some(1000),
            ..Default::default()
        }),
        ..Default::default()
    };

    let gemini_request = GeminiRequest::from(&request);

    let gen_config = gemini_request
        .generation_config
        .expect("Missing generation_config");
    assert!(gen_config.thinking_config.is_none());
}

#[test]
fn test_mapping_pdf_and_nested_image_parts_without_http() {
    let request = GaiseInstructRequest {
        model: "gemini-2.5-pro".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Parts {
                parts: vec![
                    GaiseContent::Text {
                        text: "Inspect both".to_string(),
                    },
                    GaiseContent::File {
                        data: vec![1, 2, 3],
                        name: Some("report.PDF".to_string()),
                    },
                    GaiseContent::Image {
                        data: vec![4, 5, 6],
                        format: Some("image/png".to_string()),
                    },
                ],
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let mapped = GeminiRequest::from(&request);
    assert_eq!(mapped.contents[0].parts.len(), 3);
    let document = mapped.contents[0].parts[1].inline_data.as_ref().unwrap();
    assert_eq!(document.mime_type, "application/pdf");
    assert_eq!(document.data, "AQID");
    let image = mapped.contents[0].parts[2].inline_data.as_ref().unwrap();
    assert_eq!(image.mime_type, "image/png");
    assert_eq!(image.data, "BAUG");

    let json = serde_json::to_value(mapped).unwrap();
    assert_eq!(
        json["contents"][0]["parts"][1]["inlineData"]["mimeType"],
        "application/pdf"
    );
}
