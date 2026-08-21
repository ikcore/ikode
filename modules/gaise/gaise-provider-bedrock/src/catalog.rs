//! Bedrock `ListFoundationModels` / `ListInferenceProfiles` mapping.
//!
//! The control-plane API reports input/output modalities (`TEXT`, `IMAGE`,
//! `EMBEDDING`), streaming support, inference types and lifecycle status per
//! foundation model. It says nothing about tool use, reasoning, documents,
//! audio or video, so those stay unknown until the registry overlay runs.
//!
//! Cross-region inference profiles (`us.`, `eu.`, `global.` ...) are what
//! callers usually invoke; they are listed as models in their own right with
//! the capabilities of the foundation model they front.

use gaise_core::contracts::{
    GaiseMetadataSource, GaiseModality, GaiseModel, GaiseModelStatus, GaiseOperation, GaiseSupport,
};
use serde::{Deserialize, Serialize};

/// SDK-independent view of `FoundationModelSummary`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BedrockModelSummary {
    pub model_id: String,
    pub model_arn: String,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub provider_name: Option<String>,
    #[serde(default)]
    pub input_modalities: Vec<String>,
    #[serde(default)]
    pub output_modalities: Vec<String>,
    #[serde(default)]
    pub response_streaming_supported: Option<bool>,
    #[serde(default)]
    pub inference_types_supported: Vec<String>,
    #[serde(default)]
    pub lifecycle_status: Option<String>,
}

/// SDK-independent view of `InferenceProfileSummary`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BedrockInferenceProfile {
    pub inference_profile_id: String,
    pub inference_profile_arn: String,
    #[serde(default)]
    pub inference_profile_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub profile_type: Option<String>,
    #[serde(default)]
    pub model_arns: Vec<String>,
}

impl From<&aws_sdk_bedrock::types::FoundationModelSummary> for BedrockModelSummary {
    fn from(m: &aws_sdk_bedrock::types::FoundationModelSummary) -> Self {
        Self {
            model_id: m.model_id().to_string(),
            model_arn: m.model_arn().to_string(),
            model_name: m.model_name().map(str::to_string),
            provider_name: m.provider_name().map(str::to_string),
            input_modalities: m
                .input_modalities()
                .iter()
                .map(|x| x.as_str().to_string())
                .collect(),
            output_modalities: m
                .output_modalities()
                .iter()
                .map(|x| x.as_str().to_string())
                .collect(),
            response_streaming_supported: m.response_streaming_supported(),
            inference_types_supported: m
                .inference_types_supported()
                .iter()
                .map(|x| x.as_str().to_string())
                .collect(),
            lifecycle_status: m.model_lifecycle().map(|l| l.status().as_str().to_string()),
        }
    }
}

impl From<&aws_sdk_bedrock::types::InferenceProfileSummary> for BedrockInferenceProfile {
    fn from(p: &aws_sdk_bedrock::types::InferenceProfileSummary) -> Self {
        Self {
            inference_profile_id: p.inference_profile_id().to_string(),
            inference_profile_arn: p.inference_profile_arn().to_string(),
            inference_profile_name: Some(p.inference_profile_name().to_string()),
            description: p.description().map(str::to_string),
            status: Some(p.status().as_str().to_string()),
            profile_type: Some(p.r#type().as_str().to_string()),
            model_arns: p
                .models()
                .iter()
                .filter_map(|m| m.model_arn().map(str::to_string))
                .collect(),
        }
    }
}

/// `arn:aws:bedrock:REGION::foundation-model/ID` → `ID`.
pub fn model_id_from_arn(arn: &str) -> &str {
    match arn
        .rsplit_once("/foundation-model/")
        .or_else(|| arn.rsplit_once("foundation-model/"))
    {
        Some((_, id)) => id,
        None => arn,
    }
}

fn modality(value: &str) -> Option<GaiseModality> {
    match value {
        "TEXT" => Some(GaiseModality::Text),
        "IMAGE" => Some(GaiseModality::Image),
        "EMBEDDING" => Some(GaiseModality::Embedding),
        _ => None,
    }
}

/// Map one foundation model summary to the common record.
pub fn map_foundation_model(summary: &BedrockModelSummary, include_raw: bool) -> GaiseModel {
    let mut out = GaiseModel::new("bedrock", summary.model_id.clone());
    out.display_name = summary.model_name.clone();
    out.description = summary.provider_name.clone();
    out.status = match summary.lifecycle_status.as_deref() {
        Some("ACTIVE") => GaiseModelStatus::Active,
        Some("LEGACY") => GaiseModelStatus::Legacy,
        _ => GaiseModelStatus::Unknown,
    };
    if !summary.inference_types_supported.is_empty()
        && !summary
            .inference_types_supported
            .iter()
            .any(|t| t == "ON_DEMAND" || t == "INFERENCE_PROFILE")
    {
        out.notes = Some(format!(
            "inference types: {}",
            summary.inference_types_supported.join(", ")
        ));
    }

    let caps = &mut out.capabilities;
    caps.add_source(GaiseMetadataSource::Provider);
    for m in summary.input_modalities.iter().filter_map(|v| modality(v)) {
        if m != GaiseModality::Embedding {
            caps.add_input(m);
        }
    }
    for m in summary.output_modalities.iter().filter_map(|v| modality(v)) {
        caps.add_output(m);
    }
    let text_in = caps.input.contains(&GaiseModality::Text);
    if caps.output.contains(&GaiseModality::Embedding) {
        caps.add_operation(GaiseOperation::Embeddings);
    } else if text_in && caps.output.contains(&GaiseModality::Text) {
        caps.add_operation(GaiseOperation::Instruct);
        if summary.response_streaming_supported == Some(true) {
            caps.add_operation(GaiseOperation::InstructStream);
        }
    }
    let _ = GaiseSupport::Unknown; // tools/reasoning stay unknown: not reported by the API

    if include_raw {
        out.raw = serde_json::to_value(summary).ok();
    }
    out
}

/// Turn inference profiles into models that inherit their foundation model's
/// capabilities. Profiles whose foundation model is not in `foundation` are
/// still listed, with identity only.
pub fn map_inference_profiles(
    profiles: &[BedrockInferenceProfile],
    foundation: &[GaiseModel],
    include_raw: bool,
) -> Vec<GaiseModel> {
    profiles
        .iter()
        .filter(|p| p.status.as_deref().is_none_or(|s| s == "ACTIVE"))
        .map(|p| {
            let base = p
                .model_arns
                .iter()
                .map(|arn| model_id_from_arn(arn))
                .find_map(|id| foundation.iter().find(|m| m.id == id));
            let mut out = match base {
                Some(base) => {
                    let mut m = base.clone();
                    m.id = p.inference_profile_id.clone();
                    m.raw = None;
                    m
                }
                None => {
                    let mut m = GaiseModel::new("bedrock", p.inference_profile_id.clone());
                    m.capabilities.add_source(GaiseMetadataSource::Provider);
                    m.status = GaiseModelStatus::Active;
                    m
                }
            };
            out.display_name = p.inference_profile_name.clone().or(out.display_name);
            let kind = match p.profile_type.as_deref() {
                Some("SYSTEM_DEFINED") => "cross-region inference profile",
                Some("APPLICATION") => "application inference profile",
                _ => "inference profile",
            };
            out.notes = Some(match &out.notes {
                Some(existing) => format!("{kind}; {existing}"),
                None => kind.to_string(),
            });
            if include_raw {
                out.raw = serde_json::to_value(p).ok();
            }
            out
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude() -> BedrockModelSummary {
        BedrockModelSummary {
            model_id: "anthropic.claude-sonnet-4-5-20250929-v1:0".into(),
            model_arn: "arn:aws:bedrock:us-east-1::foundation-model/anthropic.claude-sonnet-4-5-20250929-v1:0".into(),
            model_name: Some("Claude Sonnet 4.5".into()),
            provider_name: Some("Anthropic".into()),
            input_modalities: vec!["TEXT".into(), "IMAGE".into()],
            output_modalities: vec!["TEXT".into()],
            response_streaming_supported: Some(true),
            inference_types_supported: vec!["INFERENCE_PROFILE".into()],
            lifecycle_status: Some("ACTIVE".into()),
        }
    }

    #[test]
    fn maps_modalities_streaming_and_lifecycle() {
        let model = map_foundation_model(&claude(), false);
        assert_eq!(
            model.capabilities.input,
            vec![GaiseModality::Text, GaiseModality::Image]
        );
        assert_eq!(model.capabilities.output, vec![GaiseModality::Text]);
        assert_eq!(
            model.capabilities.operations,
            vec![GaiseOperation::Instruct, GaiseOperation::InstructStream]
        );
        assert_eq!(model.capabilities.tools, GaiseSupport::Unknown);
        assert_eq!(model.status, GaiseModelStatus::Active);
        assert_eq!(model.display_name.as_deref(), Some("Claude Sonnet 4.5"));
        assert!(model.notes.is_none());

        let embed = BedrockModelSummary {
            model_id: "amazon.titan-embed-text-v2:0".into(),
            model_arn: "arn:aws:bedrock:us-east-1::foundation-model/amazon.titan-embed-text-v2:0"
                .into(),
            input_modalities: vec!["TEXT".into()],
            output_modalities: vec!["EMBEDDING".into()],
            response_streaming_supported: Some(false),
            inference_types_supported: vec!["ON_DEMAND".into()],
            lifecycle_status: Some("ACTIVE".into()),
            ..Default::default()
        };
        let embed = map_foundation_model(&embed, true);
        assert_eq!(
            embed.capabilities.operations,
            vec![GaiseOperation::Embeddings]
        );
        assert_eq!(embed.capabilities.output, vec![GaiseModality::Embedding]);
        assert_eq!(
            embed.raw.unwrap()["model_id"],
            "amazon.titan-embed-text-v2:0"
        );

        let legacy = BedrockModelSummary {
            model_id: "amazon.nova-canvas-v1:0".into(),
            input_modalities: vec!["TEXT".into(), "IMAGE".into()],
            output_modalities: vec!["IMAGE".into()],
            inference_types_supported: vec!["PROVISIONED".into()],
            lifecycle_status: Some("LEGACY".into()),
            ..Default::default()
        };
        let legacy = map_foundation_model(&legacy, false);
        assert_eq!(legacy.status, GaiseModelStatus::Legacy);
        assert!(legacy.capabilities.operations.is_empty());
        assert_eq!(
            legacy.notes.as_deref(),
            Some("inference types: PROVISIONED")
        );
    }

    #[test]
    fn inference_profiles_inherit_foundation_capabilities() {
        let foundation = vec![map_foundation_model(&claude(), false)];
        let profiles = vec![
            BedrockInferenceProfile {
                inference_profile_id: "us.anthropic.claude-sonnet-4-5-20250929-v1:0".into(),
                inference_profile_arn: "arn:aws:bedrock:us-east-1:123:inference-profile/us.anthropic.claude-sonnet-4-5-20250929-v1:0".into(),
                inference_profile_name: Some("US Claude Sonnet 4.5".into()),
                status: Some("ACTIVE".into()),
                profile_type: Some("SYSTEM_DEFINED".into()),
                model_arns: vec![
                    "arn:aws:bedrock:us-east-1::foundation-model/anthropic.claude-sonnet-4-5-20250929-v1:0".into(),
                    "arn:aws:bedrock:us-west-2::foundation-model/anthropic.claude-sonnet-4-5-20250929-v1:0".into(),
                ],
                ..Default::default()
            },
            BedrockInferenceProfile {
                inference_profile_id: "global.something.new-v1:0".into(),
                inference_profile_arn: "arn".into(),
                status: Some("ACTIVE".into()),
                profile_type: Some("SYSTEM_DEFINED".into()),
                model_arns: vec!["arn:aws:bedrock:us-east-1::foundation-model/something.new-v1:0".into()],
                ..Default::default()
            },
            BedrockInferenceProfile {
                inference_profile_id: "inactive".into(),
                status: Some("INACTIVE".into()),
                ..Default::default()
            },
        ];
        let mapped = map_inference_profiles(&profiles, &foundation, false);
        assert_eq!(mapped.len(), 2);
        assert_eq!(mapped[0].id, "us.anthropic.claude-sonnet-4-5-20250929-v1:0");
        assert_eq!(
            mapped[0].display_name.as_deref(),
            Some("US Claude Sonnet 4.5")
        );
        assert_eq!(
            mapped[0].capabilities.input,
            vec![GaiseModality::Text, GaiseModality::Image]
        );
        assert!(
            mapped[0]
                .capabilities
                .supports(GaiseOperation::InstructStream)
        );
        assert_eq!(
            mapped[0].notes.as_deref(),
            Some("cross-region inference profile")
        );
        assert!(mapped[1].capabilities.operations.is_empty());
        assert_eq!(
            model_id_from_arn(
                "arn:aws:bedrock:us-east-1::foundation-model/amazon.nova-2-lite-v1:0"
            ),
            "amazon.nova-2-lite-v1:0"
        );
    }
}
