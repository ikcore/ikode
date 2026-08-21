//! Vertex AI Model Garden `publishers.models.list` contract and mapping.
//!
//! Vertex AI has no modality metadata on its publisher-model resources: the
//! `v1beta1` list (`ModelGardenService.ListPublisherModels`) returns names,
//! version ids and launch stages only. The adapter therefore reports identity
//! and lifecycle from the provider, applies a narrow name heuristic for the
//! Gemini/embedding families it can drive, and leaves the rest for the
//! registry overlay.

use gaise_core::contracts::{
    GaiseMetadataSource, GaiseModality, GaiseModel, GaiseModelStatus, GaiseOperation,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VertexPublisherModelList {
    #[serde(default)]
    pub publisher_models: Vec<VertexPublisherModel>,
    #[serde(default)]
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VertexPublisherModel {
    /// Resource name, `publishers/{publisher}/models/{model}`.
    pub name: String,
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub open_source_category: Option<String>,
    #[serde(default)]
    pub launch_stage: Option<String>,
    #[serde(default)]
    pub version_state: Option<String>,
    #[serde(default)]
    pub publisher_model_template: Option<String>,
    #[serde(default)]
    pub supported_actions: Option<serde_json::Value>,
    #[serde(default)]
    pub frameworks: Vec<String>,
}

/// Where the listing call goes, derived from the configured request template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VertexCatalogEndpoint {
    /// `https://{region}-aiplatform.googleapis.com`
    pub host: String,
    /// `google`, `anthropic`, ...
    pub publisher: String,
}

impl VertexCatalogEndpoint {
    /// Derive host and publisher from the `{{MODEL}}` request template, e.g.
    /// `https://us-central1-aiplatform.googleapis.com/v1/projects/P/locations/L/publishers/google/models/{{MODEL}}`.
    pub fn from_template(template: &str) -> Result<Self, String> {
        let scheme_end = template
            .find("://")
            .ok_or_else(|| format!("Vertex AI URL template has no scheme: {template}"))?;
        let after_scheme = &template[scheme_end + 3..];
        let host_len = after_scheme.find('/').unwrap_or(after_scheme.len());
        let host = format!(
            "{}{}",
            &template[..scheme_end + 3],
            &after_scheme[..host_len]
        );
        let publisher = template
            .split('/')
            .collect::<Vec<_>>()
            .windows(2)
            .find(|w| w[0] == "publishers")
            .map(|w| w[1].to_string())
            .filter(|p| !p.is_empty())
            .ok_or_else(|| {
                format!("Vertex AI URL template has no publishers/<publisher> segment: {template}")
            })?;
        Ok(Self { host, publisher })
    }

    pub fn list_url(&self, page_token: Option<&str>) -> String {
        let mut url = format!(
            "{}/v1beta1/publishers/{}/models?pageSize=1000",
            self.host, self.publisher
        );
        if let Some(token) = page_token {
            url.push_str("&pageToken=");
            url.push_str(token);
        }
        url
    }
}

/// Strip `publishers/{publisher}/models/`.
pub fn vertex_model_id(name: &str) -> &str {
    match name.rsplit_once("/models/") {
        Some((_, id)) => id,
        None => name,
    }
}

fn map_launch_stage(stage: Option<&str>) -> GaiseModelStatus {
    match stage {
        Some("GA") => GaiseModelStatus::Active,
        Some("PUBLIC_PREVIEW") | Some("PRIVATE_PREVIEW") | Some("EXPERIMENTAL") => {
            GaiseModelStatus::Preview
        }
        _ => GaiseModelStatus::Unknown,
    }
}

/// Map one publisher model to the common record.
pub fn map_vertex_model(model: &VertexPublisherModel, include_raw: bool) -> GaiseModel {
    let id = vertex_model_id(&model.name).to_string();
    let mut out = GaiseModel::new("vertexai", id.clone());
    out.status = map_launch_stage(model.launch_stage.as_deref());
    if out.status == GaiseModelStatus::Unknown && id.to_ascii_lowercase().contains("preview") {
        out.status = GaiseModelStatus::Preview;
    }
    if let Some(version) = &model.version_id {
        out.description = Some(format!("version {version}"));
    }
    let caps = &mut out.capabilities;
    caps.add_source(GaiseMetadataSource::Provider);

    let lower = id.to_ascii_lowercase();
    if lower.contains("embedding") {
        caps.add_input(GaiseModality::Text);
        caps.add_output(GaiseModality::Embedding);
        caps.add_operation(GaiseOperation::Embeddings);
        caps.add_source(GaiseMetadataSource::Heuristic);
    } else if lower.starts_with("gemini-") || lower.starts_with("claude-") {
        if lower.contains("-live") || lower.contains("-tts") || lower.contains("-omni") {
            // Live/TTS/video surfaces are not driven by the Vertex adapter.
        } else {
            caps.add_input(GaiseModality::Text);
            caps.add_output(GaiseModality::Text);
            caps.add_operation(GaiseOperation::Instruct);
            caps.add_operation(GaiseOperation::InstructStream);
            caps.add_source(GaiseMetadataSource::Heuristic);
        }
    }

    if include_raw {
        out.raw = serde_json::to_value(model).ok();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_catalog_endpoint_from_template() {
        let ep = VertexCatalogEndpoint::from_template(
            "https://us-central1-aiplatform.googleapis.com/v1/projects/p/locations/us-central1/publishers/google/models/{{MODEL}}",
        )
        .unwrap();
        assert_eq!(ep.host, "https://us-central1-aiplatform.googleapis.com");
        assert_eq!(ep.publisher, "google");
        assert_eq!(
            ep.list_url(None),
            "https://us-central1-aiplatform.googleapis.com/v1beta1/publishers/google/models?pageSize=1000"
        );
        assert!(ep.list_url(Some("tok")).ends_with("&pageToken=tok"));

        let anthropic = VertexCatalogEndpoint::from_template(
            "https://europe-west1-aiplatform.googleapis.com/v1/projects/p/locations/europe-west1/publishers/anthropic/models/{{MODEL}}",
        )
        .unwrap();
        assert_eq!(anthropic.publisher, "anthropic");
        assert!(VertexCatalogEndpoint::from_template("https://host/v1/models/{{MODEL}}").is_err());
        assert!(VertexCatalogEndpoint::from_template("nonsense").is_err());
    }

    #[test]
    fn maps_publisher_models() {
        let list: VertexPublisherModelList = serde_json::from_str(
            r#"{"publisherModels":[
              {"name":"publishers/google/models/gemini-3.6-flash","versionId":"001","launchStage":"GA","versionState":"STABLE","supportedActions":{"openGenerationAiStudio":{}}},
              {"name":"publishers/google/models/gemini-3.1-pro-preview","launchStage":"PUBLIC_PREVIEW"},
              {"name":"publishers/google/models/gemini-embedding-2","launchStage":"GA"},
              {"name":"publishers/google/models/imagen-4.0-generate-001","launchStage":"GA"},
              {"name":"publishers/google/models/gemini-live-2.5-flash-native-audio","launchStage":"GA"}
            ],"nextPageToken":"n"}"#,
        )
        .unwrap();
        let models: Vec<GaiseModel> = list
            .publisher_models
            .iter()
            .map(|m| map_vertex_model(m, true))
            .collect();
        assert_eq!(models[0].id, "gemini-3.6-flash");
        assert_eq!(models[0].provider, "vertexai");
        assert_eq!(models[0].status, GaiseModelStatus::Active);
        assert_eq!(models[0].description.as_deref(), Some("version 001"));
        assert_eq!(
            models[0].capabilities.operations,
            vec![GaiseOperation::Instruct, GaiseOperation::InstructStream]
        );
        assert_eq!(models[0].raw.as_ref().unwrap()["launchStage"], "GA");
        assert_eq!(models[1].status, GaiseModelStatus::Preview);
        assert_eq!(
            models[2].capabilities.operations,
            vec![GaiseOperation::Embeddings]
        );
        assert!(models[3].capabilities.operations.is_empty());
        assert!(
            models[4].capabilities.operations.is_empty(),
            "no Vertex Live adapter"
        );
    }
}
