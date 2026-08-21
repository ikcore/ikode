//! Ollama `GET /api/tags` and `POST /api/show` contracts and mapping.
//!
//! `/api/tags` lists installed tags with family/size metadata only. The
//! per-model `/api/show` call returns a `capabilities` array (`completion`,
//! `vision`, `tools`, `embedding`, `thinking`, `insert`) and `model_info`
//! with context and embedding lengths; it costs one request per tag, so it is
//! only fetched when `include_details` is requested.

use gaise_core::contracts::{
    GaiseMetadataSource, GaiseModality, GaiseModel, GaiseModelStatus, GaiseOperation, GaiseSupport,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaTagList {
    #[serde(default)]
    pub models: Vec<OllamaTag>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaTag {
    pub name: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub modified_at: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    pub details: Option<OllamaModelDetails>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OllamaModelDetails {
    #[serde(default)]
    pub parent_model: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub families: Option<Vec<String>>,
    #[serde(default)]
    pub parameter_size: Option<String>,
    #[serde(default)]
    pub quantization_level: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OllamaShowRequest {
    pub model: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OllamaShowResponse {
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub details: Option<OllamaModelDetails>,
    #[serde(default)]
    pub model_info: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    pub parameters: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub modified_at: Option<String>,
}

/// Map an installed tag (no `/api/show` detail yet).
pub fn map_ollama_tag(tag: &OllamaTag, include_raw: bool) -> GaiseModel {
    let mut out = GaiseModel::new("ollama", tag.name.clone());
    out.created_at = tag.modified_at.clone();
    out.status = GaiseModelStatus::Active;
    if let Some(details) = &tag.details {
        let mut bits = Vec::new();
        if let Some(family) = &details.family {
            bits.push(family.clone());
        }
        if let Some(size) = &details.parameter_size {
            bits.push(size.clone());
        }
        if let Some(q) = &details.quantization_level {
            bits.push(q.clone());
        }
        if !bits.is_empty() {
            out.description = Some(bits.join(" "));
        }
    }
    out.capabilities.add_source(GaiseMetadataSource::Provider);
    if include_raw {
        out.raw = serde_json::to_value(tag).ok();
    }
    out
}

/// Apply `/api/show` detail onto a tag record.
pub fn apply_ollama_show(model: &mut GaiseModel, show: &OllamaShowResponse, include_raw: bool) {
    let caps = &mut model.capabilities;
    let has = |name: &str| show.capabilities.iter().any(|c| c == name);
    if has("completion") {
        caps.add_input(GaiseModality::Text);
        caps.add_output(GaiseModality::Text);
        caps.add_operation(GaiseOperation::Instruct);
        caps.add_operation(GaiseOperation::InstructStream);
    }
    if has("vision") {
        caps.add_input(GaiseModality::Image);
    }
    if has("embedding") {
        caps.add_input(GaiseModality::Text);
        caps.add_output(GaiseModality::Embedding);
        caps.add_operation(GaiseOperation::Embeddings);
    }
    if !show.capabilities.is_empty() {
        caps.tools = GaiseSupport::from_bool(has("tools"));
        caps.reasoning = GaiseSupport::from_bool(has("thinking"));
    }
    if let Some(info) = &show.model_info {
        for (key, value) in info {
            if key.ends_with(".context_length")
                && let Some(v) = value.as_u64()
            {
                // The architecture's trained window; Ollama serves it with a
                // smaller default `num_ctx` unless the request raises it.
                model.limits.context_window = Some(v);
                model.limits.max_input_tokens = Some(v);
            }
            if key.ends_with(".embedding_length")
                && let Some(v) = value.as_u64()
                && caps.supports(GaiseOperation::Embeddings)
            {
                model.limits.embedding_dimensions = Some(v as u32);
            }
        }
    }
    if include_raw && let Some(raw) = model.raw.as_mut().and_then(|r| r.as_object_mut()) {
        let mut show_value = serde_json::to_value(show).unwrap_or(serde_json::Value::Null);
        // Drop bulky prose fields from the raw copy.
        if let Some(obj) = show_value.as_object_mut() {
            obj.remove("template");
            obj.remove("parameters");
        }
        raw.insert("show".to_string(), show_value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_tags_and_show_details() {
        let tags: OllamaTagList = serde_json::from_str(
            r#"{"models":[{"name":"qwen3:8b","model":"qwen3:8b","modified_at":"2026-08-01T10:00:00Z","size":5,"digest":"abc",
               "details":{"format":"gguf","family":"qwen3","families":["qwen3"],"parameter_size":"8.2B","quantization_level":"Q4_K_M"}},
               {"name":"nomic-embed-text:latest","details":{"family":"nomic-bert","parameter_size":"137M","quantization_level":"F16"}}]}"#,
        )
        .unwrap();
        let mut qwen = map_ollama_tag(&tags.models[0], true);
        assert_eq!(qwen.description.as_deref(), Some("qwen3 8.2B Q4_K_M"));
        assert!(
            qwen.capabilities.operations.is_empty(),
            "nothing known before /api/show"
        );

        let show: OllamaShowResponse = serde_json::from_str(
            r#"{"capabilities":["completion","tools","thinking"],"details":{"family":"qwen3"},
               "model_info":{"general.architecture":"qwen3","qwen3.context_length":40960,"qwen3.embedding_length":4096},
               "template":"{{ .Prompt }}","parameters":"stop \"<|im_end|>\""}"#,
        )
        .unwrap();
        apply_ollama_show(&mut qwen, &show, true);
        assert_eq!(
            qwen.capabilities.operations,
            vec![GaiseOperation::Instruct, GaiseOperation::InstructStream]
        );
        assert_eq!(qwen.capabilities.tools, GaiseSupport::Supported);
        assert_eq!(qwen.capabilities.reasoning, GaiseSupport::Supported);
        assert_eq!(qwen.limits.max_input_tokens, Some(40_960));
        assert_eq!(qwen.limits.context_window, Some(40_960));
        assert_eq!(
            qwen.limits.embedding_dimensions, None,
            "only meaningful for embedding models"
        );
        assert!(qwen.raw.as_ref().unwrap()["show"].get("template").is_none());

        let mut embed = map_ollama_tag(&tags.models[1], false);
        let show: OllamaShowResponse = serde_json::from_str(
            r#"{"capabilities":["embedding"],"model_info":{"nomic-bert.context_length":2048,"nomic-bert.embedding_length":768}}"#,
        )
        .unwrap();
        apply_ollama_show(&mut embed, &show, false);
        assert_eq!(
            embed.capabilities.operations,
            vec![GaiseOperation::Embeddings]
        );
        assert_eq!(embed.limits.embedding_dimensions, Some(768));
        assert_eq!(embed.capabilities.tools, GaiseSupport::Unsupported);
        assert!(embed.raw.is_none());

        let mut vision = GaiseModel::new("ollama", "gemma4:latest");
        apply_ollama_show(
            &mut vision,
            &OllamaShowResponse {
                capabilities: vec!["completion".into(), "vision".into()],
                ..Default::default()
            },
            false,
        );
        assert_eq!(
            vision.capabilities.input,
            vec![GaiseModality::Text, GaiseModality::Image]
        );
    }
}
