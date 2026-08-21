use gaise_core::contracts::{
    GaiseContent, GaiseFunctionCall, GaiseGenerationConfig, GaiseInstructRequest, GaiseMessage,
    GaiseTool, GaiseToolCall, GaiseToolParameter, OneOrMany,
};
use gaise_provider_anthropic::contracts::models::{
    AnthropicContent, AnthropicContentBlock, AnthropicRequest,
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
        model: "claude-3-5-sonnet-20241022".to_string(),
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

    let anthropic_request = AnthropicRequest::from(&request);

    let tools = anthropic_request.tools.expect("Missing tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "get_current_weather");
    assert_eq!(tools[0].input_schema.r#type, "object");
    assert!(tools[0].input_schema.properties.contains_key("location"));
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
        model: "claude-3-5-sonnet-20241022".to_string(),
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

    let anthropic_request = AnthropicRequest::from(&request);

    let tools = anthropic_request.tools.expect("Missing tools");
    let prop = tools[0]
        .input_schema
        .properties
        .get("tasks")
        .expect("Missing tasks property");
    assert_eq!(prop.r#type, "array");
    let items = prop
        .items
        .as_ref()
        .expect("Missing items in array property");
    assert_eq!(items.r#type, "string");
}

#[test]
fn test_mapping_text_request() {
    let request = GaiseInstructRequest {
        model: "claude-3-5-sonnet-20241022".to_string(),
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

    let anthropic_request = AnthropicRequest::from(&request);

    assert_eq!(anthropic_request.messages.len(), 1);
    assert_eq!(anthropic_request.messages[0].role, "user");

    // The last message's content is promoted to a single text block carrying a cache
    // breakpoint, so the conversation prefix rolls into the cache each turn.
    if let AnthropicContent::Blocks(blocks) = &anthropic_request.messages[0].content {
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            AnthropicContentBlock::Text {
                text,
                cache_control,
            } => {
                assert_eq!(text, "Hello");
                assert_eq!(
                    cache_control.as_ref().map(|c| c.r#type.as_str()),
                    Some("ephemeral")
                );
            }
            _ => panic!("Expected a text block"),
        }
    } else {
        panic!("Expected blocks content");
    }

    assert_eq!(anthropic_request.temperature, Some(0.7));
    assert_eq!(anthropic_request.max_tokens, 100);
}

#[test]
fn test_mapping_system_message() {
    let request = GaiseInstructRequest {
        model: "claude-3-5-sonnet-20241022".to_string(),
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
                    text: "Hello".to_string(),
                })),
                ..Default::default()
            },
        ]),
        ..Default::default()
    };

    let anthropic_request = AnthropicRequest::from(&request);

    let system = anthropic_request.system.expect("Missing system");
    assert_eq!(system.len(), 1);
    assert_eq!(system[0].text, "You are a helpful assistant.");
    // The system block must carry a cache breakpoint so Anthropic caches the prefix.
    assert_eq!(
        system[0].cache_control.as_ref().map(|c| c.r#type.as_str()),
        Some("ephemeral")
    );
    assert_eq!(anthropic_request.messages.len(), 1);
    assert_eq!(anthropic_request.messages[0].role, "user");
}

#[test]
fn test_mapping_multimodal_request() {
    let request = GaiseInstructRequest {
        model: "claude-3-5-sonnet-20241022".to_string(),
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

    let anthropic_request = AnthropicRequest::from(&request);

    assert_eq!(anthropic_request.messages.len(), 1);

    if let AnthropicContent::Blocks(blocks) = &anthropic_request.messages[0].content {
        assert_eq!(blocks.len(), 2);
    } else {
        panic!("Expected blocks content");
    }
}

#[test]
fn test_mapping_thinking_effort() {
    let request = GaiseInstructRequest {
        model: "claude-sonnet-4-6".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Prove √2 is irrational".to_string(),
            })),
            ..Default::default()
        }),
        generation_config: Some(GaiseGenerationConfig {
            thinking_effort: Some("high".to_string()),
            max_tokens: Some(16000),
            ..Default::default()
        }),
        ..Default::default()
    };

    let anthropic_request = AnthropicRequest::from(&request);

    let thinking = anthropic_request.thinking.expect("Missing thinking");
    assert_eq!(thinking.r#type, "adaptive");
    assert_eq!(thinking.budget_tokens, None);
    assert_eq!(
        anthropic_request
            .output_config
            .expect("Missing output_config")
            .effort,
        "high"
    );
    assert_eq!(anthropic_request.max_tokens, 16000);
}

#[test]
fn test_mapping_thinking_with_budget() {
    let request = GaiseInstructRequest {
        model: "claude-sonnet-4-5-20250929".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Complex reasoning task".to_string(),
            })),
            ..Default::default()
        }),
        generation_config: Some(GaiseGenerationConfig {
            thinking_effort: Some("medium".to_string()),
            thinking_tokens: Some(10000),
            max_tokens: Some(16000),
            ..Default::default()
        }),
        ..Default::default()
    };

    let anthropic_request = AnthropicRequest::from(&request);

    let thinking = anthropic_request.thinking.expect("Missing thinking");
    assert_eq!(thinking.r#type, "enabled");
    assert_eq!(thinking.budget_tokens, Some(10000));
    // Sonnet 4.5 supports manual thinking budgets, but Anthropic's effort
    // parameter is only available on Opus 4.5 and newer adaptive families.
    assert!(anthropic_request.output_config.is_none());
}

#[test]
fn test_mapping_no_thinking() {
    let request = GaiseInstructRequest {
        model: "claude-sonnet-4-6".to_string(),
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

    let anthropic_request = AnthropicRequest::from(&request);

    assert!(anthropic_request.thinking.is_none());
    assert!(anthropic_request.output_config.is_none());
}

#[test]
fn test_mapping_pdf_document_uses_anthropic_document_block_without_http() {
    let request = GaiseInstructRequest {
        model: "claude-sonnet-4-5".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::Many(vec![
                GaiseContent::Text {
                    text: "Summarize this".to_string(),
                },
                GaiseContent::File {
                    data: vec![1, 2, 3],
                    name: Some("report.pdf".to_string()),
                },
            ])),
            ..Default::default()
        }),
        ..Default::default()
    };

    let mapped = AnthropicRequest::from(&request);
    let json = serde_json::to_value(mapped).unwrap();
    assert_eq!(json["messages"][0]["content"][1]["type"], "document");
    assert_eq!(
        json["messages"][0]["content"][1]["source"]["type"],
        "base64"
    );
    assert_eq!(
        json["messages"][0]["content"][1]["source"]["media_type"],
        "application/pdf"
    );
    assert_eq!(json["messages"][0]["content"][1]["source"]["data"], "AQID");
    assert_eq!(json["messages"][0]["content"][1]["title"], "report.pdf");
}

#[test]
fn test_mapping_all_supported_effort_values_without_http() {
    for effort in ["low", "medium", "high", "max"] {
        let request = GaiseInstructRequest {
            model: "claude-sonnet-4-6".to_string(),
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

        let mapped = AnthropicRequest::from(&request);
        assert_eq!(
            mapped
                .output_config
                .as_ref()
                .map(|config| config.effort.as_str()),
            Some(effort)
        );
    }
}

#[test]
fn test_mapping_thought_display_without_http() {
    for (include, expected) in [(true, "summarized"), (false, "omitted")] {
        let request = GaiseInstructRequest {
            model: "claude-opus-4-8".to_string(),
            generation_config: Some(GaiseGenerationConfig {
                include_thoughts: Some(include),
                ..Default::default()
            }),
            ..Default::default()
        };

        let mapped = AnthropicRequest::from(&request);
        let thinking = mapped.thinking.expect("Missing adaptive thinking config");
        assert_eq!(thinking.r#type, "adaptive");
        assert_eq!(thinking.display.as_deref(), Some(expected));
    }
}

#[test]
fn test_mapping_model_specific_sampling_rules_without_http() {
    let fixed_sampling = GaiseInstructRequest {
        model: "claude-opus-4-8".to_string(),
        generation_config: Some(GaiseGenerationConfig {
            temperature: Some(0.4),
            top_p: Some(0.8),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mapped = AnthropicRequest::from(&fixed_sampling);
    assert_eq!(mapped.temperature, None);
    assert_eq!(mapped.top_p, None);

    let fable_sampling = GaiseInstructRequest {
        model: "claude-fable-5".to_string(),
        generation_config: Some(GaiseGenerationConfig {
            temperature: Some(0.4),
            top_p: Some(0.8),
            top_k: Some(20),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mapped = AnthropicRequest::from(&fable_sampling);
    assert_eq!(mapped.temperature, None);
    assert_eq!(mapped.top_p, None);
    assert_eq!(mapped.top_k, None);

    // Opus 5 (2026-07-24) follows the Opus 4.7 rules: adaptive-only thinking
    // and no non-default temperature/top_p/top_k.
    let opus5_sampling = GaiseInstructRequest {
        model: "claude-opus-5".to_string(),
        generation_config: Some(GaiseGenerationConfig {
            temperature: Some(0.4),
            top_p: Some(0.8),
            top_k: Some(20),
            thinking_effort: Some("xhigh".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mapped = AnthropicRequest::from(&opus5_sampling);
    assert_eq!(mapped.temperature, None);
    assert_eq!(mapped.top_p, None);
    assert_eq!(mapped.top_k, None);
    let thinking =
        serde_json::to_value(mapped.thinking.as_ref().expect("adaptive thinking")).unwrap();
    assert_eq!(thinking["type"], "adaptive");
    assert_eq!(
        serde_json::to_value(mapped.output_config.as_ref().unwrap()).unwrap()["effort"],
        "xhigh"
    );

    let exclusive_sampling = GaiseInstructRequest {
        model: "claude-sonnet-4-5-20250929".to_string(),
        generation_config: Some(GaiseGenerationConfig {
            temperature: Some(0.4),
            top_p: Some(0.8),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mapped = AnthropicRequest::from(&exclusive_sampling);
    assert_eq!(mapped.temperature, Some(0.4));
    assert_eq!(mapped.top_p, None);

    let thinking_sampling = GaiseInstructRequest {
        model: "claude-sonnet-4-6".to_string(),
        generation_config: Some(GaiseGenerationConfig {
            thinking_effort: Some("high".to_string()),
            temperature: Some(0.4),
            top_p: Some(0.97),
            top_k: Some(20),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mapped = AnthropicRequest::from(&thinking_sampling);
    assert_eq!(mapped.temperature, None);
    assert_eq!(mapped.top_p, Some(0.97));
    assert_eq!(mapped.top_k, None);

    let legacy_sampling = GaiseInstructRequest {
        model: "claude-sonnet-4-5-20250929".to_string(),
        generation_config: Some(GaiseGenerationConfig {
            top_k: Some(20),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(AnthropicRequest::from(&legacy_sampling).top_k, Some(20));
}

#[test]
fn test_mapping_multimodal_tool_result_uses_user_role_without_http() {
    let request = GaiseInstructRequest {
        model: "claude-opus-4-8".to_string(),
        input: OneOrMany::Many(vec![
            GaiseMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(vec![GaiseToolCall {
                    id: "toolu-123".to_string(),
                    r#type: "function".to_string(),
                    function: GaiseFunctionCall {
                        name: "inspect-image".to_string(),
                        arguments: Some("{}".to_string()),
                    },
                    thought_signature: None,
                }]),
                ..Default::default()
            },
            GaiseMessage {
                role: "tool".to_string(),
                content: Some(OneOrMany::Many(vec![
                    GaiseContent::Text {
                        text: "inspection result".to_string(),
                    },
                    GaiseContent::Image {
                        data: vec![1, 2, 3],
                        format: Some("image/png".to_string()),
                    },
                ])),
                tool_call_id: Some("toolu-123".to_string()),
                tool_name: Some("inspect-image".to_string()),
                ..Default::default()
            },
        ]),
        ..Default::default()
    };

    let mapped = AnthropicRequest::from(&request);
    assert_eq!(mapped.messages[1].role, "user");
    let json = serde_json::to_value(mapped).unwrap();
    assert_eq!(json["messages"][1]["content"][0]["type"], "tool_result");
    assert_eq!(
        json["messages"][1]["content"][0]["tool_use_id"],
        "toolu-123"
    );
    assert_eq!(
        json["messages"][1]["content"][0]["content"][0]["type"],
        "text"
    );
    assert_eq!(
        json["messages"][1]["content"][0]["content"][1]["type"],
        "image"
    );
}

#[test]
fn test_mapping_redacted_reasoning_is_replayed_verbatim_without_http() {
    let request = GaiseInstructRequest {
        model: "claude-opus-4-8".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "assistant".to_string(),
            content: Some(OneOrMany::One(GaiseContent::RedactedReasoning {
                data: b"opaque-redacted-block".to_vec(),
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let mapped = AnthropicRequest::from(&request);
    let json = serde_json::to_value(mapped).unwrap();
    assert_eq!(
        json["messages"][0]["content"][0]["type"],
        "redacted_thinking"
    );
    assert_eq!(
        json["messages"][0]["content"][0]["data"],
        "opaque-redacted-block"
    );
}
