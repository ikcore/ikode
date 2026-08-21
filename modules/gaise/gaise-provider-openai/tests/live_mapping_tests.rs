#![cfg(feature = "live")]

use gaise_core::contracts::*;

mod tests {
    use super::*;
    use gaise_provider_openai::contracts::realtime_models::*;

    #[test]
    fn test_session_update_basic() {
        let config = GaiseLiveConfig {
            model: "gpt-realtime-2.1".to_string(),
            system_instruction: Some("You are a helpful assistant.".to_string()),
            voice: Some("alloy".to_string()),
            modalities: vec![GaiseLiveModality::Audio, GaiseLiveModality::Text],
            ..Default::default()
        };

        let update = build_test_session_update(&config);
        let json = serde_json::to_value(&update).unwrap();

        assert_eq!(json["type"], "session.update");
        assert_eq!(json["session"]["type"], "realtime");
        assert_eq!(json["session"]["output_modalities"][0], "audio");
        assert_eq!(
            json["session"]["output_modalities"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            json["session"]["instructions"],
            "You are a helpful assistant."
        );
        assert_eq!(json["session"]["audio"]["output"]["voice"], "alloy");
        assert_eq!(json["session"]["audio"]["input"]["format"]["rate"], 24_000);
    }

    #[test]
    fn test_session_update_with_tools() {
        let config = GaiseLiveConfig {
            model: "gpt-realtime-2.1".to_string(),
            tools: Some(vec![GaiseTool {
                name: "get-weather".to_string(),
                description: Some("Get weather".to_string()),
                parameters: Some(GaiseToolParameter {
                    r#type: Some("object".to_string()),
                    properties: Some(
                        [(
                            "city".to_string(),
                            GaiseToolParameter {
                                r#type: Some("string".to_string()),
                                description: Some("City name".to_string()),
                                ..Default::default()
                            },
                        )]
                        .into_iter()
                        .collect(),
                    ),
                    required: Some(vec!["city".to_string()]),
                    ..Default::default()
                }),
            }]),
            ..Default::default()
        };

        let update = build_test_session_update(&config);
        let json = serde_json::to_value(&update).unwrap();

        let tools = &json["session"]["tools"];
        assert_eq!(tools[0]["type"], "function");
        // OpenAI preserves hyphens in tool names (unlike Gemini)
        assert_eq!(tools[0]["name"], "get-weather");
        assert_eq!(tools[0]["description"], "Get weather");
        assert_eq!(
            tools[0]["parameters"]["properties"]["city"]["type"],
            "string"
        );
    }

    #[test]
    fn test_session_update_with_vad() {
        let config = GaiseLiveConfig {
            model: "gpt-realtime-2.1".to_string(),
            vad_config: Some(GaiseVadConfig {
                enabled: true,
                silence_duration_ms: Some(500),
                prefix_padding_ms: Some(300),
                ..Default::default()
            }),
            ..Default::default()
        };

        let update = build_test_session_update(&config);
        let json = serde_json::to_value(&update).unwrap();

        let td = &json["session"]["audio"]["input"]["turn_detection"];
        assert_eq!(td["type"], "server_vad");
        assert_eq!(td["silence_duration_ms"], 500);
        assert_eq!(td["prefix_padding_ms"], 300);
    }

    #[test]
    fn test_session_update_with_transcription() {
        let config = GaiseLiveConfig {
            model: "gpt-realtime-2.1".to_string(),
            transcription: Some(GaiseTranscriptionConfig {
                input: true,
                output: false,
            }),
            ..Default::default()
        };

        let update = build_test_session_update(&config);
        let json = serde_json::to_value(&update).unwrap();

        assert_eq!(
            json["session"]["audio"]["input"]["transcription"]["model"],
            "gpt-4o-mini-transcribe"
        );
    }

    #[test]
    fn test_audio_append_serialization() {
        let msg = OpenAIRealtimeAudioAppend {
            r#type: "input_audio_buffer.append".to_string(),
            audio: "AQIDBA==".to_string(),
        };

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "input_audio_buffer.append");
        assert_eq!(json["audio"], "AQIDBA==");
    }

    #[test]
    fn test_text_item_create_serialization() {
        let msg = OpenAIRealtimeItemCreate {
            r#type: "conversation.item.create".to_string(),
            item: OpenAIRealtimeItem {
                r#type: "message".to_string(),
                role: Some("user".to_string()),
                content: Some(vec![OpenAIRealtimeItemContent {
                    r#type: "input_text".to_string(),
                    text: Some("Hello".to_string()),
                    image_url: None,
                    detail: None,
                }]),
                call_id: None,
                output: None,
            },
        };

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "conversation.item.create");
        assert_eq!(json["item"]["type"], "message");
        assert_eq!(json["item"]["role"], "user");
        assert_eq!(json["item"]["content"][0]["type"], "input_text");
        assert_eq!(json["item"]["content"][0]["text"], "Hello");
    }

    #[test]
    fn test_image_item_create_serialization() {
        let msg = OpenAIRealtimeItemCreate {
            r#type: "conversation.item.create".to_string(),
            item: OpenAIRealtimeItem {
                r#type: "message".to_string(),
                role: Some("user".to_string()),
                content: Some(vec![OpenAIRealtimeItemContent {
                    r#type: "input_image".to_string(),
                    text: None,
                    image_url: Some("data:image/png;base64,AQID".to_string()),
                    detail: Some("high".to_string()),
                }]),
                call_id: None,
                output: None,
            },
        };

        let json = serde_json::to_value(&msg).unwrap();
        let image = &json["item"]["content"][0];
        assert_eq!(image["type"], "input_image");
        assert_eq!(image["image_url"], "data:image/png;base64,AQID");
        assert_eq!(image["detail"], "high");
    }

    #[test]
    fn test_tool_response_item_create_serialization() {
        let msg = OpenAIRealtimeItemCreate {
            r#type: "conversation.item.create".to_string(),
            item: OpenAIRealtimeItem {
                r#type: "function_call_output".to_string(),
                role: None,
                content: None,
                call_id: Some("call_123".to_string()),
                output: Some(r#"{"temperature": 22}"#.to_string()),
            },
        };

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["item"]["type"], "function_call_output");
        assert_eq!(json["item"]["call_id"], "call_123");
        assert_eq!(json["item"]["output"], r#"{"temperature": 22}"#);
    }

    #[test]
    fn test_server_event_function_call_done() {
        let json = r#"{
            "type": "response.function_call_arguments.done",
            "call_id": "call_abc",
            "name": "get-weather",
            "arguments": "{\"city\": \"London\"}"
        }"#;

        let event: OpenAIRealtimeServerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.r#type, "response.function_call_arguments.done");
        assert_eq!(event.call_id.unwrap(), "call_abc");
        assert_eq!(event.name.unwrap(), "get-weather");
        assert_eq!(event.arguments.unwrap(), r#"{"city": "London"}"#);
    }

    #[test]
    fn test_server_event_response_done_with_usage() {
        let json = r#"{
            "type": "response.done",
            "response": {
                "usage": {
                    "total_tokens": 150,
                    "input_tokens": 50,
                    "output_tokens": 100
                }
            }
        }"#;

        let event: OpenAIRealtimeServerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.r#type, "response.done");
        let usage = event.response.unwrap().usage.unwrap();
        assert_eq!(usage.total_tokens, Some(150));
        assert_eq!(usage.input_tokens, Some(50));
        assert_eq!(usage.output_tokens, Some(100));
    }

    #[test]
    fn test_server_event_error() {
        let json = r#"{
            "type": "error",
            "error": {
                "message": "Rate limit exceeded",
                "code": "rate_limit"
            }
        }"#;

        let event: OpenAIRealtimeServerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.r#type, "error");
        let err = event.error.unwrap();
        assert_eq!(err.message.unwrap(), "Rate limit exceeded");
        assert_eq!(err.code.unwrap(), "rate_limit");
    }

    #[test]
    fn test_server_event_audio_delta() {
        let json = r#"{
            "type": "response.output_audio.delta",
            "delta": "AQIDBA=="
        }"#;

        let event: OpenAIRealtimeServerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.r#type, "response.output_audio.delta");
        assert_eq!(event.delta.unwrap(), "AQIDBA==");
    }

    // Helper to build session update (mirrors the private function in openai_live_client)
    fn build_test_session_update(config: &GaiseLiveConfig) -> OpenAIRealtimeSessionUpdate {
        let output_modalities: Vec<String> = if config.modalities.is_empty()
            || config.modalities.contains(&GaiseLiveModality::Audio)
        {
            vec!["audio".to_string()]
        } else {
            vec!["text".to_string()]
        };

        let turn_detection = match config.vad_config.as_ref() {
            Some(vad) if !vad.enabled => None,
            vad => Some(OpenAIRealtimeTurnDetection {
                r#type: "server_vad".to_string(),
                create_response: Some(true),
                interrupt_response: Some(true),
                threshold: None,
                prefix_padding_ms: vad.and_then(|v| v.prefix_padding_ms),
                silence_duration_ms: vad.and_then(|v| v.silence_duration_ms),
            }),
        };

        let tools = config.tools.as_ref().map(|ts| {
            ts.iter()
                .map(|t| OpenAIRealtimeTool {
                    r#type: "function".to_string(),
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t
                        .parameters
                        .as_ref()
                        .map(map_test_param)
                        .unwrap_or(serde_json::json!({"type": "object", "properties": {}})),
                })
                .collect()
        });

        let max_output_tokens = config
            .generation_config
            .as_ref()
            .and_then(|gc| gc.max_tokens)
            .map(serde_json::Value::from);

        let transcription = config.transcription.as_ref().filter(|t| t.input).map(|_| {
            OpenAIRealtimeTranscriptionConfig {
                model: "gpt-4o-mini-transcribe".to_string(),
            }
        });

        let pcm24 = || OpenAIRealtimeAudioFormat {
            r#type: "audio/pcm".to_string(),
            rate: 24_000,
        };
        let audio_output = output_modalities.iter().any(|m| m == "audio");

        OpenAIRealtimeSessionUpdate {
            r#type: "session.update".to_string(),
            session: OpenAIRealtimeSessionConfig {
                r#type: "realtime".to_string(),
                output_modalities: Some(output_modalities),
                instructions: config.system_instruction.clone(),
                max_output_tokens,
                audio: Some(OpenAIRealtimeAudioConfig {
                    input: Some(OpenAIRealtimeAudioInputConfig {
                        format: pcm24(),
                        turn_detection,
                        transcription,
                    }),
                    output: audio_output.then(|| OpenAIRealtimeAudioOutputConfig {
                        format: pcm24(),
                        voice: config.voice.clone(),
                    }),
                }),
                reasoning: None,
                tools,
                tool_choice: None,
            },
        }
    }

    fn map_test_param(param: &GaiseToolParameter) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        if let Some(t) = &param.r#type {
            obj.insert("type".into(), serde_json::Value::String(t.clone()));
        }
        if let Some(desc) = &param.description {
            obj.insert(
                "description".into(),
                serde_json::Value::String(desc.clone()),
            );
        }
        if let Some(props) = &param.properties {
            let mut properties = serde_json::Map::new();
            for (k, v) in props {
                properties.insert(k.clone(), map_test_param(v));
            }
            obj.insert("properties".into(), serde_json::Value::Object(properties));
        }
        if let Some(req) = &param.required {
            obj.insert(
                "required".into(),
                serde_json::Value::Array(
                    req.iter()
                        .map(|r| serde_json::Value::String(r.clone()))
                        .collect(),
                ),
            );
        }
        serde_json::Value::Object(obj)
    }

    #[test]
    fn reasoning_is_sent_only_to_reasoning_realtime_models_and_tokens_are_clamped() {
        use gaise_provider_openai::openai_live_client::{
            build_session_update, realtime_model_supports_reasoning,
        };

        assert!(realtime_model_supports_reasoning("gpt-realtime-2.1"));
        assert!(realtime_model_supports_reasoning("gpt-realtime-2.1-mini"));
        assert!(realtime_model_supports_reasoning("gpt-realtime-2"));
        assert!(!realtime_model_supports_reasoning("gpt-realtime-1.5"));
        assert!(!realtime_model_supports_reasoning("gpt-realtime"));
        assert!(!realtime_model_supports_reasoning("gpt-realtime-mini"));
        assert!(!realtime_model_supports_reasoning(
            "gpt-4o-realtime-preview"
        ));

        let config = |model: &str| GaiseLiveConfig {
            model: model.to_string(),
            generation_config: Some(GaiseGenerationConfig {
                thinking_effort: Some("max".into()),
                max_tokens: Some(100_000),
                ..Default::default()
            }),
            ..Default::default()
        };
        let json = serde_json::to_value(build_session_update(&config("gpt-realtime-2.1"))).unwrap();
        assert_eq!(
            json["session"]["reasoning"]["effort"], "xhigh",
            "max clamps to xhigh on Realtime"
        );
        assert_eq!(
            json["session"]["max_output_tokens"], 4096,
            "session schema caps at 4096"
        );

        let json = serde_json::to_value(build_session_update(&config("gpt-realtime-1.5"))).unwrap();
        assert!(
            json["session"].get("reasoning").is_none(),
            "non-reasoning Realtime models never get reasoning"
        );
    }
}
