use async_trait::async_trait;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use gaise_core::GaiseLiveClient;
use gaise_core::contracts::{
    GaiseFunctionCall, GaiseLiveConfig, GaiseLiveEvent, GaiseLiveEventStream, GaiseLiveInput,
    GaiseLiveModality, GaiseLiveSession, GaiseReasoningEffort, GaiseTool, GaiseToolParameter,
    GaiseUsage,
};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::contracts::realtime_models::*;

fn map_realtime_usage(usage: &OpenAIRealtimeResponseUsage) -> GaiseUsage {
    let mut input = HashMap::new();
    let mut output = HashMap::new();
    if let Some(tokens) = usage.input_tokens {
        input.insert("input_tokens".to_string(), tokens);
    }
    if let Some(tokens) = usage.output_tokens {
        output.insert("output_tokens".to_string(), tokens);
    }
    if let Some(details) = &usage.input_token_details {
        if let Some(tokens) = details.cached_tokens {
            input.insert("cached_tokens".to_string(), tokens);
        }
        if let Some(tokens) = details.text_tokens {
            input.insert("text_tokens".to_string(), tokens);
        }
        if let Some(tokens) = details.audio_tokens {
            input.insert("audio_tokens".to_string(), tokens);
        }
        if let Some(tokens) = details.image_tokens {
            input.insert("image_tokens".to_string(), tokens);
        }
        if let Some(cached) = &details.cached_tokens_details {
            if let Some(tokens) = cached.text_tokens {
                input.insert("cached_text_tokens".to_string(), tokens);
            }
            if let Some(tokens) = cached.audio_tokens {
                input.insert("cached_audio_tokens".to_string(), tokens);
            }
            if let Some(tokens) = cached.image_tokens {
                input.insert("cached_image_tokens".to_string(), tokens);
            }
        }
    }
    if let Some(details) = &usage.output_token_details {
        if let Some(tokens) = details.text_tokens {
            output.insert("text_tokens".to_string(), tokens);
        }
        if let Some(tokens) = details.audio_tokens {
            output.insert("audio_tokens".to_string(), tokens);
        }
    }
    GaiseUsage {
        input: (!input.is_empty()).then_some(input),
        output: (!output.is_empty()).then_some(output),
        total: usage
            .total_tokens
            .map(|tokens| HashMap::from([("total_tokens".to_string(), tokens)])),
    }
}

fn map_transcription_usage(usage: &OpenAIRealtimeTranscriptionUsage) -> GaiseUsage {
    let mut input = HashMap::new();
    let mut output = HashMap::new();
    if let Some(tokens) = usage.input_tokens {
        input.insert("transcription_input_tokens".to_string(), tokens);
    }
    if let Some(tokens) = usage.output_tokens {
        output.insert("transcription_output_tokens".to_string(), tokens);
    }
    if let Some(milliseconds) = usage.seconds.and_then(|seconds| {
        let milliseconds = seconds * 1_000.0;
        (milliseconds.is_finite() && milliseconds >= 0.0 && milliseconds <= usize::MAX as f64)
            .then(|| milliseconds.round() as usize)
    }) {
        input.insert("transcription_audio_milliseconds".to_string(), milliseconds);
    }
    if let Some(details) = &usage.input_token_details {
        if let Some(tokens) = details.text_tokens {
            input.insert("transcription_text_tokens".to_string(), tokens);
        }
        if let Some(tokens) = details.audio_tokens {
            input.insert("transcription_audio_tokens".to_string(), tokens);
        }
    }
    GaiseUsage {
        input: (!input.is_empty()).then_some(input),
        output: (!output.is_empty()).then_some(output),
        total: usage
            .total_tokens
            .map(|tokens| HashMap::from([("transcription_total_tokens".to_string(), tokens)])),
    }
}

fn realtime_reasoning_effort_from_tokens(tokens: usize) -> String {
    match tokens {
        0..=1_000 => "minimal",
        1_001..=4_000 => "low",
        4_001..=12_000 => "medium",
        12_001..=24_000 => "high",
        _ => "xhigh",
    }
    .to_string()
}

/// `gpt-realtime-2` and later are reasoning models; `gpt-realtime`,
/// `gpt-realtime-1.5`, `gpt-realtime-mini`, and the `gpt-4o-*-realtime`
/// family are not (model pages, audited 2026-08-20).
pub fn realtime_model_supports_reasoning(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    let Some(rest) = m.strip_prefix("gpt-realtime-") else {
        return false;
    };
    let major: u32 = rest
        .split(['-', '.'])
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    major >= 2
}

/// Realtime accepts `minimal`…`xhigh`; `none` becomes `minimal`, `max`/`ultra`
/// become `xhigh`, `auto` omits the field, custom strings pass through.
fn normalize_realtime_reasoning_effort(effort: &str) -> Option<String> {
    const REALTIME_LEVELS: &[&str] = &["minimal", "low", "medium", "high", "xhigh"];
    match GaiseReasoningEffort::parse(effort) {
        GaiseReasoningEffort::Auto => None,
        GaiseReasoningEffort::Custom(raw) => Some(raw),
        level => Some(
            level
                .clamp_to(&GaiseReasoningEffort::levels(REALTIME_LEVELS))
                .as_str()
                .to_string(),
        ),
    }
}

pub struct GaiseClientOpenAILive {
    api_url: String,
    api_key: String,
}

impl GaiseClientOpenAILive {
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

fn build_realtime_tools(tools: &[GaiseTool]) -> Vec<OpenAIRealtimeTool> {
    tools
        .iter()
        .map(|t| OpenAIRealtimeTool {
            r#type: "function".to_string(),
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: t
                .parameters
                .as_ref()
                .map(map_tool_parameter)
                .unwrap_or(serde_json::json!({"type": "object", "properties": {}})),
        })
        .collect()
}

/// Build the `session.update` frame for a live config (pure; used by tests).
pub fn build_session_update(config: &GaiseLiveConfig) -> OpenAIRealtimeSessionUpdate {
    // The GA Realtime API permits exactly one output modality. Audio responses
    // always include a transcript, so prefer audio when callers request both.
    let output_modalities: Vec<String> =
        if config.modalities.is_empty() || config.modalities.contains(&GaiseLiveModality::Audio) {
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

    let tools = config.tools.as_ref().map(|ts| build_realtime_tools(ts));

    // The GA session schema accepts 1..=4096 or "inf".
    let max_output_tokens = config
        .generation_config
        .as_ref()
        .and_then(|gc| gc.max_tokens)
        .map(|m| serde_json::Value::from(m.clamp(1, 4096)));
    // Only reasoning-capable Realtime models (gpt-realtime-2, 2.1, 2.1-mini)
    // accept `reasoning.effort`; legacy gpt-realtime / 1.5 / mini reject it.
    let reasoning_model = realtime_model_supports_reasoning(&config.model);
    let reasoning = config
        .generation_config
        .as_ref()
        .filter(|_| reasoning_model)
        .and_then(|gc| {
            gc.thinking_effort
                .as_deref()
                .and_then(normalize_realtime_reasoning_effort)
                .or_else(|| {
                    gc.thinking_tokens
                        .map(realtime_reasoning_effort_from_tokens)
                })
                .map(|effort| OpenAIRealtimeReasoning { effort })
        });

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
    let audio = Some(OpenAIRealtimeAudioConfig {
        input: Some(OpenAIRealtimeAudioInputConfig {
            format: pcm24(),
            turn_detection,
            transcription,
        }),
        output: audio_output.then(|| OpenAIRealtimeAudioOutputConfig {
            format: pcm24(),
            voice: config.voice.clone(),
        }),
    });

    let tool_choice = config.tool_config.as_ref().and_then(|tc| {
        tc.mode.as_deref().map(|mode| match mode {
            "any" => "required".to_string(),
            other => other.to_string(),
        })
    });

    OpenAIRealtimeSessionUpdate {
        r#type: "session.update".to_string(),
        session: OpenAIRealtimeSessionConfig {
            r#type: "realtime".to_string(),
            output_modalities: Some(output_modalities),
            instructions: config.system_instruction.clone(),
            max_output_tokens,
            audio,
            reasoning,
            tools,
            tool_choice,
        },
    }
}

async fn send_two_messages<S: serde::Serialize, T: serde::Serialize>(
    sink: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    msg1: &S,
    msg2: &T,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let json1 = serde_json::to_string(msg1)?;
    sink.send(Message::Text(json1.into())).await?;
    let json2 = serde_json::to_string(msg2)?;
    sink.send(Message::Text(json2.into())).await?;
    Ok(())
}

#[async_trait]
impl GaiseLiveClient for GaiseClientOpenAILive {
    async fn live_connect(
        &self,
        config: &GaiseLiveConfig,
    ) -> Result<GaiseLiveSession, Box<dyn std::error::Error + Send + Sync>> {
        // Build WebSocket URL: wss://api.openai.com/v1/realtime?model=MODEL
        let base = self
            .api_url
            .trim_end_matches('/')
            .replace("https://", "wss://")
            .replace("http://", "ws://");
        let api_root = if base.ends_with("/v1") {
            base
        } else {
            format!("{base}/v1")
        };
        let ws_url = format!("{api_root}/realtime?model={}", config.model);

        // Build request with auth header
        let request = http::Request::builder()
            .uri(&ws_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tungstenite::handshake::client::generate_key(),
            )
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header(
                "Host",
                http::Uri::try_from(&ws_url)?
                    .host()
                    .unwrap_or("api.openai.com"),
            )
            .body(())?;

        let (ws_stream, _response) = tokio_tungstenite::connect_async(request).await?;
        let (mut ws_sink, mut ws_source) = ws_stream.split();

        // Wait for session.created
        let mut session_created = false;
        while let Some(msg) = ws_source.next().await {
            let msg = msg?;
            if let Message::Text(text) = msg {
                let event: OpenAIRealtimeServerEvent = serde_json::from_str(&text)?;
                if event.r#type == "session.created" {
                    session_created = true;
                    break;
                }
                if event.r#type == "error" {
                    let message = event
                        .error
                        .and_then(|error| error.message)
                        .unwrap_or_else(|| "OpenAI Realtime session creation failed".to_string());
                    return Err(std::io::Error::other(message).into());
                }
            }
        }
        if !session_created {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "OpenAI Realtime closed before session.created",
            )
            .into());
        }

        // Send session.update with config
        let session_update = build_session_update(config);
        let update_json = serde_json::to_string(&session_update)?;
        ws_sink.send(Message::Text(update_json.into())).await?;

        // Wait for session.updated
        let mut session_updated = false;
        while let Some(msg) = ws_source.next().await {
            let msg = msg?;
            if let Message::Text(text) = msg {
                let event: OpenAIRealtimeServerEvent = serde_json::from_str(&text)?;
                if event.r#type == "session.updated" {
                    session_updated = true;
                    break;
                }
                if event.r#type == "error" {
                    let message = event
                        .error
                        .and_then(|error| error.message)
                        .unwrap_or_else(|| "OpenAI Realtime session update failed".to_string());
                    return Err(std::io::Error::other(message).into());
                }
            }
        }
        if !session_updated {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "OpenAI Realtime closed before session.updated",
            )
            .into());
        }

        // Create channels
        let (input_tx, mut input_rx) = mpsc::channel::<GaiseLiveInput>(256);
        let (event_tx, event_rx) =
            mpsc::channel::<Result<GaiseLiveEvent, Box<dyn std::error::Error + Send + Sync>>>(256);

        let session_id = uuid_simple();

        let _ = event_tx
            .send(Ok(GaiseLiveEvent::SessionStarted {
                session_id: session_id.clone(),
                model: config.model.clone(),
            }))
            .await;

        // Spawn send loop
        let event_tx_send = event_tx.clone();
        let manual_vad = config.vad_config.as_ref().is_some_and(|vad| !vad.enabled);
        let default_image_detail = config
            .generation_config
            .as_ref()
            .and_then(|gc| gc.input_image_detail.clone());
        tokio::spawn(async move {
            while let Some(input) = input_rx.recv().await {
                let result: Result<(), Box<dyn std::error::Error + Send + Sync>> = match input {
                    GaiseLiveInput::Audio { data, sample_rate } => {
                        if sample_rate != 24_000 {
                            let _ = event_tx_send
                                .send(Ok(GaiseLiveEvent::Error {
                                    message: format!(
                                        "OpenAI Realtime requires 24 kHz PCM16 input; received {sample_rate} Hz"
                                    ),
                                }))
                                .await;
                            continue;
                        }
                        let b64 = base64::prelude::BASE64_STANDARD.encode(&data);
                        let msg = OpenAIRealtimeAudioAppend {
                            r#type: "input_audio_buffer.append".to_string(),
                            audio: b64,
                        };
                        match serde_json::to_string(&msg) {
                            Ok(json) => ws_sink
                                .send(Message::Text(json.into()))
                                .await
                                .map_err(|e| e.into()),
                            Err(e) => Err(e.into()),
                        }
                    }
                    GaiseLiveInput::Image {
                        data,
                        mime_type,
                        detail,
                    } => {
                        let normalized_mime = match mime_type.to_ascii_lowercase().as_str() {
                            "image/png" => "image/png",
                            "image/jpeg" | "image/jpg" => "image/jpeg",
                            _ => {
                                let _ = event_tx_send
                                    .send(Ok(GaiseLiveEvent::Error {
                                        message: format!(
                                            "OpenAI Realtime image input supports PNG and JPEG, not {mime_type}"
                                        ),
                                    }))
                                    .await;
                                continue;
                            }
                        };
                        let image_url = format!(
                            "data:{normalized_mime};base64,{}",
                            base64::prelude::BASE64_STANDARD.encode(data)
                        );
                        let item_msg = OpenAIRealtimeItemCreate {
                            r#type: "conversation.item.create".to_string(),
                            item: OpenAIRealtimeItem {
                                r#type: "message".to_string(),
                                role: Some("user".to_string()),
                                content: Some(vec![OpenAIRealtimeItemContent {
                                    r#type: "input_image".to_string(),
                                    text: None,
                                    image_url: Some(image_url),
                                    detail: detail.or_else(|| default_image_detail.clone()),
                                }]),
                                call_id: None,
                                output: None,
                            },
                        };
                        let response_msg = OpenAIRealtimeResponseCreate {
                            r#type: "response.create".to_string(),
                        };
                        send_two_messages(&mut ws_sink, &item_msg, &response_msg).await
                    }
                    GaiseLiveInput::Text { text } => {
                        let item_msg = OpenAIRealtimeItemCreate {
                            r#type: "conversation.item.create".to_string(),
                            item: OpenAIRealtimeItem {
                                r#type: "message".to_string(),
                                role: Some("user".to_string()),
                                content: Some(vec![OpenAIRealtimeItemContent {
                                    r#type: "input_text".to_string(),
                                    text: Some(text),
                                    image_url: None,
                                    detail: None,
                                }]),
                                call_id: None,
                                output: None,
                            },
                        };
                        let response_msg = OpenAIRealtimeResponseCreate {
                            r#type: "response.create".to_string(),
                        };
                        send_two_messages(&mut ws_sink, &item_msg, &response_msg).await
                    }
                    GaiseLiveInput::ToolResponse {
                        call_id,
                        name: _,
                        result,
                    } => {
                        let item_msg = OpenAIRealtimeItemCreate {
                            r#type: "conversation.item.create".to_string(),
                            item: OpenAIRealtimeItem {
                                r#type: "function_call_output".to_string(),
                                role: None,
                                content: None,
                                call_id: Some(call_id),
                                output: Some(result.to_string()),
                            },
                        };
                        let response_msg = OpenAIRealtimeResponseCreate {
                            r#type: "response.create".to_string(),
                        };
                        send_two_messages(&mut ws_sink, &item_msg, &response_msg).await
                    }
                    GaiseLiveInput::ActivityStart => Ok(()),
                    GaiseLiveInput::ActivityEnd | GaiseLiveInput::AudioStreamEnd => {
                        if manual_vad {
                            let commit = OpenAIRealtimeSimpleEvent {
                                r#type: "input_audio_buffer.commit".to_string(),
                            };
                            let response = OpenAIRealtimeResponseCreate {
                                r#type: "response.create".to_string(),
                            };
                            send_two_messages(&mut ws_sink, &commit, &response).await
                        } else {
                            Ok(())
                        }
                    }
                    GaiseLiveInput::ClearAudio => {
                        let msg = OpenAIRealtimeSimpleEvent {
                            r#type: "input_audio_buffer.clear".to_string(),
                        };
                        match serde_json::to_string(&msg) {
                            Ok(json) => ws_sink
                                .send(Message::Text(json.into()))
                                .await
                                .map_err(|e| e.into()),
                            Err(e) => Err(e.into()),
                        }
                    }
                    GaiseLiveInput::CancelResponse => {
                        let msg = OpenAIRealtimeSimpleEvent {
                            r#type: "response.cancel".to_string(),
                        };
                        match serde_json::to_string(&msg) {
                            Ok(json) => ws_sink
                                .send(Message::Text(json.into()))
                                .await
                                .map_err(|e| e.into()),
                            Err(e) => Err(e.into()),
                        }
                    }
                    GaiseLiveInput::Close => {
                        let _ = ws_sink.close().await;
                        break;
                    }
                };

                if let Err(e) = result {
                    let _ = event_tx_send
                        .send(Ok(GaiseLiveEvent::Error {
                            message: format!("Send error: {}", e),
                        }))
                        .await;
                    break;
                }
            }
        });

        // Spawn receive loop
        tokio::spawn(async move {
            while let Some(msg) = ws_source.next().await {
                let msg = match msg {
                    Ok(m) => m,
                    Err(e) => {
                        let _ = event_tx
                            .send(Ok(GaiseLiveEvent::Error {
                                message: format!("WebSocket error: {}", e),
                            }))
                            .await;
                        break;
                    }
                };

                let text = match msg {
                    Message::Text(t) => t,
                    Message::Close(_) => {
                        let _ = event_tx.send(Ok(GaiseLiveEvent::SessionEnded)).await;
                        break;
                    }
                    _ => continue,
                };

                let event: OpenAIRealtimeServerEvent = match serde_json::from_str(&text) {
                    Ok(e) => e,
                    Err(e) => {
                        let _ = event_tx
                            .send(Ok(GaiseLiveEvent::Error {
                                message: format!("Parse error: {}", e),
                            }))
                            .await;
                        continue;
                    }
                };

                match event.r#type.as_str() {
                    // Audio output
                    "response.output_audio.delta" | "response.audio.delta" => {
                        if let Some(delta) = &event.delta
                            && let Ok(audio_bytes) = base64::prelude::BASE64_STANDARD.decode(delta)
                        {
                            let _ = event_tx
                                .send(Ok(GaiseLiveEvent::Audio {
                                    data: audio_bytes,
                                    sample_rate: 24000,
                                }))
                                .await;
                        }
                    }

                    // Text output
                    "response.output_text.delta" | "response.text.delta" => {
                        if let Some(delta) = &event.delta {
                            let _ = event_tx
                                .send(Ok(GaiseLiveEvent::Text {
                                    text: delta.clone(),
                                }))
                                .await;
                        }
                    }

                    // Audio transcript (model speech as text)
                    "response.output_audio_transcript.delta"
                    | "response.audio_transcript.delta" => {
                        if let Some(delta) = &event.delta {
                            let _ = event_tx
                                .send(Ok(GaiseLiveEvent::Transcript {
                                    role: "assistant".to_string(),
                                    text: delta.clone(),
                                }))
                                .await;
                        }
                    }

                    // Input audio transcription
                    "conversation.item.input_audio_transcription.completed" => {
                        if let Some(transcript) = &event.transcript {
                            let _ = event_tx
                                .send(Ok(GaiseLiveEvent::Transcript {
                                    role: "user".to_string(),
                                    text: transcript.clone(),
                                }))
                                .await;
                        }
                        if let Some(usage) = &event.usage {
                            let _ = event_tx
                                .send(Ok(GaiseLiveEvent::Usage(map_transcription_usage(usage))))
                                .await;
                        }
                    }

                    // Tool call completed
                    "response.function_call_arguments.done" => {
                        if let (Some(call_id), Some(name)) = (&event.call_id, &event.name) {
                            let _ = event_tx
                                .send(Ok(GaiseLiveEvent::ToolCall {
                                    id: call_id.clone(),
                                    function: GaiseFunctionCall {
                                        name: name.clone(),
                                        arguments: event.arguments.clone(),
                                    },
                                }))
                                .await;
                        }
                    }

                    // Response done (turn complete)
                    "response.done" => {
                        // Extract usage if available
                        if let Some(resp) = &event.response
                            && let Some(usage) = &resp.usage
                        {
                            let _ = event_tx
                                .send(Ok(GaiseLiveEvent::Usage(map_realtime_usage(usage))))
                                .await;
                        }
                        let _ = event_tx.send(Ok(GaiseLiveEvent::TurnComplete)).await;
                    }

                    // Speech stopped (barge-in)
                    "input_audio_buffer.speech_started" => {
                        let _ = event_tx.send(Ok(GaiseLiveEvent::Interrupted)).await;
                    }

                    // Error
                    "error" => {
                        let message = event
                            .error
                            .as_ref()
                            .and_then(|e| e.message.clone())
                            .unwrap_or_else(|| "Unknown error".to_string());
                        let _ = event_tx.send(Ok(GaiseLiveEvent::Error { message })).await;
                    }

                    // Ignore other event types (session.created, session.updated, etc.)
                    _ => {}
                }
            }
        });

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
    fn maps_realtime_usage_with_input_and_output_modalities() {
        let usage: OpenAIRealtimeResponseUsage = serde_json::from_value(serde_json::json!({
            "total_tokens": 230,
            "input_tokens": 100,
            "output_tokens": 130,
            "input_token_details": {
                "cached_tokens": 12,
                "text_tokens": 40,
                "audio_tokens": 35,
                "image_tokens": 25,
                "cached_tokens_details": {
                    "text_tokens": 5,
                    "audio_tokens": 4,
                    "image_tokens": 3
                }
            },
            "output_token_details": {
                "text_tokens": 50,
                "audio_tokens": 80
            }
        }))
        .unwrap();

        let mapped = map_realtime_usage(&usage);
        let input = mapped.input.unwrap();
        let output = mapped.output.unwrap();
        assert_eq!(input.get("text_tokens"), Some(&40));
        assert_eq!(input.get("image_tokens"), Some(&25));
        assert_eq!(input.get("audio_tokens"), Some(&35));
        assert_eq!(input.get("cached_text_tokens"), Some(&5));
        assert_eq!(input.get("cached_audio_tokens"), Some(&4));
        assert_eq!(input.get("cached_image_tokens"), Some(&3));
        assert_eq!(output.get("text_tokens"), Some(&50));
        assert_eq!(output.get("audio_tokens"), Some(&80));
        assert!(!output.contains_key("total_tokens"));
        assert_eq!(mapped.total.unwrap().get("total_tokens"), Some(&230));
    }

    #[test]
    fn maps_separately_billed_transcription_usage_without_overwriting_turn_usage() {
        let usage: OpenAIRealtimeTranscriptionUsage = serde_json::from_value(serde_json::json!({
            "type": "tokens",
            "input_tokens": 10,
            "output_tokens": 4,
            "total_tokens": 14,
            "input_token_details": {"text_tokens": 2, "audio_tokens": 8}
        }))
        .unwrap();
        let mapped = map_transcription_usage(&usage);
        assert_eq!(
            mapped
                .input
                .as_ref()
                .unwrap()
                .get("transcription_audio_tokens"),
            Some(&8)
        );
        assert_eq!(
            mapped
                .output
                .as_ref()
                .unwrap()
                .get("transcription_output_tokens"),
            Some(&4)
        );
        assert_eq!(
            mapped
                .total
                .as_ref()
                .unwrap()
                .get("transcription_total_tokens"),
            Some(&14)
        );

        let duration: OpenAIRealtimeTranscriptionUsage =
            serde_json::from_value(serde_json::json!({
                "type": "duration",
                "seconds": 3.25
            }))
            .unwrap();
        assert_eq!(
            map_transcription_usage(&duration)
                .input
                .unwrap()
                .get("transcription_audio_milliseconds"),
            Some(&3250)
        );
    }
}
