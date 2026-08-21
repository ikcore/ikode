//! OpenAI `GET /v1/models` contract and mapping.
//!
//! The OpenAI model object carries almost no capability metadata (`id`,
//! `created`, `owned_by`, optional `shutdown_date`). Everything else in the
//! mapped [`GaiseModel`] is either a name heuristic (tagged
//! [`GaiseMetadataSource::Heuristic`]) or comes from the registry overlay
//! applied later by the router.

use gaise_core::contracts::{
    GaiseMetadataSource, GaiseModality, GaiseModel, GaiseModelStatus, GaiseOperation,
    rfc3339_from_unix,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIModelList {
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub data: Vec<OpenAIModelObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIModelObject {
    pub id: String,
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub created: Option<i64>,
    #[serde(default)]
    pub owned_by: Option<String>,
    /// Announced discontinuation date (`YYYY-MM-DD`) when present.
    #[serde(default)]
    pub shutdown_date: Option<String>,
}

/// Rough surface classification from the identifier alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAIModelKind {
    Chat,
    Embeddings,
    Realtime,
    Audio,
    Image,
    /// Speech, transcription, moderation, legacy completions, ... — no GAISe
    /// surface drives these.
    Other,
}

/// Classify an OpenAI model id. Conservative: only patterns with a single
/// well-known meaning are recognised; anything else is `Other` and left for
/// the registry overlay.
pub fn classify_openai_model_id(id: &str) -> OpenAIModelKind {
    let id = id.to_ascii_lowercase();
    let base = id.strip_prefix("ft:").unwrap_or(&id);
    const OTHER_PREFIXES: &[&str] = &[
        "tts-",
        "whisper-",
        "gpt-4o-mini-tts",
        "gpt-4o-transcribe",
        "gpt-4o-mini-transcribe",
        "gpt-transcribe",
        "gpt-live-transcribe",
        "gpt-realtime-whisper",
        "gpt-realtime-translate",
        "omni-moderation",
        "text-moderation",
        "babbage",
        "davinci",
        "computer-use",
        "sora",
        "daybreak",
    ];
    if OTHER_PREFIXES.iter().any(|p| base.starts_with(p)) {
        return OpenAIModelKind::Other;
    }
    if base.starts_with("text-embedding") {
        return OpenAIModelKind::Embeddings;
    }
    if base.starts_with("gpt-image")
        || base.starts_with("dall-e")
        || base.starts_with("chatgpt-image")
    {
        return OpenAIModelKind::Image;
    }
    if base.contains("realtime") {
        return OpenAIModelKind::Realtime;
    }
    if base.contains("audio") {
        return OpenAIModelKind::Audio;
    }
    if base.starts_with("gpt-")
        || base.starts_with("chatgpt-")
        || base.starts_with("o1")
        || base.starts_with("o3")
        || base.starts_with("o4")
        || base.starts_with("codex")
    {
        return OpenAIModelKind::Chat;
    }
    OpenAIModelKind::Other
}

/// Map one model object to the common record.
pub fn map_openai_model(model: &OpenAIModelObject, include_raw: bool) -> GaiseModel {
    let mut out = GaiseModel::new("openai", model.id.clone());
    out.created_at = model.created.map(rfc3339_from_unix);
    if let Some(date) = &model.shutdown_date {
        out.retires_on = Some(date.clone());
        out.status = GaiseModelStatus::Deprecated;
    }
    if let Some(owner) = &model.owned_by
        && owner != "openai"
        && owner != "system"
        && !owner.starts_with("openai-")
    {
        out.notes = Some(format!("owned by {owner}"));
    }
    out.capabilities.add_source(GaiseMetadataSource::Provider);

    let caps = &mut out.capabilities;
    match classify_openai_model_id(&model.id) {
        OpenAIModelKind::Chat => {
            caps.add_input(GaiseModality::Text);
            caps.add_output(GaiseModality::Text);
            caps.add_operation(GaiseOperation::Instruct);
            caps.add_operation(GaiseOperation::InstructStream);
        }
        OpenAIModelKind::Embeddings => {
            caps.add_input(GaiseModality::Text);
            caps.add_output(GaiseModality::Embedding);
            caps.add_operation(GaiseOperation::Embeddings);
        }
        OpenAIModelKind::Realtime => {
            caps.add_input(GaiseModality::Text);
            caps.add_input(GaiseModality::Audio);
            caps.add_output(GaiseModality::Text);
            caps.add_output(GaiseModality::Audio);
            caps.add_operation(GaiseOperation::Live);
        }
        OpenAIModelKind::Audio => {
            caps.add_input(GaiseModality::Text);
            caps.add_input(GaiseModality::Audio);
            caps.add_output(GaiseModality::Text);
            caps.add_output(GaiseModality::Audio);
            caps.add_operation(GaiseOperation::Instruct);
            caps.add_operation(GaiseOperation::InstructStream);
        }
        OpenAIModelKind::Image => {
            caps.add_input(GaiseModality::Text);
            caps.add_input(GaiseModality::Image);
            caps.add_output(GaiseModality::Image);
            // Images API / Responses tooling only; no instruct surface.
        }
        OpenAIModelKind::Other => {}
    }
    if !caps.input.is_empty() || !caps.operations.is_empty() {
        caps.add_source(GaiseMetadataSource::Heuristic);
    }

    if include_raw {
        out.raw = serde_json::to_value(model).ok();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
      "object": "list",
      "data": [
        {"id": "gpt-5.6", "object": "model", "created": 1784851200, "owned_by": "system"},
        {"id": "text-embedding-3-small", "object": "model", "created": 1705948997, "owned_by": "system"},
        {"id": "gpt-realtime-2.1", "object": "model", "created": 1784851200, "owned_by": "system"},
        {"id": "gpt-audio-1.5", "object": "model", "created": 1784851200, "owned_by": "system"},
        {"id": "gpt-image-2", "object": "model", "created": 1784851200, "owned_by": "system"},
        {"id": "whisper-1", "object": "model", "created": 1677532384, "owned_by": "openai-internal"},
        {"id": "gpt-5-chat-latest", "object": "model", "created": 1754524800, "owned_by": "system", "shutdown_date": "2026-07-23"},
        {"id": "ft:gpt-5.4-mini:acme::abc123", "object": "model", "created": 1784851200, "owned_by": "acme"}
      ]
    }"#;

    #[test]
    fn parses_list_and_maps_heuristics() {
        let list: OpenAIModelList = serde_json::from_str(FIXTURE).unwrap();
        assert_eq!(list.data.len(), 8);
        let models: Vec<GaiseModel> = list
            .data
            .iter()
            .map(|m| map_openai_model(m, false))
            .collect();

        let chat = &models[0];
        assert_eq!(chat.id, "gpt-5.6");
        assert_eq!(chat.created_at.as_deref(), Some("2026-07-24T00:00:00Z"));
        assert_eq!(
            chat.capabilities.operations,
            vec![GaiseOperation::Instruct, GaiseOperation::InstructStream]
        );
        assert_eq!(
            chat.capabilities.sources,
            vec![
                GaiseMetadataSource::Provider,
                GaiseMetadataSource::Heuristic
            ]
        );
        assert_eq!(chat.status, GaiseModelStatus::Unknown);

        assert_eq!(
            models[1].capabilities.operations,
            vec![GaiseOperation::Embeddings]
        );
        assert_eq!(
            models[1].capabilities.output,
            vec![GaiseModality::Embedding]
        );
        assert_eq!(
            models[2].capabilities.operations,
            vec![GaiseOperation::Live]
        );
        assert!(models[3].capabilities.input.contains(&GaiseModality::Audio));
        assert!(models[3].capabilities.supports(GaiseOperation::Instruct));
        assert!(models[4].capabilities.operations.is_empty());
        assert_eq!(models[4].capabilities.output, vec![GaiseModality::Image]);
        assert!(models[5].capabilities.operations.is_empty());
        assert!(models[5].capabilities.input.is_empty());
        assert_eq!(
            models[5].capabilities.sources,
            vec![GaiseMetadataSource::Provider]
        );

        assert_eq!(models[6].status, GaiseModelStatus::Deprecated);
        assert_eq!(models[6].retires_on.as_deref(), Some("2026-07-23"));

        assert!(models[7].capabilities.supports(GaiseOperation::Instruct));
        assert_eq!(models[7].notes.as_deref(), Some("owned by acme"));
        assert!(models[7].raw.is_none());
    }

    #[test]
    fn raw_is_attached_on_request() {
        let m = OpenAIModelObject {
            id: "gpt-5.6".into(),
            object: Some("model".into()),
            created: Some(1),
            owned_by: Some("system".into()),
            shutdown_date: None,
        };
        let mapped = map_openai_model(&m, true);
        assert_eq!(mapped.raw.unwrap()["id"], "gpt-5.6");
    }

    #[test]
    fn classification_is_conservative() {
        assert_eq!(
            classify_openai_model_id("gpt-4o-mini-tts"),
            OpenAIModelKind::Other
        );
        assert_eq!(
            classify_openai_model_id("gpt-4o-mini-realtime-preview"),
            OpenAIModelKind::Realtime
        );
        assert_eq!(
            classify_openai_model_id("gpt-realtime-translate"),
            OpenAIModelKind::Other
        );
        assert_eq!(classify_openai_model_id("o3-pro"), OpenAIModelKind::Chat);
        assert_eq!(
            classify_openai_model_id("chatgpt-image-latest"),
            OpenAIModelKind::Image
        );
        assert_eq!(
            classify_openai_model_id("text-embedding-3-large"),
            OpenAIModelKind::Embeddings
        );
        assert_eq!(
            classify_openai_model_id("mystery-model"),
            OpenAIModelKind::Other
        );
    }
}
