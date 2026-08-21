//! Text-to-speech contracts.
//!
//! `speech` renders a complete clip; `speech_stream` yields audio as the
//! provider produces it. Realtime *input* streaming (send text fragments,
//! receive audio as they are voiced) reuses the live-session protocol:
//! [`crate::GaiseLiveClient::live_connect`] with `GaiseLiveInput::Text`
//! fragments in and `GaiseLiveEvent::Audio` out.

use serde::{Deserialize, Serialize};

use super::GaiseUsage;

/// Provider-neutral voice tuning. Every field is optional; adapters map what
/// their API supports and omit the rest.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GaiseVoiceSettings {
    /// 0.0–1.0; higher is more consistent, lower is more expressive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stability: Option<f32>,
    /// 0.0–1.0; how closely to track the reference voice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f32>,
    /// 0.0–1.0 style exaggeration where the model supports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_boost: Option<bool>,
    /// Playback speed multiplier (1.0 = normal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GaiseSpeechRequest {
    /// `provider::model` through the router, bare model ID through a direct client.
    pub model: String,
    /// Text to voice.
    pub input: String,
    /// Provider voice identifier (ElevenLabs `voice_id`, OpenAI voice name, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    /// Requested output encoding. Accepts a MIME type (`audio/mpeg`,
    /// `audio/wav`, `audio/pcm`) or a provider-native format string
    /// (`mp3_44100_128`, `pcm_24000`). Adapters normalize via
    /// [`audio_media_type`](super::audio_media_type) and their own tables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Requested PCM sample rate when `format` is a raw PCM MIME type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    /// ISO 639-1 language hint where the model accepts one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_settings: Option<GaiseVoiceSettings>,
    /// Free-text delivery instructions where the model accepts them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Deterministic seed where supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// Ask for character/word timing alongside the audio where supported.
    #[serde(default)]
    pub include_alignment: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// Per-request provider endpoint/credential overrides (take precedence
    /// over the router configuration for this call only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<super::GaiseConnection>,
}

/// Character timing returned by providers that support alignment.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GaiseSpeechAlignment {
    pub characters: Vec<String>,
    pub start_seconds: Vec<f64>,
    pub end_seconds: Vec<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GaiseSpeechResponse {
    /// Encoded audio bytes.
    #[serde(with = "serde_bytes")]
    pub audio: Vec<u8>,
    /// MIME type of `audio` (`audio/mpeg`, `audio/pcm`, ...).
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<GaiseSpeechAlignment>,
    /// Provider-named counters (`characters`, `credits`, ...) when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<GaiseUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GaiseSpeechChunk {
    /// A slice of encoded audio in the response `format`.
    Audio {
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
        format: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sample_rate: Option<u32>,
    },
    Alignment(GaiseSpeechAlignment),
    Usage(GaiseUsage),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GaiseSpeechStreamResponse {
    pub chunk: GaiseSpeechChunk,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_compactly() {
        let request = GaiseSpeechRequest {
            model: "elevenlabs::eleven_v3".into(),
            input: "Hello".into(),
            voice: Some("21m00Tcm4TlvDq8ikWAM".into()),
            format: Some("audio/mpeg".into()),
            ..Default::default()
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["voice"], "21m00Tcm4TlvDq8ikWAM");
        assert!(json.get("language").is_none());
        assert_eq!(json["include_alignment"], false);

        let chunk = GaiseSpeechStreamResponse {
            chunk: GaiseSpeechChunk::Audio {
                data: vec![1, 2, 3],
                format: "audio/mpeg".into(),
                sample_rate: None,
            },
            external_id: None,
        };
        let json = serde_json::to_value(&chunk).unwrap();
        assert_eq!(json["chunk"]["audio"]["data"], serde_json::json!([1, 2, 3]));
    }
}
