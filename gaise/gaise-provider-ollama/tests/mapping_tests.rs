use gaise_core::contracts::{
    GaiseContent, GaiseGenerationConfig, GaiseInstructRequest,
    GaiseMessage, GaiseTool, GaiseToolParameter, OneOrMany,
    GaiseToolCall, GaiseFunctionCall
};
use gaise_provider_ollama::contracts::models::OllamaChatRequest;
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
        model: "llama3.1".to_string(),
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

    let ollama_request = OllamaChatRequest::from(&request);

    let tools = ollama_request.tools.expect("Missing tools");
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
        model: "llama3.1".to_string(),
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

    let ollama_request = OllamaChatRequest::from(&request);

    let tools = ollama_request.tools.expect("Missing tools");
    let prop = tools[0].function.parameters.properties.get("tasks").expect("Missing tasks property");
    assert_eq!(prop.r#type, "array");
    let items = prop.items.as_ref().expect("Missing items in array property");
    assert_eq!(items.r#type, "string");
}

#[test]
fn test_mapping_text_request() {
    let request = GaiseInstructRequest {
        model: "llama3.1".to_string(),
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

    let ollama_request = OllamaChatRequest::from(&request);

    assert_eq!(ollama_request.messages.len(), 1);
    assert_eq!(ollama_request.messages[0].role, "user");
    assert_eq!(ollama_request.messages[0].content, Some(String::from("Hello")));
    
    let options = ollama_request.options.expect("Missing options");
    assert_eq!(options.temperature, Some(0.7));
    assert_eq!(options.num_predict, Some(100));
}

#[test]
fn test_mapping_multimodal_request() {
    let request = GaiseInstructRequest {
        model: "llava".to_string(),
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

    let ollama_request = OllamaChatRequest::from(&request);

    assert_eq!(ollama_request.messages.len(), 1);
    assert_eq!(ollama_request.messages[0].content, Some("What is in this image?".to_string()));
    
    let images = ollama_request.messages[0].images.as_ref().expect("Missing images");
    assert_eq!(images.len(), 1);
    assert_eq!(images[0], "AQID"); // base64 for [1, 2, 3]
}

#[test]
fn test_mapping_tool_response_request() {
    let request = GaiseInstructRequest {
        model: "llama3.1".to_string(),
        input: OneOrMany::Many(vec![
            GaiseMessage {
                role: "user".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text { text: "What's the weather?".to_string() })),
                ..Default::default()
            },
            GaiseMessage {
                role: "assistant".to_string(),
                tool_calls: Some(vec![GaiseToolCall {
                    id: "123".to_string(),
                    r#type: "function".to_string(),
                    function: GaiseFunctionCall {
                        name: "get_weather".to_string(),
                        arguments: Some("{\"location\": \"London\"}".to_string()),
                    },
                }]),
                ..Default::default()
            }
        ]),
        ..Default::default()
    };

    let ollama_request = OllamaChatRequest::from(&request);
    assert_eq!(ollama_request.messages.len(), 2);
    assert_eq!(ollama_request.messages[1].role, "assistant");
    let tool_calls = ollama_request.messages[1].tool_calls.as_ref().expect("Missing tool_calls");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].function.name, "get_weather");
    assert_eq!(tool_calls[0].function.arguments.get("location").unwrap(), "London");
}
