use gaise_core::contracts::{
    GaiseContent, GaiseGenerationConfig, GaiseInstructRequest,
    GaiseMessage, GaiseTool, GaiseToolParameter, OneOrMany,
    GaiseToolCall, GaiseFunctionCall
};
use gaise_provider_openai::contracts::models::{OpenAIChatRequest, OpenAIContent, OpenAIContentPart};
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
        model: "gpt-4o".to_string(),
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

    let openai_request = OpenAIChatRequest::from(&request);

    let tools = openai_request.tools.expect("Missing tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].function.name, "get_current_weather");
    assert_eq!(
        tools[0].function.parameters.r#type,
        "object"
    );
    assert!(tools[0].function
        .parameters
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
        model: "gpt-4o".to_string(),
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

    let openai_request = OpenAIChatRequest::from(&request);

    let tools = openai_request.tools.expect("Missing tools");
    let prop = tools[0].function.parameters.properties.get("tasks").expect("Missing tasks property");
    assert_eq!(prop.r#type, "array");
    let items = prop.items.as_ref().expect("Missing items in array property");
    assert_eq!(items.r#type, "string");
}

#[test]
fn test_mapping_text_request() {
    let request = GaiseInstructRequest {
        model: "gpt-4o".to_string(),
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

    let openai_request = OpenAIChatRequest::from(&request);

    assert_eq!(openai_request.messages.len(), 1);
    assert_eq!(openai_request.messages[0].role, "user");
    
    if let Some(OpenAIContent::Text(text)) = &openai_request.messages[0].content {
        assert_eq!(text, "Hello");
    } else {
        panic!("Expected text content");
    }
    
    assert_eq!(openai_request.temperature, Some(0.7));
    assert_eq!(openai_request.max_tokens, Some(100));
}

#[test]
fn test_mapping_cache_key() {
    let request = GaiseInstructRequest {
        model: "gpt-4o".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text { text: "Hello".to_string() })),
            ..Default::default()
        }),
        generation_config: Some(GaiseGenerationConfig {
            cache_key: Some("test-cache-key".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let openai_request = OpenAIChatRequest::from(&request);

    assert_eq!(openai_request.prompt_cache_key, Some("test-cache-key".to_string()));
}

#[test]
fn test_mapping_multimodal_request() {
    let request = GaiseInstructRequest {
        model: "gpt-4o".to_string(),
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

    let openai_request = OpenAIChatRequest::from(&request);

    assert_eq!(openai_request.messages.len(), 1);
    
    if let Some(OpenAIContent::Parts(parts)) = &openai_request.messages[0].content {
        assert_eq!(parts.len(), 2);
        match &parts[0] {
            OpenAIContentPart::Text { text } => assert_eq!(text, "What is in this image?"),
            _ => panic!("Expected text part"),
        }
        match &parts[1] {
            OpenAIContentPart::ImageUrl { image_url } => {
                assert!(image_url.url.contains("data:image/png;base64,"));
                assert!(image_url.url.contains("AQID"));
            },
            _ => panic!("Expected image part"),
        }
    } else {
        panic!("Expected parts content");
    }
}

#[test]
fn test_mapping_tool_response_request() {
    let request = GaiseInstructRequest {
        model: "gpt-4o".to_string(),
        input: OneOrMany::Many(vec![
            GaiseMessage {
                role: "user".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text { text: "What's the weather?".to_string() })),
                ..Default::default()
            },
            GaiseMessage {
                role: "assistant".to_string(),
                tool_calls: Some(vec![GaiseToolCall {
                    id: "call_123".to_string(),
                    r#type: "function".to_string(),
                    function: GaiseFunctionCall {
                        name: "get_weather".to_string(),
                        arguments: Some("{\"location\": \"London\"}".to_string()),
                    },
                }]),
                ..Default::default()
            },
            GaiseMessage {
                role: "tool".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text { text: "{\"temp\": 15}".to_string() })),
                tool_call_id: Some("call_123".to_string()),
                ..Default::default()
            }
        ]),
        ..Default::default()
    };

    let openai_request = OpenAIChatRequest::from(&request);
    assert_eq!(openai_request.messages.len(), 3);
    assert_eq!(openai_request.messages[1].role, "assistant");
    let tool_calls = openai_request.messages[1].tool_calls.as_ref().expect("Missing tool_calls");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].id, "call_123");
    assert_eq!(tool_calls[0].function.name, "get_weather");
    assert_eq!(tool_calls[0].function.arguments, "{\"location\": \"London\"}");

    assert_eq!(openai_request.messages[2].role, "tool");
    assert_eq!(openai_request.messages[2].tool_call_id, Some("call_123".to_string()));
}
