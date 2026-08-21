//! Gemini API `GET /v1beta/models` contract and mapping.
//!
//! `models.list` reports token limits, `supportedGenerationMethods` and a
//! `thinking` flag. Methods tell us which GAISe operation applies
//! (`generateContent` → instruct, `embedContent` → embeddings,
//! `bidiGenerateContent` → live) but say nothing about input/output
//! modalities beyond text, which the registry overlay supplies.

use gaise_core::contracts::{
    GaiseMetadataSource, GaiseModality, GaiseModel, GaiseModelStatus, GaiseOperation, GaiseSupport,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiModelList {
    #[serde(default)]
    pub models: Vec<GeminiModelInfo>,
    #[serde(default)]
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiModelInfo {
    /// Resource name, `models/{id}`.
    pub name: String,
    #[serde(default)]
    pub base_model_id: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub input_token_limit: Option<u64>,
    #[serde(default)]
    pub output_token_limit: Option<u64>,
    #[serde(default)]
    pub supported_generation_methods: Vec<String>,
    #[serde(default)]
    pub thinking: Option<bool>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub top_k: Option<u64>,
}

/// Strip the `models/` resource prefix.
pub fn gemini_model_id(name: &str) -> &str {
    name.strip_prefix("models/").unwrap_or(name)
}

/// Map one Gemini model record to the common record.
pub fn map_gemini_model(model: &GeminiModelInfo, include_raw: bool) -> GaiseModel {
    map_google_model("gemini", model, include_raw)
}

/// Shared mapper for Gemini-style model resources (used by Vertex AI too).
pub fn map_google_model(provider: &str, model: &GeminiModelInfo, include_raw: bool) -> GaiseModel {
    let id = gemini_model_id(&model.name).to_string();
    let mut out = GaiseModel::new(provider, id.clone());
    out.display_name = model.display_name.clone();
    out.description = model.description.clone();
    // Google documents an input limit and a separate output limit; the input
    // limit is the figure its model pages call the context window.
    out.limits.max_input_tokens = model.input_token_limit.filter(|v| *v > 0);
    out.limits.context_window = out.limits.max_input_tokens;
    out.limits.max_output_tokens = model.output_token_limit.filter(|v| *v > 0);
    let lower = id.to_ascii_lowercase();
    out.status = if lower.contains("preview") || lower.contains("-exp") {
        GaiseModelStatus::Preview
    } else {
        GaiseModelStatus::Active
    };

    let caps = &mut out.capabilities;
    caps.add_source(GaiseMetadataSource::Provider);
    let methods = &model.supported_generation_methods;
    let has = |m: &str| methods.iter().any(|x| x == m);
    if has("generateContent") {
        caps.add_input(GaiseModality::Text);
        caps.add_output(GaiseModality::Text);
        caps.add_operation(GaiseOperation::Instruct);
        caps.add_operation(GaiseOperation::InstructStream);
    }
    if has("embedContent") || has("batchEmbedContents") {
        caps.add_input(GaiseModality::Text);
        caps.add_output(GaiseModality::Embedding);
        caps.add_operation(GaiseOperation::Embeddings);
    }
    if has("bidiGenerateContent") {
        caps.add_input(GaiseModality::Text);
        caps.add_input(GaiseModality::Audio);
        caps.add_output(GaiseModality::Text);
        caps.add_output(GaiseModality::Audio);
        caps.add_operation(GaiseOperation::Live);
    }
    // `predict` / `predictLongRunning` (Imagen, Veo) have no GAISe surface.
    caps.reasoning = GaiseSupport::from_option(model.thinking);
    if has("generateContent") && lower.contains("image") && !lower.contains("embedding") {
        // Native image-generation models advertise themselves by name only.
        caps.add_input(GaiseModality::Image);
        caps.add_output(GaiseModality::Image);
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
      "models": [
        {
          "name": "models/gemini-3.6-flash",
          "version": "001",
          "displayName": "Gemini 3.6 Flash",
          "description": "Fast multimodal model",
          "inputTokenLimit": 1048576,
          "outputTokenLimit": 65536,
          "supportedGenerationMethods": ["generateContent", "countTokens", "createCachedContent", "batchGenerateContent"],
          "thinking": true,
          "temperature": 1,
          "topP": 0.95,
          "topK": 64,
          "maxTemperature": 2
        },
        {
          "name": "models/gemini-embedding-2",
          "displayName": "Gemini Embedding 2",
          "inputTokenLimit": 8192,
          "outputTokenLimit": 1,
          "supportedGenerationMethods": ["embedContent", "batchEmbedContents"]
        },
        {
          "name": "models/gemini-3.1-flash-live-preview",
          "displayName": "Gemini 3.1 Flash Live",
          "inputTokenLimit": 131072,
          "outputTokenLimit": 8192,
          "supportedGenerationMethods": ["bidiGenerateContent", "countTokens"],
          "thinking": true
        },
        {
          "name": "models/gemini-3.1-flash-image",
          "displayName": "Gemini 3.1 Flash Image",
          "inputTokenLimit": 65536,
          "outputTokenLimit": 32768,
          "supportedGenerationMethods": ["generateContent", "countTokens"],
          "thinking": true
        },
        {
          "name": "models/imagen-4.0-generate-001",
          "displayName": "Imagen 4",
          "supportedGenerationMethods": ["predict"]
        }
      ],
      "nextPageToken": "abc"
    }"#;

    #[test]
    fn maps_methods_to_operations() {
        let list: GeminiModelList = serde_json::from_str(FIXTURE).unwrap();
        assert_eq!(list.next_page_token.as_deref(), Some("abc"));
        let models: Vec<GaiseModel> = list
            .models
            .iter()
            .map(|m| map_gemini_model(m, false))
            .collect();

        assert_eq!(models[0].id, "gemini-3.6-flash");
        assert_eq!(models[0].provider, "gemini");
        assert_eq!(
            models[0].capabilities.operations,
            vec![GaiseOperation::Instruct, GaiseOperation::InstructStream]
        );
        assert_eq!(models[0].capabilities.reasoning, GaiseSupport::Supported);
        assert_eq!(models[0].limits.max_input_tokens, Some(1_048_576));
        assert_eq!(models[0].limits.context_window, Some(1_048_576));
        assert_eq!(models[0].limits.max_output_tokens, Some(65_536));
        assert_eq!(models[0].status, GaiseModelStatus::Active);
        assert_eq!(
            models[0].capabilities.input,
            vec![GaiseModality::Text],
            "modalities beyond text come from the registry"
        );

        assert_eq!(
            models[1].capabilities.operations,
            vec![GaiseOperation::Embeddings]
        );
        assert_eq!(models[1].capabilities.reasoning, GaiseSupport::Unknown);

        assert_eq!(
            models[2].capabilities.operations,
            vec![GaiseOperation::Live]
        );
        assert_eq!(models[2].status, GaiseModelStatus::Preview);
        assert!(
            models[2]
                .capabilities
                .output
                .contains(&GaiseModality::Audio)
        );

        assert!(
            models[3]
                .capabilities
                .output
                .contains(&GaiseModality::Image)
        );
        assert!(
            models[3]
                .capabilities
                .sources
                .contains(&GaiseMetadataSource::Heuristic)
        );

        assert!(models[4].capabilities.operations.is_empty());
        assert!(models[4].capabilities.input.is_empty());
    }
}
