#[cfg(test)]
mod tests {
    use gaise_core::contracts::{
        GaiseContent, GaiseInstructStreamResponse, GaiseStreamAccumulator, GaiseStreamChunk,
        GaiseUsage, OneOrMany,
    };
    use std::collections::HashMap;

    #[test]
    fn test_accumulation_text() {
        let mut acc = GaiseStreamAccumulator::new();

        acc.push(&GaiseInstructStreamResponse {
            chunk: GaiseStreamChunk::Text("Hello ".to_string()),
            external_id: Some("ext-1".to_string()),
        });
        acc.push(&GaiseInstructStreamResponse {
            chunk: GaiseStreamChunk::Text("world!".to_string()),
            external_id: Some("ext-1".to_string()),
        });

        let msg = acc.finish();
        assert_eq!(msg.role, "assistant");

        if let Some(gaise_core::contracts::OneOrMany::One(
            gaise_core::contracts::GaiseContent::Text { text },
        )) = msg.content
        {
            assert_eq!(text, "Hello world!");
        } else {
            panic!("Expected text content");
        }
    }

    #[test]
    fn test_accumulation_tool_calls() {
        let mut acc = GaiseStreamAccumulator::new();

        acc.push(&GaiseInstructStreamResponse {
            chunk: GaiseStreamChunk::ToolCall {
                index: 0,
                id: Some("call_1".to_string()),
                name: Some("get_weather".to_string()),
                arguments: Some("{\"loc".to_string()),
                thought_signature: None,
            },
            external_id: None,
        });

        acc.push(&GaiseInstructStreamResponse {
            chunk: GaiseStreamChunk::ToolCall {
                index: 0,
                id: None,
                name: None,
                arguments: Some("ation\": \"London\"}".to_string()),
                thought_signature: None,
            },
            external_id: None,
        });

        let msg = acc.finish();
        let tool_calls = msg.tool_calls.expect("Expected tool calls");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].function.name, "get_weather");
        assert_eq!(
            tool_calls[0].function.arguments.as_deref(),
            Some("{\"location\": \"London\"}")
        );
    }

    #[test]
    fn test_accumulation_usage() {
        let mut acc = GaiseStreamAccumulator::new();

        let mut input_usage = HashMap::new();
        input_usage.insert("prompt".to_string(), 10);

        acc.push(&GaiseInstructStreamResponse {
            chunk: GaiseStreamChunk::Usage(GaiseUsage {
                input: Some(input_usage),
                output: None,
                total: None,
            }),
            external_id: None,
        });

        let mut output_usage = HashMap::new();
        output_usage.insert("completion".to_string(), 5);

        acc.push(&GaiseInstructStreamResponse {
            chunk: GaiseStreamChunk::Usage(GaiseUsage {
                input: None,
                output: Some(output_usage),
                total: Some(HashMap::from([("total_tokens".to_string(), 15)])),
            }),
            external_id: None,
        });

        assert!(acc.usage.is_some());
        let usage = acc.usage.as_ref().unwrap();
        assert_eq!(usage.input.as_ref().unwrap().get("prompt"), Some(&10));
        assert_eq!(usage.output.as_ref().unwrap().get("completion"), Some(&5));
        assert_eq!(usage.total.as_ref().unwrap().get("total_tokens"), Some(&15));

        // Usage events carry cumulative snapshots. A later value replaces the
        // earlier value rather than being added to it.
        acc.push(&GaiseInstructStreamResponse {
            chunk: GaiseStreamChunk::Usage(GaiseUsage {
                input: None,
                output: Some(HashMap::from([("completion".to_string(), 7)])),
                total: Some(HashMap::from([("total_tokens".to_string(), 17)])),
            }),
            external_id: None,
        });
        let usage = acc.usage.as_ref().unwrap();
        assert_eq!(usage.output.as_ref().unwrap().get("completion"), Some(&7));
        assert_eq!(usage.total.as_ref().unwrap().get("total_tokens"), Some(&17));
    }

    #[test]
    fn test_accumulation_preserves_reasoning_and_generated_media_order() {
        let mut acc = GaiseStreamAccumulator::new();
        for content in [
            GaiseContent::Reasoning {
                text: "first ".to_string(),
                signature: None,
            },
            GaiseContent::Reasoning {
                text: "second".to_string(),
                signature: Some("opaque-signature".to_string()),
            },
            GaiseContent::Image {
                data: vec![1, 2, 3],
                format: Some("image/png".to_string()),
            },
        ] {
            acc.push(&GaiseInstructStreamResponse {
                chunk: GaiseStreamChunk::Content(content),
                external_id: None,
            });
        }
        acc.push(&GaiseInstructStreamResponse {
            chunk: GaiseStreamChunk::Text("final answer".to_string()),
            external_id: None,
        });

        let message = acc.finish();
        let OneOrMany::Many(parts) = message.content.expect("Expected response content") else {
            panic!("Expected ordered multimodal content");
        };
        assert_eq!(parts.len(), 3);
        assert_eq!(
            parts[0],
            GaiseContent::Reasoning {
                text: "first second".to_string(),
                signature: Some("opaque-signature".to_string()),
            }
        );
        assert_eq!(
            parts[1],
            GaiseContent::Image {
                data: vec![1, 2, 3],
                format: Some("image/png".to_string()),
            }
        );
        assert_eq!(
            parts[2],
            GaiseContent::Text {
                text: "final answer".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn test_collect_stream() {
        let chunks: Vec<
            Result<GaiseInstructStreamResponse, Box<dyn std::error::Error + Send + Sync>>,
        > = vec![
            Ok(GaiseInstructStreamResponse {
                chunk: GaiseStreamChunk::Text("Hello ".to_string()),
                external_id: None,
            }),
            Ok(GaiseInstructStreamResponse {
                chunk: GaiseStreamChunk::Text("world!".to_string()),
                external_id: None,
            }),
        ];
        let stream = futures_util::stream::iter(chunks);

        let msg = GaiseStreamAccumulator::collect(stream).await.unwrap();

        if let Some(gaise_core::contracts::OneOrMany::One(
            gaise_core::contracts::GaiseContent::Text { text },
        )) = msg.content
        {
            assert_eq!(text, "Hello world!");
        } else {
            panic!("Expected text content");
        }
    }
}
