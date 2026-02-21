use gaise_core::GaiseClient;
use gaise_core::contracts::{GaiseContent, GaiseInstructRequest, GaiseMessage, OneOrMany};
use gaise_provider_ollama::ollama_client::GaiseClientOllama;

#[tokio::test]
#[ignore] // Ignore by default as it requires a running Ollama instance and the specific model
async fn test_ollama_instruct_gpt_oss() {
    let client = GaiseClientOllama::new("http://localhost:11434".to_string());
    
    let request = GaiseInstructRequest {
        model: "gpt-oss:20b".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Explain quantum entanglement in one sentence.".to_string(),
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let response = client.instruct(&request).await;
    
    match response {
        Ok(res) => {
            println!("Response: {:?}", res);
            match res.output {
                OneOrMany::One(msg) => {
                    assert_eq!(msg.role, "assistant");
                    assert!(msg.content.is_some());
                }
                _ => panic!("Expected single message response"),
            }
        }
        Err(e) => {
            // If the model is not found or Ollama is not running, this will fail.
            // In a real CI we might skip this, but here we want to show it works.
            panic!("Failed to call Ollama: {}", e);
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_ollama_instruct_stream_gpt_oss() {
    use futures_util::StreamExt;
    
    let client = GaiseClientOllama::new("http://localhost:11434".to_string());
    
    let request = GaiseInstructRequest {
        model: "gpt-oss:20b".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Count from 1 to 5.".to_string(),
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let stream_res = client.instruct_stream(&request).await;
    assert!(stream_res.is_ok(), "Failed to start stream: {:?}", stream_res.err());
    
    let mut stream = stream_res.unwrap();
    let mut full_text = String::new();
    
    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.expect("Stream chunk error");
        if let gaise_core::contracts::GaiseStreamChunk::Text(t) = chunk.chunk {
            full_text.push_str(&t);
        }
    }
    
    assert!(!full_text.is_empty());
    println!("Streamed text: {}", full_text);
}

#[tokio::test]
#[ignore]
async fn test_ollama_tool_call_gpt_oss() {
    use gaise_core::contracts::{GaiseTool, GaiseToolParameter};
    use std::collections::HashMap;

    let client = GaiseClientOllama::new("http://localhost:11434".to_string());

    let mut properties = HashMap::new();
    properties.insert(
        "location".to_string(),
        GaiseToolParameter {
            r#type: Some("string".to_string()),
            description: Some("The city and state, e.g. San Francisco, CA".to_string()),
            ..Default::default()
        },
    );

    let tools = vec![GaiseTool {
        name: "get_current_weather".to_string(),
        description: Some("Get the current weather in a given location".to_string()),
        parameters: Some(GaiseToolParameter {
            r#type: Some("object".to_string()),
            properties: Some(properties),
            required: Some(vec!["location".to_string()]),
            ..Default::default()
        }),
    }];

    let request = GaiseInstructRequest {
        model: "gpt-oss:20b".to_string(),
        tools: Some(tools),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "What's the weather like in Boston?".to_string(),
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let response = client.instruct(&request).await;

    match response {
        Ok(res) => {
            println!("Response: {:?}", res);
            match res.output {
                OneOrMany::One(msg) => {
                    assert_eq!(msg.role, "assistant");
                    let tool_calls = msg.tool_calls.expect("Expected tool calls in response");
                    assert!(!tool_calls.is_empty());
                    assert_eq!(tool_calls[0].function.name, "get_current_weather");
                    assert!(tool_calls[0].function.arguments.is_some());
                }
                _ => panic!("Expected single message response"),
            }
        }
        Err(e) => {
            panic!("Failed to call Ollama: {}", e);
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_ollama_tool_call_stream_gpt_oss() {
    use gaise_core::contracts::{GaiseTool, GaiseToolParameter, GaiseStreamAccumulator};
    use std::collections::HashMap;

    let client = GaiseClientOllama::new("http://localhost:11434".to_string());

    let mut properties = HashMap::new();
    properties.insert(
        "location".to_string(),
        GaiseToolParameter {
            r#type: Some("string".to_string()),
            description: Some("The city and state, e.g. San Francisco, CA".to_string()),
            ..Default::default()
        },
    );

    let tools = vec![GaiseTool {
        name: "get_current_weather".to_string(),
        description: Some("Get the current weather in a given location".to_string()),
        parameters: Some(GaiseToolParameter {
            r#type: Some("object".to_string()),
            properties: Some(properties),
            required: Some(vec!["location".to_string()]),
            ..Default::default()
        }),
    }];

    let request = GaiseInstructRequest {
        model: "gpt-oss:20b".to_string(),
        tools: Some(tools),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "What's the weather like in Boston?".to_string(),
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let stream_res = client.instruct_stream(&request).await;
    assert!(stream_res.is_ok(), "Failed to start stream: {:?}", stream_res.err());

    let stream = stream_res.unwrap();
    let message = GaiseStreamAccumulator::collect(stream).await.expect("Failed to collect stream");

    assert_eq!(message.role, "assistant");
    let tool_calls = message.tool_calls.as_ref().expect("Expected tool calls in streamed response");
    assert!(!tool_calls.is_empty());
    assert_eq!(tool_calls[0].function.name, "get_current_weather");
    assert!(tool_calls[0].function.arguments.is_some());
    println!("Streamed tool call message: {:?}", message);
}
