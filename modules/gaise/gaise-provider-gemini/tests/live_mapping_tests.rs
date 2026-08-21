#![cfg(feature = "live")]

use gaise_core::contracts::*;

// These tests verify the Gemini Live wire protocol mapping
// by testing config building and message serialization.

mod tests {
    use super::*;
    use gaise_provider_gemini::contracts::live_models::*;

    #[test]
    fn test_setup_message_basic_audio() {
        let config = GaiseLiveConfig {
            model: "gemini-3.1-flash-live-preview".to_string(),
            system_instruction: Some("You are a helpful assistant.".to_string()),
            voice: Some("Puck".to_string()),
            modalities: vec![GaiseLiveModality::Audio],
            ..Default::default()
        };

        let setup = build_test_setup(&config);
        let json = serde_json::to_value(&setup).unwrap();

        let gen_config = &json["setup"]["generationConfig"];
        assert_eq!(gen_config["responseModalities"][0], "AUDIO");
        assert_eq!(
            gen_config["speechConfig"]["voiceConfig"]["prebuiltVoiceConfig"]["voiceName"],
            "Puck"
        );

        let sys = &json["setup"]["systemInstruction"];
        assert_eq!(sys["parts"][0]["text"], "You are a helpful assistant.");
    }

    #[test]
    fn test_setup_message_with_tools() {
        let config = GaiseLiveConfig {
            model: "gemini-3.1-flash-live-preview".to_string(),
            tools: Some(vec![GaiseTool {
                name: "get-weather".to_string(),
                description: Some("Get weather for a city".to_string()),
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

        let setup = build_test_setup(&config);
        let json = serde_json::to_value(&setup).unwrap();

        let tools = &json["setup"]["tools"][0]["functionDeclarations"];
        // Gemini permits dashes in function names, so preserve the caller's name.
        assert_eq!(tools[0]["name"], "get-weather");
        assert_eq!(tools[0]["description"], "Get weather for a city");
        assert_eq!(
            tools[0]["parameters"]["properties"]["city"]["type"],
            "string"
        );
        assert_eq!(tools[0]["parameters"]["required"][0], "city");
    }

    #[test]
    fn test_setup_message_with_vad() {
        let config = GaiseLiveConfig {
            model: "gemini-3.1-flash-live-preview".to_string(),
            vad_config: Some(GaiseVadConfig {
                enabled: true,
                start_sensitivity: Some("high".to_string()),
                end_sensitivity: Some("low".to_string()),
                silence_duration_ms: Some(500),
                prefix_padding_ms: Some(40),
            }),
            ..Default::default()
        };

        let setup = build_test_setup(&config);
        let json = serde_json::to_value(&setup).unwrap();

        let vad = &json["setup"]["realtimeInputConfig"]["automaticActivityDetection"];
        assert_eq!(vad["disabled"], false);
        assert_eq!(vad["startOfSpeechSensitivity"], "START_SENSITIVITY_HIGH");
        assert_eq!(vad["endOfSpeechSensitivity"], "END_SENSITIVITY_LOW");
        assert_eq!(vad["silenceDurationMs"], 500);
        assert_eq!(vad["prefixPaddingMs"], 40);
    }

    #[test]
    fn test_setup_message_with_transcription() {
        let config = GaiseLiveConfig {
            model: "gemini-3.1-flash-live-preview".to_string(),
            transcription: Some(GaiseTranscriptionConfig {
                input: true,
                output: true,
            }),
            ..Default::default()
        };

        let setup = build_test_setup(&config);
        let json = serde_json::to_value(&setup).unwrap();

        assert!(json["setup"]["inputAudioTranscription"].is_object());
        assert!(json["setup"]["outputAudioTranscription"].is_object());
    }

    #[test]
    fn test_setup_message_text_modality() {
        let config = GaiseLiveConfig {
            model: "gemini-3.1-flash-live-preview".to_string(),
            modalities: vec![GaiseLiveModality::Text],
            ..Default::default()
        };

        let setup = build_test_setup(&config);
        let json = serde_json::to_value(&setup).unwrap();

        assert_eq!(
            json["setup"]["generationConfig"]["responseModalities"][0],
            "TEXT"
        );
    }

    #[test]
    fn test_audio_input_serialization() {
        let msg = GeminiLiveRealtimeInput {
            realtime_input: GeminiLiveRealtimeInputData {
                media_chunks: None,
                audio: Some(GeminiLiveMediaChunk {
                    mime_type: "audio/pcm;rate=16000".to_string(),
                    data: "AQID".to_string(), // base64 of [1,2,3]
                }),
                video: None,
                text: None,
                activity_start: None,
                activity_end: None,
                audio_stream_end: None,
            },
        };

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(
            json["realtimeInput"]["audio"]["mimeType"],
            "audio/pcm;rate=16000"
        );
        assert_eq!(json["realtimeInput"]["audio"]["data"], "AQID");
        assert!(json["realtimeInput"].get("mediaChunks").is_none());
    }

    #[test]
    fn test_video_frame_and_activity_serialization() {
        let frame = GeminiLiveRealtimeInput {
            realtime_input: GeminiLiveRealtimeInputData {
                media_chunks: None,
                audio: None,
                video: Some(GeminiLiveMediaChunk {
                    mime_type: "image/jpeg".to_string(),
                    data: "AQID".to_string(),
                }),
                text: None,
                activity_start: None,
                activity_end: None,
                audio_stream_end: None,
            },
        };
        let activity_end = GeminiLiveRealtimeInput {
            realtime_input: GeminiLiveRealtimeInputData {
                media_chunks: None,
                audio: None,
                video: None,
                text: None,
                activity_start: None,
                activity_end: Some(serde_json::json!({})),
                audio_stream_end: None,
            },
        };

        let frame_json = serde_json::to_value(frame).unwrap();
        assert_eq!(
            frame_json["realtimeInput"]["video"]["mimeType"],
            "image/jpeg"
        );
        let activity_json = serde_json::to_value(activity_end).unwrap();
        assert!(activity_json["realtimeInput"]["activityEnd"].is_object());
    }

    #[test]
    fn test_tool_response_serialization() {
        let msg = GeminiLiveToolResponse {
            tool_response: GeminiLiveToolResponseData {
                function_responses: vec![GeminiLiveFunctionResponse {
                    id: "call_123".to_string(),
                    name: "get_weather".to_string(),
                    response: serde_json::json!({"temperature": 22, "condition": "sunny"}),
                }],
            },
        };

        let json = serde_json::to_value(&msg).unwrap();
        let resp = &json["toolResponse"]["functionResponses"][0];
        assert_eq!(resp["id"], "call_123");
        assert_eq!(resp["name"], "get_weather");
        assert_eq!(resp["response"]["temperature"], 22);
    }

    #[test]
    fn test_server_message_deserialization_tool_call() {
        let json = r#"{
            "toolCall": {
                "functionCalls": [
                    {
                        "id": "call_abc",
                        "name": "get_weather",
                        "args": {"city": "London"}
                    }
                ]
            }
        }"#;

        let msg: GeminiLiveServerMessage = serde_json::from_str(json).unwrap();
        let tool_call = msg.tool_call.unwrap();
        assert_eq!(tool_call.function_calls.len(), 1);
        assert_eq!(tool_call.function_calls[0].id, "call_abc");
        assert_eq!(tool_call.function_calls[0].name, "get_weather");
        assert_eq!(
            tool_call.function_calls[0].args.as_ref().unwrap()["city"],
            "London"
        );
    }

    #[test]
    fn test_server_message_deserialization_turn_complete() {
        let json = r#"{
            "serverContent": {
                "turnComplete": true
            }
        }"#;

        let msg: GeminiLiveServerMessage = serde_json::from_str(json).unwrap();
        let content = msg.server_content.unwrap();
        assert_eq!(content.turn_complete, Some(true));
    }

    #[test]
    fn test_server_message_deserialization_interrupted() {
        let json = r#"{
            "serverContent": {
                "interrupted": true
            }
        }"#;

        let msg: GeminiLiveServerMessage = serde_json::from_str(json).unwrap();
        let content = msg.server_content.unwrap();
        assert_eq!(content.interrupted, Some(true));
    }

    #[test]
    fn test_server_message_deserialization_transcript() {
        let json = r#"{
            "serverContent": {
                "inputTranscription": { "text": "hello world" },
                "outputTranscription": { "text": "hi there" }
            }
        }"#;

        let msg: GeminiLiveServerMessage = serde_json::from_str(json).unwrap();
        let content = msg.server_content.unwrap();
        assert_eq!(
            content.input_transcription.unwrap().text.unwrap(),
            "hello world"
        );
        assert_eq!(
            content.output_transcription.unwrap().text.unwrap(),
            "hi there"
        );
    }

    #[test]
    fn test_server_message_deserialization_tool_call_cancellation() {
        let json = r#"{
            "toolCallCancellation": {
                "ids": ["call_1", "call_2"]
            }
        }"#;

        let msg: GeminiLiveServerMessage = serde_json::from_str(json).unwrap();
        let cancel = msg.tool_call_cancellation.unwrap();
        assert_eq!(cancel.ids, vec!["call_1", "call_2"]);
    }

    // Helper to build setup message (mirrors the private function in gemini_live_client)
    fn build_test_setup(config: &GaiseLiveConfig) -> GeminiLiveSetup {
        let modalities: Vec<String> = if config.modalities.is_empty() {
            vec!["AUDIO".to_string()]
        } else {
            config
                .modalities
                .iter()
                .map(|m| match m {
                    GaiseLiveModality::Text => "TEXT".to_string(),
                    GaiseLiveModality::Audio => "AUDIO".to_string(),
                })
                .collect()
        };

        let speech_config = config.voice.as_ref().map(|voice| GeminiLiveSpeechConfig {
            voice_config: GeminiLiveVoiceConfig {
                prebuilt_voice_config: GeminiLivePrebuiltVoice {
                    voice_name: voice.clone(),
                },
            },
        });

        let transcription_config = config.transcription.as_ref();
        let input_audio_transcription = transcription_config
            .filter(|t| t.input)
            .map(|_| serde_json::json!({}));
        let output_audio_transcription = transcription_config
            .filter(|t| t.output)
            .map(|_| serde_json::json!({}));

        let realtime_input_config =
            config
                .vad_config
                .as_ref()
                .map(|vad| GeminiLiveRealtimeInputConfig {
                    automatic_activity_detection: Some(GeminiLiveVadConfig {
                        disabled: Some(!vad.enabled),
                        start_of_speech_sensitivity: vad.start_sensitivity.as_deref().map(|s| {
                            match s {
                                "high" => "START_SENSITIVITY_HIGH",
                                "low" => "START_SENSITIVITY_LOW",
                                _ => "START_SENSITIVITY_MEDIUM",
                            }
                            .to_string()
                        }),
                        end_of_speech_sensitivity: vad.end_sensitivity.as_deref().map(|s| {
                            match s {
                                "high" => "END_SENSITIVITY_HIGH",
                                "low" => "END_SENSITIVITY_LOW",
                                _ => "END_SENSITIVITY_MEDIUM",
                            }
                            .to_string()
                        }),
                        prefix_padding_ms: vad.prefix_padding_ms,
                        silence_duration_ms: vad.silence_duration_ms,
                    }),
                });

        let tools = config.tools.as_ref().map(|ts| {
            vec![GeminiLiveToolSet {
                function_declarations: ts
                    .iter()
                    .map(|t| GeminiLiveFunctionDeclaration {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        parameters: t.parameters.as_ref().map(map_test_param),
                    })
                    .collect(),
            }]
        });

        let temperature = config
            .generation_config
            .as_ref()
            .and_then(|gc| gc.temperature);
        let top_p = config.generation_config.as_ref().and_then(|gc| gc.top_p);
        let top_k = config.generation_config.as_ref().and_then(|gc| gc.top_k);
        let max_output_tokens = config
            .generation_config
            .as_ref()
            .and_then(|gc| gc.max_tokens);

        GeminiLiveSetup {
            setup: GeminiLiveSetupConfig {
                model: format!("models/{}", config.model),
                generation_config: Some(GeminiLiveGenerationConfig {
                    response_modalities: Some(modalities),
                    speech_config,
                    temperature,
                    top_p,
                    top_k,
                    max_output_tokens,
                    thinking_config: None,
                    media_resolution: None,
                }),
                system_instruction: config.system_instruction.as_ref().map(|text| {
                    GeminiLiveSystemInstruction {
                        parts: vec![GeminiLiveTextPart { text: text.clone() }],
                    }
                }),
                tools,
                realtime_input_config,
                input_audio_transcription,
                output_audio_transcription,
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
}
