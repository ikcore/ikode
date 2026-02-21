use gaise_core::contracts::{
    GaiseContent, GaiseGenerationConfig, GaiseInstructRequest,
    GaiseMessage, GaiseTool, GaiseToolParameter, OneOrMany,
};
use gaise_provider_vertexai::contracts::models::GoogleInstructRequest;
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
            content: Some(OneOrMany::One(GaiseContent::Text { text: "What's the weather like in Boston?".to_string() })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let google_request = GoogleInstructRequest::from(&request);

    let tools = google_request.tools.expect("Missing tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].function_declarations.len(), 1);
    assert_eq!(tools[0].function_declarations[0].name, "get_current_weather");
    assert_eq!(
        tools[0].function_declarations[0].parameters.r#type,
        "object"
    );
    assert!(tools[0].function_declarations[0]
        .parameters
        .properties
        .as_ref()
        .unwrap()
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
        model: "gemini-1.5-pro".to_string(),
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

    let google_request = GoogleInstructRequest::from(&request);

    let tools = google_request.tools.expect("Missing tools");
    let prop = tools[0].function_declarations[0].parameters.properties.as_ref().unwrap().get("tasks").expect("Missing tasks property");
    assert_eq!(prop.r#type, "array");
    let items = prop.items.as_ref().expect("Missing items in array property");
    assert_eq!(items.r#type, "string");
}

#[test]
fn test_mapping_text_request() {
    let request = GaiseInstructRequest {
        model: "gemini-1.5-pro".to_string(),
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

    let google_request = GoogleInstructRequest::from(&request);

    assert_eq!(google_request.contents.len(), 1);
    assert_eq!(google_request.contents[0].role, "user");
    assert_eq!(google_request.contents[0].parts.len(), 1);
    assert_eq!(google_request.contents[0].parts[0].text, Some("Hello".to_string()));
    
    let gen_config = google_request.generation_config.expect("Missing generation config");
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
                GaiseContent::Text { text: "What is in this image?".to_string() },
                GaiseContent::Image { data: vec![1, 2, 3], format: Some("image/png".to_string()) },
            ])),
            ..Default::default()
        }),
        ..Default::default()
    };

    let google_request = GoogleInstructRequest::from(&request);

    assert_eq!(google_request.contents.len(), 1);
    assert_eq!(google_request.contents[0].parts.len(), 2);
    
    assert_eq!(google_request.contents[0].parts[0].text, Some("What is in this image?".to_string()));
    
    let image_part = &google_request.contents[0].parts[1];
    assert!(image_part.text.is_none());
    let inline_data = image_part.inline_data.as_ref().expect("Missing inline data");
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
                GaiseContent::Text { text: "can you take this image".to_string() },
                GaiseContent::Image { data: vec![1, 2, 3], format: Some("image/png".to_string()) },
                GaiseContent::Text { text: "and make it look like this other images style".to_string() },
                GaiseContent::Image { data: vec![4, 5, 6], format: Some("image/jpeg".to_string()) },
            ])),
            ..Default::default()
        }),
        ..Default::default()
    };

    let google_request = GoogleInstructRequest::from(&request);

    assert_eq!(google_request.contents.len(), 1);
    assert_eq!(google_request.contents[0].parts.len(), 4);
    
    assert_eq!(google_request.contents[0].parts[0].text, Some("can you take this image".to_string()));
    assert_eq!(google_request.contents[0].parts[1].inline_data.as_ref().unwrap().data, "AQID");
    assert_eq!(google_request.contents[0].parts[2].text, Some("and make it look like this other images style".to_string()));
    assert_eq!(google_request.contents[0].parts[3].inline_data.as_ref().unwrap().data, "BAUG");
}

#[test]
fn test_mapping_nested_parts_request() {
    let request = GaiseInstructRequest {
        model: "gemini-1.5-flash".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Parts {
                parts: vec![
                    GaiseContent::Text { text: "Combined parts:".to_string() },
                    GaiseContent::Image { data: vec![1, 1, 1], format: Some("image/png".to_string()) },
                ]
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let google_request = GoogleInstructRequest::from(&request);

    assert_eq!(google_request.contents.len(), 1);
    assert_eq!(google_request.contents[0].parts.len(), 2);
    assert_eq!(google_request.contents[0].parts[0].text, Some("Combined parts:".to_string()));
    assert_eq!(google_request.contents[0].parts[1].inline_data.as_ref().unwrap().data, "AQEB");
}

#[test]
fn test_mapping_system_instruction() {
    let request = GaiseInstructRequest {
        model: "gemini-1.5-pro".to_string(),
        input: OneOrMany::Many(vec![
            GaiseMessage {
                role: "system".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text { text: "You are a helpful assistant.".to_string() })),
                ..Default::default()
            },
            GaiseMessage {
                role: "user".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text { text: "Hi".to_string() })),
                ..Default::default()
            },
        ]),
        ..Default::default()
    };

    let google_request = GoogleInstructRequest::from(&request);

    let system_instruction = google_request.system_instruction.expect("Missing system instruction");
    assert_eq!(system_instruction.parts[0].text, Some("You are a helpful assistant.".to_string()));
    
    assert_eq!(google_request.contents.len(), 1);
    assert_eq!(google_request.contents[0].role, "user");
    assert_eq!(google_request.contents[0].parts[0].text, Some("Hi".to_string()));
}
