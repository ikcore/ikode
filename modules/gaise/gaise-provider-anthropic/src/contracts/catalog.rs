//! Anthropic `GET /v1/models` contract and mapping.
//!
//! The Models API returns a `capabilities` object per model (image/PDF input,
//! thinking types, effort levels, structured outputs, ...), so nearly every
//! field of the common record can be provider-sourced.

use gaise_core::contracts::{
    GaiseMetadataSource, GaiseModality, GaiseModel, GaiseModelStatus, GaiseOperation, GaiseSupport,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicModelList {
    #[serde(default)]
    pub data: Vec<AnthropicModelInfo>,
    #[serde(default)]
    pub first_id: Option<String>,
    #[serde(default)]
    pub last_id: Option<String>,
    #[serde(default)]
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicModelInfo {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    /// RFC 3339 release time. May be an epoch value when unknown.
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub max_input_tokens: Option<u64>,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub capabilities: Option<AnthropicModelCapabilities>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicCapabilitySupport {
    #[serde(default)]
    pub supported: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicThinkingTypes {
    #[serde(default)]
    pub adaptive: Option<AnthropicCapabilitySupport>,
    #[serde(default)]
    pub enabled: Option<AnthropicCapabilitySupport>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicThinkingCapability {
    #[serde(default)]
    pub supported: bool,
    #[serde(default)]
    pub types: Option<AnthropicThinkingTypes>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicEffortCapability {
    #[serde(default)]
    pub supported: bool,
    #[serde(default)]
    pub low: Option<AnthropicCapabilitySupport>,
    #[serde(default)]
    pub medium: Option<AnthropicCapabilitySupport>,
    #[serde(default)]
    pub high: Option<AnthropicCapabilitySupport>,
    #[serde(default)]
    pub xhigh: Option<AnthropicCapabilitySupport>,
    #[serde(default)]
    pub max: Option<AnthropicCapabilitySupport>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicModelCapabilities {
    #[serde(default)]
    pub image_input: Option<AnthropicCapabilitySupport>,
    #[serde(default)]
    pub pdf_input: Option<AnthropicCapabilitySupport>,
    #[serde(default)]
    pub structured_outputs: Option<AnthropicCapabilitySupport>,
    #[serde(default)]
    pub thinking: Option<AnthropicThinkingCapability>,
    #[serde(default)]
    pub effort: Option<AnthropicEffortCapability>,
    #[serde(default)]
    pub batch: Option<AnthropicCapabilitySupport>,
    #[serde(default)]
    pub citations: Option<AnthropicCapabilitySupport>,
    #[serde(default)]
    pub code_execution: Option<AnthropicCapabilitySupport>,
    /// Keep any capability keys added after this crate was written.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn support(value: &Option<AnthropicCapabilitySupport>) -> GaiseSupport {
    GaiseSupport::from_option(value.as_ref().map(|v| v.supported))
}

/// Map one Anthropic model record to the common record.
pub fn map_anthropic_model(model: &AnthropicModelInfo, include_raw: bool) -> GaiseModel {
    let mut out = GaiseModel::new("anthropic", model.id.clone());
    out.display_name = model.display_name.clone();
    out.created_at = model.created_at.clone();
    // Anthropic names the context window `max_input_tokens`; zero means the
    // API did not know.
    out.limits.max_input_tokens = model.max_input_tokens.filter(|v| *v > 0);
    out.limits.context_window = out.limits.max_input_tokens;
    out.limits.max_output_tokens = model.max_tokens.filter(|v| *v > 0);
    // Models API only lists callable models; retired ids are absent.
    out.status = GaiseModelStatus::Active;

    let caps = &mut out.capabilities;
    caps.add_source(GaiseMetadataSource::Provider);
    // Every Messages model is a text chat model with streaming and tool use.
    caps.add_input(GaiseModality::Text);
    caps.add_output(GaiseModality::Text);
    caps.add_operation(GaiseOperation::Instruct);
    caps.add_operation(GaiseOperation::InstructStream);
    caps.tools = GaiseSupport::Supported;

    if let Some(c) = &model.capabilities {
        if support(&c.image_input) == GaiseSupport::Supported {
            caps.add_input(GaiseModality::Image);
        }
        if support(&c.pdf_input) == GaiseSupport::Supported {
            caps.add_input(GaiseModality::File);
        }
        caps.structured_output = support(&c.structured_outputs);
        caps.reasoning = GaiseSupport::from_option(c.thinking.as_ref().map(|t| t.supported));
        if let Some(effort) = &c.effort
            && effort.supported
        {
            let mut values = Vec::new();
            for (name, flag) in [
                ("low", &effort.low),
                ("medium", &effort.medium),
                ("high", &effort.high),
                ("xhigh", &effort.xhigh),
                ("max", &effort.max),
            ] {
                if support(flag) == GaiseSupport::Supported {
                    values.push(name.to_string());
                }
            }
            if !values.is_empty() {
                caps.reasoning_values = Some(values);
            }
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

    const FIXTURE: &str = r#"{
      "data": [
        {
          "id": "claude-opus-4-6",
          "capabilities": {
            "batch": {"supported": true},
            "citations": {"supported": true},
            "code_execution": {"supported": true},
            "context_management": {"supported": true, "compact_20260112": {"supported": true}},
            "effort": {
              "supported": true,
              "low": {"supported": true}, "medium": {"supported": true},
              "high": {"supported": true}, "xhigh": {"supported": false}, "max": {"supported": true}
            },
            "image_input": {"supported": true},
            "pdf_input": {"supported": true},
            "structured_outputs": {"supported": true},
            "thinking": {"supported": true, "types": {"adaptive": {"supported": true}, "enabled": {"supported": true}}}
          },
          "created_at": "2026-02-04T00:00:00Z",
          "display_name": "Claude Opus 4.6",
          "max_input_tokens": 200000,
          "max_tokens": 128000,
          "type": "model"
        },
        {
          "id": "claude-haiku-4-5-20251001",
          "capabilities": {
            "effort": {"supported": false},
            "image_input": {"supported": true},
            "pdf_input": {"supported": true},
            "structured_outputs": {"supported": true},
            "thinking": {"supported": true, "types": {"adaptive": {"supported": false}, "enabled": {"supported": true}}}
          },
          "created_at": "2025-10-15T00:00:00Z",
          "display_name": "Claude Haiku 4.5",
          "max_input_tokens": 0,
          "max_tokens": 0,
          "type": "model"
        }
      ],
      "first_id": "claude-opus-4-6",
      "has_more": false,
      "last_id": "claude-haiku-4-5-20251001"
    }"#;

    #[test]
    fn maps_capabilities_limits_and_effort() {
        let list: AnthropicModelList = serde_json::from_str(FIXTURE).unwrap();
        assert!(!list.has_more);
        let opus = map_anthropic_model(&list.data[0], false);
        assert_eq!(opus.display_name.as_deref(), Some("Claude Opus 4.6"));
        assert_eq!(
            opus.capabilities.input,
            vec![
                GaiseModality::Text,
                GaiseModality::Image,
                GaiseModality::File
            ]
        );
        assert_eq!(opus.capabilities.output, vec![GaiseModality::Text]);
        assert_eq!(
            opus.capabilities.operations,
            vec![GaiseOperation::Instruct, GaiseOperation::InstructStream]
        );
        assert_eq!(opus.capabilities.reasoning, GaiseSupport::Supported);
        assert_eq!(opus.capabilities.structured_output, GaiseSupport::Supported);
        assert_eq!(opus.capabilities.tools, GaiseSupport::Supported);
        assert_eq!(
            opus.capabilities.reasoning_values.as_deref(),
            Some(
                &[
                    "low".to_string(),
                    "medium".into(),
                    "high".into(),
                    "max".into()
                ][..]
            )
        );
        assert_eq!(opus.limits.max_input_tokens, Some(200_000));
        assert_eq!(opus.limits.context_window, Some(200_000));
        assert_eq!(opus.limits.max_output_tokens, Some(128_000));
        assert_eq!(opus.status, GaiseModelStatus::Active);
        assert_eq!(
            opus.capabilities.sources,
            vec![GaiseMetadataSource::Provider]
        );

        let haiku = map_anthropic_model(&list.data[1], true);
        assert!(haiku.capabilities.reasoning_values.is_none());
        assert_eq!(haiku.limits.max_input_tokens, None, "zero means unknown");
        assert_eq!(haiku.limits.context_window, None);
        assert_eq!(haiku.raw.unwrap()["display_name"], "Claude Haiku 4.5");
    }

    #[test]
    fn tolerates_missing_capabilities_object() {
        let info: AnthropicModelInfo =
            serde_json::from_str(r#"{"id":"claude-x","type":"model","capabilities":null}"#)
                .unwrap();
        let mapped = map_anthropic_model(&info, false);
        assert_eq!(mapped.capabilities.reasoning, GaiseSupport::Unknown);
        assert_eq!(mapped.capabilities.input, vec![GaiseModality::Text]);
    }
}
