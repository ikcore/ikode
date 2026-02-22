use gaise_core::contracts::{
    GaiseContent, GaiseGenerationConfig, GaiseInstructRequest,
    GaiseMessage, GaiseTool, GaiseToolParameter, OneOrMany,
};
use gaise_provider_anthropic::contracts::models::{AnthropicRequest, AnthropicContent};
use std::collections::HashMap;

#[test]
fn test_mapping_tool_request() {
    let mut properties = HashMap::new();
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
            content: Some(OneOrMany::One(GaiseContent::Text { text: "What's the weather like in Boston?".to_string() })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let anthropic_request = AnthropicRequest::from(&request);

    let tools = anthropic_request.tools.expect("Missing tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "get_current_weather");
    assert_eq!(
        tools[0].input_schema.r#type,
        "object"
    );
    assert!(tools[0].input_schema
        .properties
        .contains_key("location"));
}

#[test]
fn test_mapping_array_tool_request() {
    let mut properties = HashMap::new();
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
            content: Some(OneOrMany::One(GaiseContent::Text { text: "Add some tasks".to_string() })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let anthropic_request = AnthropicRequest::from(&request);

    let tools = anthropic_request.tools.expect("Missing tools");
    let prop = tools[0].input_schema.properties.get("tasks").expect("Missing tasks property");
    assert_eq!(prop.r#type, "array");
    let items = prop.items.as_ref().expect("Missing items in array property");
    assert_eq!(items.r#type, "string");
}

#[test]
fn test_mapping_text_request() {
    let request = GaiseInstructRequest {
        model: "claude-3-5-sonnet-20241022".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text { text: "Hello".to_string() })),
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

    if let AnthropicContent::Text(text) = &anthropic_request.messages[0].content {
        assert_eq!(text, "Hello");
    } else {
        panic!("Expected text content");
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
                content: Some(OneOrMany::One(GaiseContent::Text { text: "You are a helpful assistant.".to_string() })),
                ..Default::default()
            },
            GaiseMessage {
                role: "user".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text { text: "Hello".to_string() })),
                ..Default::default()
            },
        ]),
        ..Default::default()
    };

    let anthropic_request = AnthropicRequest::from(&request);

    assert_eq!(anthropic_request.system, Some("You are a helpful assistant.".to_string()));
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
                GaiseContent::Text { text: "What is in this image?".to_string() },
                GaiseContent::Image { data: vec![1, 2, 3], format: Some("image/png".to_string()) },
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
