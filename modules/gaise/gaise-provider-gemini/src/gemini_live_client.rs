use async_trait::async_trait;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use gaise_core::GaiseLiveClient;
use gaise_core::contracts::{
    GaiseLiveConfig, GaiseLiveEvent, GaiseLiveEventStream, GaiseLiveInput, GaiseLiveModality,
    GaiseLiveSession, GaiseReasoningEffort, GaiseTool, GaiseToolParameter, GaiseUsage,
};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::contracts::live_models::*;

fn live_thinking_level_from_tokens(tokens: usize) -> String {
    match tokens {
        0 => "MINIMAL",
        1..=2_000 => "LOW",
        2_001..=12_000 => "MEDIUM",
        _ => "HIGH",
    }
    .to_string()
}

/// Live models accept `MINIMAL`…`HIGH`; resolved through the canonical
/// vocabulary (`none` → MINIMAL, `xhigh`/`max`/`ultra` → HIGH, `auto` → omit).
fn normalize_live_thinking_level(effort: &str) -> Option<String> {
    const LIVE_LEVELS: &[&str] = &["minimal", "low", "medium", "high"];
    match GaiseReasoningEffort::parse(effort) {
        GaiseReasoningEffort::Auto => None,
        GaiseReasoningEffort::Custom(raw) => Some(raw.to_ascii_uppercase()),
        level => Some(
            level
                .clamp_to(&GaiseReasoningEffort::levels(LIVE_LEVELS))
                .as_str()
                .to_ascii_uppercase(),
        ),
    }
}

fn normalize_live_media_resolution(resolution: &str) -> String {
    let upper = resolution.to_ascii_uppercase();
    if upper.starts_with("MEDIA_RESOLUTION_") {
        upper
    } else {
        format!("MEDIA_RESOLUTION_{upper}")
    }
}

fn live_modality_key(modality: &str, prefix: &str) -> String {
    let name = modality
        .strip_prefix("MODALITY_")
        .unwrap_or(modality)
        .to_ascii_lowercase();
    format!("{prefix}{name}_tokens")
}

fn insert_live_modality_usage(
    target: &mut HashMap<String, usize>,
    details: Option<&Vec<GeminiLiveModalityTokenCount>>,
    prefix: &str,
) {
    if let Some(details) = details {
        for detail in details {
            *target
                .entry(live_modality_key(&detail.modality, prefix))
                .or_insert(0) += detail.token_count;
        }
    }
}

fn map_live_usage(usage: &GeminiLiveUsageMetadata) -> GaiseUsage {
    let mut input = HashMap::new();
    if let Some(prompt) = usage.prompt_token_count {
        input.insert("prompt_tokens".to_string(), prompt);
    }
    if let Some(cached) = usage.cached_content_token_count {
        input.insert("cached_tokens".to_string(), cached);
    }
    if let Some(tool_prompt) = usage.tool_use_prompt_token_count {
        input.insert("tool_prompt_tokens".to_string(), tool_prompt);
    }
    insert_live_modality_usage(&mut input, usage.prompt_tokens_details.as_ref(), "");
    insert_live_modality_usage(&mut input, usage.cache_tokens_details.as_ref(), "cached_");
    insert_live_modality_usage(
        &mut input,
        usage.tool_use_prompt_tokens_details.as_ref(),
        "tool_",
    );

    let mut output = HashMap::new();
    if let Some(response) = usage.response_token_count {
        output.insert("response_tokens".to_string(), response);
    }
    if let Some(thoughts) = usage.thoughts_token_count {
        output.insert("reasoning_tokens".to_string(), thoughts);
    }
    insert_live_modality_usage(&mut output, usage.response_tokens_details.as_ref(), "");

    GaiseUsage {
        input: (!input.is_empty()).then_some(input),
        output: (!output.is_empty()).then_some(output),
        total: usage
            .total_token_count
            .map(|total| HashMap::from([("total_tokens".to_string(), total)])),
    }
}

pub struct GaiseClientGeminiLive {
    api_url: String,
    api_key: String,
}

impl GaiseClientGeminiLive {
    pub fn new(api_url: String, api_key: String) -> Self {
        Self { api_url, api_key }
    }
}

fn map_tool_parameter(param: &GaiseToolParameter) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    if let Some(t) = &param.r#type {
        let mapped = if t == "text" { "string" } else { t.as_str() };
        obj.insert("type".into(), serde_json::Value::String(mapped.to_string()));
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
            properties.insert(k.clone(), map_tool_parameter(v));
        }
        obj.insert("properties".into(), serde_json::Value::Object(properties));
    }
    if let Some(items) = &param.items {
        obj.insert("items".into(), map_tool_parameter(items));
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

fn build_tool_declarations(tools: &[GaiseTool]) -> Vec<GeminiLiveFunctionDeclaration> {
    tools
        .iter()
        .map(|t| GeminiLiveFunctionDeclaration {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: t.parameters.as_ref().map(map_tool_parameter),
        })
        .collect()
}

fn build_setup_message(config: &GaiseLiveConfig, api_model_path: &str) -> GeminiLiveSetup {
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

    let realtime_input_config = config.vad_config.as_ref().map(|vad| {
        let sensitivity_map = |s: &str| match s {
            "high" => "START_SENSITIVITY_HIGH",
            "low" => "START_SENSITIVITY_LOW",
            _ => "START_SENSITIVITY_MEDIUM",
        };
        let end_sensitivity_map = |s: &str| match s {
            "high" => "END_SENSITIVITY_HIGH",
            "low" => "END_SENSITIVITY_LOW",
            _ => "END_SENSITIVITY_MEDIUM",
        };

        GeminiLiveRealtimeInputConfig {
            automatic_activity_detection: Some(GeminiLiveVadConfig {
                disabled: Some(!vad.enabled),
                start_of_speech_sensitivity: vad
                    .start_sensitivity
                    .as_deref()
                    .map(|s| sensitivity_map(s).to_string()),
                end_of_speech_sensitivity: vad
                    .end_sensitivity
                    .as_deref()
                    .map(|s| end_sensitivity_map(s).to_string()),
                prefix_padding_ms: vad.prefix_padding_ms,
                silence_duration_ms: vad.silence_duration_ms,
            }),
        }
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
    let thinking_config = config.generation_config.as_ref().and_then(|gc| {
        let uses_levels = config.model.to_ascii_lowercase().starts_with("gemini-3");
        let requested = gc.thinking_effort.is_some()
            || gc.thinking_tokens.is_some()
            || gc.include_thoughts.is_some();
        requested.then(|| {
            if uses_levels {
                crate::contracts::GeminiThinkingConfig {
                    thinking_budget: None,
                    thinking_level: gc
                        .thinking_effort
                        .as_deref()
                        .and_then(normalize_live_thinking_level)
                        .or_else(|| gc.thinking_tokens.map(live_thinking_level_from_tokens)),
                    include_thoughts: gc.include_thoughts,
                }
            } else {
                crate::contracts::GeminiThinkingConfig {
                    thinking_budget: gc.thinking_tokens.map(|tokens| tokens as i64),
                    thinking_level: None,
                    include_thoughts: gc.include_thoughts,
                }
            }
        })
    });
    let media_resolution = config
        .generation_config
        .as_ref()
        .and_then(|gc| gc.input_media_resolution.as_deref())
        .map(normalize_live_media_resolution);

    let generation_config = GeminiLiveGenerationConfig {
        response_modalities: Some(modalities),
        speech_config,
        temperature,
        top_p,
        top_k,
        max_output_tokens,
        thinking_config,
        media_resolution,
    };

    let system_instruction =
        config
            .system_instruction
            .as_ref()
            .map(|text| GeminiLiveSystemInstruction {
                parts: vec![GeminiLiveTextPart { text: text.clone() }],
            });

    let tools = config.tools.as_ref().map(|ts| {
        vec![GeminiLiveToolSet {
            function_declarations: build_tool_declarations(ts),
        }]
    });

    GeminiLiveSetup {
        setup: GeminiLiveSetupConfig {
            model: api_model_path.to_string(),
            generation_config: Some(generation_config),
            system_instruction,
            tools,
            realtime_input_config,
            input_audio_transcription,
            output_audio_transcription,
        },
    }
}

#[async_trait]
impl GaiseLiveClient for GaiseClientGeminiLive {
    async fn live_connect(
        &self,
        config: &GaiseLiveConfig,
    ) -> Result<GaiseLiveSession, Box<dyn std::error::Error + Send + Sync>> {
        // Build WebSocket URL
        // api_url is like "https://generativelanguage.googleapis.com/v1beta"
        // We need: wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent
        let url_parsed: url::Url = self.api_url.parse()?;
        let host = url_parsed
            .host_str()
            .ok_or("Invalid API URL: no host")?
            .to_string();
        let scheme = if self.api_url.starts_with("https") {
            "wss"
        } else {
            "ws"
        };

        let ws_url = format!(
            "{}://{}/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key={}",
            scheme, host, self.api_key
        );

        let api_model_path = format!("models/{}", config.model);

        // Connect WebSocket
        let (ws_stream, _response) = tokio_tungstenite::connect_async(&ws_url).await?;
        let (mut ws_sink, mut ws_source) = ws_stream.split();

        // Send setup message
        let setup_msg = build_setup_message(config, &api_model_path);
        let setup_json = serde_json::to_string(&setup_msg)?;
        ws_sink.send(Message::Text(setup_json.into())).await?;

        // Wait for setupComplete
        let mut setup_complete = false;
        while let Some(msg) = ws_source.next().await {
            let msg = msg?;
            if let Message::Text(text) = msg {
                let server_msg: GeminiLiveServerMessage = serde_json::from_str(&text)?;
                if server_msg.setup_complete.is_some() {
                    setup_complete = true;
                    break;
                }
                if let Some(error) = server_msg.error {
                    return Err(std::io::Error::other(format!(
                        "Gemini Live setup failed: {error}"
                    ))
                    .into());
                }
            }
        }
        if !setup_complete {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Gemini Live closed before setupComplete",
            )
            .into());
        }

        // Create channels
        let (input_tx, mut input_rx) = mpsc::channel::<GaiseLiveInput>(256);
        let (event_tx, event_rx) =
            mpsc::channel::<Result<GaiseLiveEvent, Box<dyn std::error::Error + Send + Sync>>>(256);

        let session_id = uuid_simple();

        // Send session started event
        let _ = event_tx
            .send(Ok(GaiseLiveEvent::SessionStarted {
                session_id: session_id.clone(),
                model: config.model.clone(),
            }))
            .await;

        // Spawn send loop: reads from input_rx, writes to ws_sink
        let event_tx_send = event_tx.clone();
        tokio::spawn(async move {
            while let Some(input) = input_rx.recv().await {
                let msg_result = match input {
                    GaiseLiveInput::Audio { data, sample_rate } => {
                        let b64 = base64::prelude::BASE64_STANDARD.encode(&data);
                        let mime = format!("audio/pcm;rate={}", sample_rate);
                        let msg = GeminiLiveRealtimeInput {
                            realtime_input: GeminiLiveRealtimeInputData {
                                media_chunks: None,
                                audio: Some(GeminiLiveMediaChunk {
                                    mime_type: mime,
                                    data: b64,
                                }),
                                video: None,
                                text: None,
                                activity_start: None,
                                activity_end: None,
                                audio_stream_end: None,
                            },
                        };
                        serde_json::to_string(&msg)
                    }
                    GaiseLiveInput::Image {
                        data,
                        mime_type,
                        detail: _,
                    } => {
                        let msg = GeminiLiveRealtimeInput {
                            realtime_input: GeminiLiveRealtimeInputData {
                                media_chunks: None,
                                audio: None,
                                video: Some(GeminiLiveMediaChunk {
                                    mime_type,
                                    data: base64::prelude::BASE64_STANDARD.encode(data),
                                }),
                                text: None,
                                activity_start: None,
                                activity_end: None,
                                audio_stream_end: None,
                            },
                        };
                        serde_json::to_string(&msg)
                    }
                    GaiseLiveInput::Text { text } => {
                        // Gemini 3.1 Live only permits clientContent for initial
                        // history; realtimeInput.text is valid throughout a session.
                        let msg = GeminiLiveRealtimeInput {
                            realtime_input: GeminiLiveRealtimeInputData {
                                media_chunks: None,
                                audio: None,
                                video: None,
                                text: Some(text),
                                activity_start: None,
                                activity_end: None,
                                audio_stream_end: None,
                            },
                        };
                        serde_json::to_string(&msg)
                    }
                    GaiseLiveInput::ToolResponse {
                        call_id,
                        name,
                        result,
                    } => {
                        let response = if result.is_object() {
                            result
                        } else {
                            serde_json::json!({ "result": result })
                        };
                        let msg = GeminiLiveToolResponse {
                            tool_response: GeminiLiveToolResponseData {
                                function_responses: vec![GeminiLiveFunctionResponse {
                                    id: call_id,
                                    name,
                                    response,
                                }],
                            },
                        };
                        serde_json::to_string(&msg)
                    }
                    GaiseLiveInput::ActivityStart => {
                        let msg = GeminiLiveRealtimeInput {
                            realtime_input: GeminiLiveRealtimeInputData {
                                media_chunks: None,
                                audio: None,
                                video: None,
                                text: None,
                                activity_start: Some(serde_json::json!({})),
                                activity_end: None,
                                audio_stream_end: None,
                            },
                        };
                        serde_json::to_string(&msg)
                    }
                    GaiseLiveInput::ActivityEnd => {
                        let msg = GeminiLiveRealtimeInput {
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
                        serde_json::to_string(&msg)
                    }
                    GaiseLiveInput::AudioStreamEnd => {
                        let msg = GeminiLiveRealtimeInput {
                            realtime_input: GeminiLiveRealtimeInputData {
                                media_chunks: None,
                                audio: None,
                                video: None,
                                text: None,
                                activity_start: None,
                                activity_end: None,
                                audio_stream_end: Some(true),
                            },
                        };
                        serde_json::to_string(&msg)
                    }
                    GaiseLiveInput::ClearAudio | GaiseLiveInput::CancelResponse => {
                        let _ = event_tx_send
                            .send(Ok(GaiseLiveEvent::Error {
                                message: "Gemini Live does not expose this realtime control"
                                    .to_string(),
                            }))
                            .await;
                        continue;
                    }
                    GaiseLiveInput::Close => {
                        let _ = ws_sink.close().await;
                        break;
                    }
                };

                match msg_result {
                    Ok(json) => {
                        if let Err(e) = ws_sink.send(Message::Text(json.into())).await {
                            let _ = event_tx_send
                                .send(Ok(GaiseLiveEvent::Error {
                                    message: format!("WebSocket send error: {}", e),
                                }))
                                .await;
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = event_tx_send
                            .send(Ok(GaiseLiveEvent::Error {
                                message: format!("Serialization error: {}", e),
                            }))
                            .await;
                    }
                }
            }
        });

        // Spawn receive loop: reads from ws_source, writes to event_tx
        tokio::spawn(async move {
            while let Some(msg) = ws_source.next().await {
                let msg = match msg {
                    Ok(m) => m,
                    Err(e) => {
                        let _ = event_tx
                            .send(Ok(GaiseLiveEvent::Error {
                                message: format!("WebSocket receive error: {}", e),
                            }))
                            .await;
                        break;
                    }
                };

                match msg {
                    Message::Text(text) => {
                        let server_msg: GeminiLiveServerMessage = match serde_json::from_str(&text)
                        {
                            Ok(m) => m,
                            Err(e) => {
                                let _ = event_tx
                                    .send(Ok(GaiseLiveEvent::Error {
                                        message: format!("Parse error: {}", e),
                                    }))
                                    .await;
                                continue;
                            }
                        };

                        if let Some(error) = &server_msg.error {
                            let _ = event_tx
                                .send(Ok(GaiseLiveEvent::Error {
                                    message: format!("Gemini Live error: {error}"),
                                }))
                                .await;
                            continue;
                        }

                        // Process server content
                        if let Some(content) = &server_msg.server_content {
                            // Audio output from model
                            if let Some(model_turn) = &content.model_turn {
                                for part in &model_turn.parts {
                                    if let Some(text) = &part.text {
                                        let event = if part.thought == Some(true) {
                                            GaiseLiveEvent::Reasoning {
                                                text: text.clone(),
                                                signature: part.thought_signature.clone(),
                                            }
                                        } else {
                                            GaiseLiveEvent::Text { text: text.clone() }
                                        };
                                        let _ = event_tx.send(Ok(event)).await;
                                    }
                                    if let Some(inline_data) = &part.inline_data
                                        && let Ok(audio_bytes) = base64::prelude::BASE64_STANDARD
                                            .decode(&inline_data.data)
                                    {
                                        // Parse sample rate from mime_type (e.g. "audio/pcm;rate=24000")
                                        let sample_rate = inline_data
                                            .mime_type
                                            .split("rate=")
                                            .nth(1)
                                            .and_then(|s| s.parse::<u32>().ok())
                                            .unwrap_or(24000);

                                        let _ = event_tx
                                            .send(Ok(GaiseLiveEvent::Audio {
                                                data: audio_bytes,
                                                sample_rate,
                                            }))
                                            .await;
                                    }
                                }
                            }

                            // Transcriptions
                            if let Some(tx_data) = &content.input_transcription
                                && let Some(text) = &tx_data.text
                            {
                                let _ = event_tx
                                    .send(Ok(GaiseLiveEvent::Transcript {
                                        role: "user".to_string(),
                                        text: text.clone(),
                                    }))
                                    .await;
                            }
                            if let Some(tx_data) = &content.output_transcription
                                && let Some(text) = &tx_data.text
                            {
                                let _ = event_tx
                                    .send(Ok(GaiseLiveEvent::Transcript {
                                        role: "assistant".to_string(),
                                        text: text.clone(),
                                    }))
                                    .await;
                            }

                            // Turn complete
                            if content.turn_complete == Some(true) {
                                let _ = event_tx.send(Ok(GaiseLiveEvent::TurnComplete)).await;
                            }

                            // Interrupted (barge-in)
                            if content.interrupted == Some(true) {
                                let _ = event_tx.send(Ok(GaiseLiveEvent::Interrupted)).await;
                            }
                        }

                        // Tool calls
                        if let Some(tool_call) = &server_msg.tool_call {
                            for fc in &tool_call.function_calls {
                                let _ = event_tx
                                    .send(Ok(GaiseLiveEvent::ToolCall {
                                        id: fc.id.clone(),
                                        function: gaise_core::contracts::GaiseFunctionCall {
                                            name: fc.name.clone(),
                                            arguments: fc.args.as_ref().map(|a| a.to_string()),
                                        },
                                    }))
                                    .await;
                            }
                        }

                        // Tool call cancellation
                        if let Some(cancel) = &server_msg.tool_call_cancellation {
                            let _ = event_tx
                                .send(Ok(GaiseLiveEvent::ToolCallCancelled {
                                    ids: cancel.ids.clone(),
                                }))
                                .await;
                        }

                        // Usage metadata
                        if let Some(usage) = &server_msg.usage_metadata {
                            let mapped = map_live_usage(usage);
                            if mapped.input.is_some()
                                || mapped.output.is_some()
                                || mapped.total.is_some()
                            {
                                let _ = event_tx.send(Ok(GaiseLiveEvent::Usage(mapped))).await;
                            }
                        }

                        // GoAway
                        if server_msg.go_away.is_some() {
                            let _ = event_tx
                                .send(Ok(GaiseLiveEvent::Error {
                                    message: "Server requested disconnect (goAway)".to_string(),
                                }))
                                .await;
                            let _ = event_tx.send(Ok(GaiseLiveEvent::SessionEnded)).await;
                            break;
                        }
                    }
                    Message::Binary(_) => {
                        // Gemini Live uses JSON text frames, not binary
                    }
                    Message::Close(_) => {
                        let _ = event_tx.send(Ok(GaiseLiveEvent::SessionEnded)).await;
                        break;
                    }
                    _ => {}
                }
            }
        });

        // Convert event_rx into a Stream
        let event_stream = tokio_stream::wrappers::ReceiverStream::new(event_rx);
        let pinned: GaiseLiveEventStream = Box::pin(event_stream);

        Ok(GaiseLiveSession {
            tx: input_tx,
            rx: pinned,
        })
    }
}

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{:x}{:x}", d.as_secs(), d.subsec_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_live_usage_modalities_and_keeps_total_neutral() {
        let usage: GeminiLiveUsageMetadata = serde_json::from_value(serde_json::json!({
            "promptTokenCount": 90,
            "cachedContentTokenCount": 10,
            "responseTokenCount": 70,
            "toolUsePromptTokenCount": 4,
            "thoughtsTokenCount": 8,
            "totalTokenCount": 172,
            "promptTokensDetails": [
                {"modality": "TEXT", "tokenCount": 30},
                {"modality": "VIDEO", "tokenCount": 20},
                {"modality": "AUDIO", "tokenCount": 40}
            ],
            "responseTokensDetails": [
                {"modality": "TEXT", "tokenCount": 25},
                {"modality": "AUDIO", "tokenCount": 45}
            ],
            "cacheTokensDetails": [{"modality": "AUDIO", "tokenCount": 10}],
            "toolUsePromptTokensDetails": [{"modality": "TEXT", "tokenCount": 4}]
        }))
        .unwrap();

        let mapped = map_live_usage(&usage);
        let input = mapped.input.unwrap();
        let output = mapped.output.unwrap();
        assert_eq!(input.get("text_tokens"), Some(&30));
        assert_eq!(input.get("video_tokens"), Some(&20));
        assert_eq!(input.get("audio_tokens"), Some(&40));
        assert_eq!(input.get("cached_audio_tokens"), Some(&10));
        assert_eq!(output.get("text_tokens"), Some(&25));
        assert_eq!(output.get("audio_tokens"), Some(&45));
        assert_eq!(output.get("reasoning_tokens"), Some(&8));
        assert!(!output.contains_key("total_tokens"));
        assert_eq!(mapped.total.unwrap().get("total_tokens"), Some(&172));
    }
}
