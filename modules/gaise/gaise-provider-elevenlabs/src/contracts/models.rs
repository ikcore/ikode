//! ElevenLabs wire types and pure mapping helpers.
//!
//! Surfaces (API reference audited 2026-08-20):
//! - `POST /v1/text-to-speech/{voice_id}` (binary audio) and
//!   `/with-timestamps` (JSON with base64 audio + character alignment)
//! - `POST /v1/text-to-speech/{voice_id}/stream` (chunked audio) and
//!   `/stream/with-timestamps` (newline-delimited JSON objects)
//! - `wss://…/v1/text-to-speech/{voice_id}/stream-input` (realtime text in,
//!   audio out; not available for `eleven_v3*`)
//! - `wss://…/v1/text-to-dialogue/stream-input` (realtime for `eleven_v3*`)
//! - `GET /v1/models`

use gaise_core::contracts::{
    GaiseMetadataSource, GaiseModality, GaiseModel, GaiseModelStatus, GaiseOperation,
    GaiseSpeechAlignment, GaiseSpeechRequest, GaiseSupport, GaiseVoiceSettings,
};
use serde::{Deserialize, Serialize};

// ── Output formats ────────────────────────────────────────────────────────

/// Resolved ElevenLabs `output_format` with its MIME type and sample rate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElevenLabsOutputFormat {
    pub output_format: String,
    pub mime: String,
    pub sample_rate: u32,
}

/// Map a provider-neutral `format` / `sample_rate` pair onto an ElevenLabs
/// `output_format`. Provider-native strings (`pcm_24000`, `mp3_44100_128`)
/// pass through. Defaults to `mp3_44100_128`.
pub fn resolve_output_format(
    format: Option<&str>,
    sample_rate: Option<u32>,
) -> Result<ElevenLabsOutputFormat, String> {
    let raw = format.map(|f| f.trim().to_ascii_lowercase());
    let raw = raw.as_deref().unwrap_or("audio/mpeg");

    // Provider-native value.
    if let Some((codec, rest)) = raw.split_once('_')
        && matches!(codec, "mp3" | "pcm" | "wav" | "opus" | "ulaw" | "alaw")
    {
        let rate: u32 = rest
            .split('_')
            .next()
            .and_then(|r| r.parse().ok())
            .ok_or_else(|| format!("unrecognised ElevenLabs output_format '{raw}'"))?;
        return Ok(ElevenLabsOutputFormat {
            output_format: raw.to_string(),
            mime: mime_for_codec(codec).to_string(),
            sample_rate: rate,
        });
    }

    let codec = match raw {
        "audio/mpeg" | "audio/mp3" | "mp3" | "mpeg" => "mp3",
        "audio/pcm" | "audio/l16" | "pcm" | "pcm16" | "raw" => "pcm",
        "audio/wav" | "audio/x-wav" | "audio/wave" | "wav" => "wav",
        "audio/opus" | "audio/ogg" | "opus" => "opus",
        "audio/basic" | "audio/ulaw" | "ulaw" | "mulaw" => "ulaw",
        "audio/alaw" | "alaw" => "alaw",
        other => return Err(format!("ElevenLabs cannot produce audio format '{other}'")),
    };
    let (output_format, rate) = match codec {
        "mp3" => match sample_rate {
            Some(22_050) => ("mp3_22050_32".to_string(), 22_050),
            Some(24_000) => ("mp3_24000_48".to_string(), 24_000),
            _ => ("mp3_44100_128".to_string(), 44_100),
        },
        "pcm" | "wav" => {
            let rate = sample_rate.unwrap_or(24_000);
            if !matches!(
                rate,
                8_000 | 16_000 | 22_050 | 24_000 | 32_000 | 44_100 | 48_000
            ) {
                return Err(format!(
                    "ElevenLabs {codec} output supports 8000, 16000, 22050, 24000, 32000, 44100, or 48000 Hz, not {rate}"
                ));
            }
            (format!("{codec}_{rate}"), rate)
        }
        "opus" => ("opus_48000_64".to_string(), 48_000),
        "ulaw" => ("ulaw_8000".to_string(), 8_000),
        _ => ("alaw_8000".to_string(), 8_000),
    };
    Ok(ElevenLabsOutputFormat {
        output_format,
        mime: mime_for_codec(codec).to_string(),
        sample_rate: rate,
    })
}

fn mime_for_codec(codec: &str) -> &'static str {
    match codec {
        "mp3" => "audio/mpeg",
        "pcm" => "audio/pcm",
        "wav" => "audio/wav",
        "opus" => "audio/opus",
        "ulaw" => "audio/basic",
        _ => "audio/alaw",
    }
}

// ── REST request / response ───────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ElevenLabsVoiceSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stability: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity_boost: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_speaker_boost: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
}

impl From<&GaiseVoiceSettings> for ElevenLabsVoiceSettings {
    fn from(v: &GaiseVoiceSettings) -> Self {
        Self {
            stability: v.stability,
            similarity_boost: v.similarity,
            style: v.style,
            use_speaker_boost: v.speaker_boost,
            // Documented range 0.7–1.2.
            speed: v.speed.map(|s| s.clamp(0.7, 1.2)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElevenLabsTtsRequest {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_settings: Option<ElevenLabsVoiceSettings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// `language_code` is rejected by `eleven_multilingual_v2`; other current
/// models accept it.
pub fn model_accepts_language_code(model: &str) -> bool {
    !model
        .to_ascii_lowercase()
        .starts_with("eleven_multilingual_v2")
}

/// `eleven_v3*` models use the text-to-dialogue WebSocket; every other TTS
/// model uses `stream-input`.
pub fn model_uses_dialogue_websocket(model: &str) -> bool {
    model.to_ascii_lowercase().starts_with("eleven_v3")
}

/// Build the REST body for a speech request. The voice travels in the path.
pub fn build_tts_request(request: &GaiseSpeechRequest) -> ElevenLabsTtsRequest {
    ElevenLabsTtsRequest {
        text: request.input.clone(),
        model_id: Some(request.model.clone()).filter(|m| !m.is_empty()),
        language_code: request
            .language
            .clone()
            .filter(|_| model_accepts_language_code(&request.model)),
        voice_settings: request
            .voice_settings
            .as_ref()
            .map(ElevenLabsVoiceSettings::from),
        seed: request.seed.map(|s| s.min(u32::MAX as u64)),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ElevenLabsAlignment {
    #[serde(default)]
    pub characters: Vec<String>,
    #[serde(default)]
    pub character_start_times_seconds: Vec<f64>,
    #[serde(default)]
    pub character_end_times_seconds: Vec<f64>,
}

impl From<&ElevenLabsAlignment> for GaiseSpeechAlignment {
    fn from(a: &ElevenLabsAlignment) -> Self {
        Self {
            characters: a.characters.clone(),
            start_seconds: a.character_start_times_seconds.clone(),
            end_seconds: a.character_end_times_seconds.clone(),
        }
    }
}

/// `/with-timestamps` response and one line of `/stream/with-timestamps`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ElevenLabsTimestampedAudio {
    #[serde(default)]
    pub audio_base64: Option<String>,
    #[serde(default)]
    pub alignment: Option<ElevenLabsAlignment>,
    #[serde(default)]
    pub normalized_alignment: Option<ElevenLabsAlignment>,
}

/// Parse one newline-delimited frame from `/stream/with-timestamps`,
/// tolerating an optional `data:` SSE prefix and blank lines.
pub fn parse_timestamp_line(
    line: &str,
) -> Result<Option<ElevenLabsTimestampedAudio>, serde_json::Error> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let payload = trimmed
        .strip_prefix("data:")
        .map(str::trim)
        .unwrap_or(trimmed);
    if payload.is_empty() || payload == "[DONE]" {
        return Ok(None);
    }
    serde_json::from_str(payload).map(Some)
}

/// ElevenLabs error envelope. `detail` is an object for API errors and an
/// array of field errors for FastAPI validation failures.
#[derive(Debug, Clone, Deserialize)]
pub struct ElevenLabsErrorEnvelope {
    #[serde(default)]
    pub detail: serde_json::Value,
}

/// Human-readable message from an error body, falling back to the raw text.
pub fn format_error(status: u16, body: &str) -> String {
    let parsed: Option<ElevenLabsErrorEnvelope> = serde_json::from_str(body).ok();
    let message = parsed.and_then(|e| match e.detail {
        serde_json::Value::Object(map) => {
            let status_code = map
                .get("status")
                .or_else(|| map.get("code"))
                .and_then(|v| v.as_str())
                .unwrap_or("error");
            let message = map.get("message").and_then(|v| v.as_str()).unwrap_or("");
            Some(format!("{status_code}: {message}"))
        }
        serde_json::Value::Array(items) => Some(
            items
                .iter()
                .map(|i| {
                    let loc = i["loc"]
                        .as_array()
                        .map(|l| {
                            l.iter()
                                .filter_map(|x| {
                                    x.as_str()
                                        .map(str::to_string)
                                        .or_else(|| x.as_u64().map(|n| n.to_string()))
                                })
                                .collect::<Vec<_>>()
                                .join(".")
                        })
                        .unwrap_or_default();
                    format!("{loc}: {}", i["msg"].as_str().unwrap_or("invalid"))
                })
                .collect::<Vec<_>>()
                .join("; "),
        ),
        serde_json::Value::String(s) => Some(s),
        _ => None,
    });
    match message {
        Some(m) if !m.is_empty() => format!("ElevenLabs API error ({status}): {m}"),
        _ => format!("ElevenLabs API error ({status}): {body}"),
    }
}

// ── Models ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ElevenLabsLanguage {
    #[serde(default)]
    pub language_id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ElevenLabsModelInfo {
    pub model_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub can_do_text_to_speech: Option<bool>,
    #[serde(default)]
    pub can_do_voice_conversion: Option<bool>,
    #[serde(default)]
    pub can_use_style: Option<bool>,
    #[serde(default)]
    pub can_use_speaker_boost: Option<bool>,
    #[serde(default)]
    pub serves_pro_voices: Option<bool>,
    #[serde(default)]
    pub requires_alpha_access: Option<bool>,
    #[serde(default)]
    pub token_cost_factor: Option<f64>,
    #[serde(default)]
    pub maximum_text_length_per_request: Option<u64>,
    #[serde(default)]
    pub max_characters_request_free_user: Option<u64>,
    #[serde(default)]
    pub max_characters_request_subscribed_user: Option<u64>,
    #[serde(default)]
    pub languages: Vec<ElevenLabsLanguage>,
    #[serde(default)]
    pub concurrency_group: Option<String>,
}

/// Map one `/v1/models` entry to the common record.
pub fn map_elevenlabs_model(model: &ElevenLabsModelInfo, include_raw: bool) -> GaiseModel {
    let mut out = GaiseModel::new("elevenlabs", model.model_id.clone());
    out.display_name = model.name.clone();
    out.description = model.description.clone();
    out.status = if model.requires_alpha_access == Some(true) {
        GaiseModelStatus::Preview
    } else {
        GaiseModelStatus::Active
    };
    // Speech is bounded by characters, not tokens.
    out.limits.max_input_characters = model.maximum_text_length_per_request;
    let caps = &mut out.capabilities;
    caps.add_source(GaiseMetadataSource::Provider);
    caps.tools = GaiseSupport::Unsupported;
    caps.reasoning = GaiseSupport::Unsupported;
    caps.structured_output = GaiseSupport::Unsupported;
    if model.can_do_text_to_speech == Some(true) {
        caps.add_input(GaiseModality::Text);
        caps.add_output(GaiseModality::Audio);
        caps.add_operation(GaiseOperation::Speech);
        // Every current TTS model has a realtime WebSocket: stream-input, or
        // text-to-dialogue for eleven_v3*.
        caps.add_operation(GaiseOperation::Live);
    }
    if model.can_do_voice_conversion == Some(true) {
        caps.add_input(GaiseModality::Audio);
    }
    if let Some(limit) = model.maximum_text_length_per_request {
        out.notes = Some(format!("max {limit} characters per request"));
    }
    if include_raw {
        out.raw = serde_json::to_value(model).ok();
    }
    out
}

// ── WebSocket frames ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct StreamInputGenerationConfig {
    pub chunk_length_schedule: Vec<u32>,
}

/// First frame on `stream-input`: a single space plus the voice settings.
#[derive(Debug, Clone, Serialize)]
pub struct StreamInputInit {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_settings: Option<ElevenLabsVoiceSettings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<StreamInputGenerationConfig>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamInputText {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flush: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamInputAlignment {
    #[serde(default)]
    pub chars: Vec<String>,
    #[serde(default)]
    pub char_start_times_ms: Vec<f64>,
    #[serde(default)]
    pub char_durations_ms: Vec<f64>,
}

impl From<&StreamInputAlignment> for GaiseSpeechAlignment {
    fn from(a: &StreamInputAlignment) -> Self {
        let start: Vec<f64> = a.char_start_times_ms.iter().map(|ms| ms / 1000.0).collect();
        let end = start
            .iter()
            .zip(a.char_durations_ms.iter())
            .map(|(s, d)| s + d / 1000.0)
            .collect();
        Self {
            characters: a.chars.clone(),
            start_seconds: start,
            end_seconds: end,
        }
    }
}

/// Server frame on `stream-input`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamInputEvent {
    #[serde(default)]
    pub audio: Option<String>,
    #[serde(default)]
    pub is_final: Option<bool>,
    #[serde(default)]
    pub alignment: Option<StreamInputAlignment>,
    #[serde(default)]
    pub normalized_alignment: Option<StreamInputAlignment>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub code: Option<i64>,
}

/// First frame on the text-to-dialogue WebSocket.
#[derive(Debug, Clone, Serialize)]
pub struct DialogueInit {
    pub voices: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_settings: Option<ElevenLabsVoiceSettings>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DialogueInputItem {
    pub text: String,
    pub voice_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DialogueInputs {
    pub inputs: Vec<DialogueInputItem>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DialogueAlignment {
    #[serde(default)]
    pub chars: Vec<String>,
    #[serde(default)]
    pub char_start_times_ms: Vec<f64>,
    #[serde(default)]
    pub char_durations_ms: Vec<f64>,
}

/// Server frame on the text-to-dialogue WebSocket (snake_case).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DialogueEvent {
    #[serde(default)]
    pub audio: Option<String>,
    #[serde(default)]
    pub alignment: Option<DialogueAlignment>,
    #[serde(default)]
    pub is_final: Option<bool>,
    #[serde(default)]
    pub is_final_audio_for_turn: Option<bool>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub code: Option<i64>,
}

impl From<&DialogueAlignment> for GaiseSpeechAlignment {
    fn from(a: &DialogueAlignment) -> Self {
        let start: Vec<f64> = a.char_start_times_ms.iter().map(|ms| ms / 1000.0).collect();
        let end = start
            .iter()
            .zip(a.char_durations_ms.iter())
            .map(|(s, d)| s + d / 1000.0)
            .collect();
        Self {
            characters: a.chars.clone(),
            start_seconds: start,
            end_seconds: end,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_output_formats() {
        let f = resolve_output_format(None, None).unwrap();
        assert_eq!(
            (f.output_format.as_str(), f.mime.as_str(), f.sample_rate),
            ("mp3_44100_128", "audio/mpeg", 44_100)
        );
        let f = resolve_output_format(Some("audio/pcm"), Some(16_000)).unwrap();
        assert_eq!(
            (f.output_format.as_str(), f.mime.as_str(), f.sample_rate),
            ("pcm_16000", "audio/pcm", 16_000)
        );
        let f = resolve_output_format(Some("wav"), None).unwrap();
        assert_eq!(f.output_format, "wav_24000");
        let f = resolve_output_format(Some("pcm_24000"), None).unwrap();
        assert_eq!((f.mime.as_str(), f.sample_rate), ("audio/pcm", 24_000));
        let f = resolve_output_format(Some("opus"), Some(24_000)).unwrap();
        assert_eq!(f.output_format, "opus_48000_64");
        let f = resolve_output_format(Some("audio/basic"), None).unwrap();
        assert_eq!(f.output_format, "ulaw_8000");
        assert!(resolve_output_format(Some("audio/pcm"), Some(11_025)).is_err());
        assert!(resolve_output_format(Some("audio/flac"), None).is_err());
    }

    #[test]
    fn builds_tts_request_with_model_rules() {
        let request = GaiseSpeechRequest {
            model: "eleven_multilingual_v2".into(),
            input: "Hello".into(),
            language: Some("de".into()),
            voice_settings: Some(GaiseVoiceSettings {
                stability: Some(0.4),
                similarity: Some(0.8),
                speed: Some(2.0),
                ..Default::default()
            }),
            seed: Some(42),
            ..Default::default()
        };
        let body = serde_json::to_value(build_tts_request(&request)).unwrap();
        assert_eq!(body["model_id"], "eleven_multilingual_v2");
        assert!(
            body.get("language_code").is_none(),
            "multilingual_v2 rejects language_code"
        );
        assert!((body["voice_settings"]["similarity_boost"].as_f64().unwrap() - 0.8).abs() < 1e-6);
        assert!(
            (body["voice_settings"]["speed"].as_f64().unwrap() - 1.2).abs() < 1e-6,
            "speed is clamped to 1.2"
        );
        assert!(body["voice_settings"].get("style").is_none());
        assert_eq!(body["seed"], 42);

        let flash = GaiseSpeechRequest {
            model: "eleven_flash_v2_5".into(),
            input: "Hello".into(),
            language: Some("de".into()),
            ..Default::default()
        };
        let body = serde_json::to_value(build_tts_request(&flash)).unwrap();
        assert_eq!(body["language_code"], "de");
        assert!(body.get("voice_settings").is_none());
        assert!(model_uses_dialogue_websocket("eleven_v3"));
        assert!(model_uses_dialogue_websocket("eleven_v3_conversational"));
        assert!(!model_uses_dialogue_websocket("eleven_flash_v2_5"));
    }

    #[test]
    fn parses_timestamp_frames_and_errors() {
        let line = r#"data: {"audio_base64":"AAEC","alignment":{"characters":["H","i"],"character_start_times_seconds":[0.0,0.1],"character_end_times_seconds":[0.1,0.2]}}"#;
        let frame = parse_timestamp_line(line).unwrap().unwrap();
        assert_eq!(frame.audio_base64.as_deref(), Some("AAEC"));
        let alignment: GaiseSpeechAlignment = (&frame.alignment.unwrap()).into();
        assert_eq!(alignment.characters, vec!["H", "i"]);
        assert!(parse_timestamp_line("").unwrap().is_none());
        assert!(parse_timestamp_line("data: [DONE]").unwrap().is_none());
        let bare = parse_timestamp_line(r#"{"audio_base64":null,"alignment":null}"#)
            .unwrap()
            .unwrap();
        assert!(bare.audio_base64.is_none());

        let msg = format_error(
            401,
            r#"{"detail":{"status":"invalid_api_key","message":"Invalid API key"}}"#,
        );
        assert_eq!(
            msg,
            "ElevenLabs API error (401): invalid_api_key: Invalid API key"
        );
        let msg = format_error(
            422,
            r#"{"detail":[{"loc":["body","text"],"msg":"field required","type":"value_error"}]}"#,
        );
        assert!(msg.contains("body.text: field required"));
        let msg = format_error(500, "upstream exploded");
        assert!(msg.ends_with("upstream exploded"));
    }

    #[test]
    fn maps_models() {
        let list: Vec<ElevenLabsModelInfo> = serde_json::from_str(
            r#"[{"model_id":"eleven_v3","name":"Eleven v3","can_do_text_to_speech":true,"can_use_style":false,
                 "maximum_text_length_per_request":5000,"languages":[{"language_id":"en","name":"English"}]},
                {"model_id":"eleven_multilingual_sts_v2","name":"STS","can_do_text_to_speech":false,"can_do_voice_conversion":true},
                {"model_id":"eleven_experimental","can_do_text_to_speech":true,"requires_alpha_access":true}]"#,
        )
        .unwrap();
        let models: Vec<GaiseModel> = list.iter().map(|m| map_elevenlabs_model(m, true)).collect();
        assert_eq!(models[0].id, "eleven_v3");
        assert_eq!(models[0].provider, "elevenlabs");
        assert_eq!(models[0].capabilities.input, vec![GaiseModality::Text]);
        assert_eq!(models[0].capabilities.output, vec![GaiseModality::Audio]);
        assert_eq!(
            models[0].capabilities.operations,
            vec![GaiseOperation::Speech, GaiseOperation::Live]
        );
        assert_eq!(
            models[0].notes.as_deref(),
            Some("max 5000 characters per request")
        );
        assert_eq!(models[0].limits.max_input_characters, Some(5000));
        assert_eq!(models[0].limits.context_window, None);
        assert_eq!(
            models[0].raw.as_ref().unwrap()["languages"][0]["language_id"],
            "en"
        );
        assert!(models[1].capabilities.operations.is_empty());
        assert_eq!(models[1].capabilities.input, vec![GaiseModality::Audio]);
        assert_eq!(models[2].status, GaiseModelStatus::Preview);
    }

    #[test]
    fn converts_websocket_alignment_to_seconds() {
        let event: StreamInputEvent = serde_json::from_str(
            r#"{"audio":"AAEC","alignment":{"chars":["H","i"],"charStartTimesMs":[0,100],"charDurationsMs":[100,50]},"isFinal":null}"#,
        )
        .unwrap();
        let alignment: GaiseSpeechAlignment = (&event.alignment.unwrap()).into();
        assert_eq!(alignment.start_seconds, vec![0.0, 0.1]);
        assert!((alignment.end_seconds[1] - 0.15).abs() < 1e-9);
        let done: StreamInputEvent = serde_json::from_str(r#"{"isFinal":true}"#).unwrap();
        assert_eq!(done.is_final, Some(true));
        let dialogue: DialogueEvent = serde_json::from_str(
            r#"{"audio":"AA==","alignment":{"chars":["a"],"char_start_times_ms":[10],"char_durations_ms":[5]},"is_final_audio_for_turn":true}"#,
        )
        .unwrap();
        assert_eq!(dialogue.is_final_audio_for_turn, Some(true));
        let err: DialogueEvent = serde_json::from_str(
            r#"{"message":"bad voice","error":"voice_not_found","code":4000}"#,
        )
        .unwrap();
        assert_eq!(err.error.as_deref(), Some("voice_not_found"));
    }
}
