//! Bundled model registry.
//!
//! `model-registry.toml` is compiled into the crate and parsed once on first
//! use. It is advisory: GAISe accepts arbitrary model identifiers, and the
//! registry only exists to fill capability and lifecycle gaps that provider
//! model APIs leave open (see [`crate::contracts::GaiseModel`]).
//!
//! The registry keeps one flat `capabilities` list per entry; this module
//! classifies that closed vocabulary into input modalities, output modalities,
//! GAISe operations, and feature flags. Unknown terms are a hard error so the
//! vocabulary cannot drift silently.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::contracts::{
    EmbeddingProfile, GaiseMetadataSource, GaiseModality, GaiseModel, GaiseModelCapabilities,
    GaiseModelLimits, GaiseModelLimitsEntry, GaiseModelLimitsMatrix, GaiseModelStatus,
    GaiseOperation, GaiseSupport,
};

/// The raw TOML text bundled with the crate.
pub const MODEL_REGISTRY_TOML: &str = include_str!("../model-registry.toml");

#[derive(Debug, Clone, Deserialize)]
struct RawRegistry {
    schema_version: u32,
    audited_on: String,
    #[serde(default)]
    providers: Vec<RegistryProvider>,
    #[serde(default)]
    models: Vec<RegistryModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistryProvider {
    pub key: String,
    #[serde(default)]
    pub gaise_surface: Option<String>,
    #[serde(default)]
    pub catalog: Option<String>,
    #[serde(default)]
    pub lifecycle: Option<String>,
    #[serde(default)]
    pub discovery: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistryModel {
    pub provider: String,
    /// Model identifier. May contain `*` as a wildcard for dynamic families
    /// (`amazon.nova-2-*`, `qwen3:*`).
    pub model: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub status: String,
    #[serde(default)]
    pub shutdown_date: Option<String>,
    #[serde(default)]
    pub retirement_not_before: Option<String>,
    #[serde(default)]
    pub replacement: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Explicit override of the GAISe operations derived from `capabilities`.
    /// Use `operations = []` for models GAISe can describe but not drive
    /// (Responses-only, TTS, Live on a provider without a live adapter).
    /// Values: `instruct`, `instruct_stream`, `embeddings`, `live`.
    #[serde(default)]
    pub operations: Option<Vec<String>>,
    #[serde(default)]
    pub reasoning_values: Option<Vec<String>>,
    /// Vendor-documented context window in tokens (see
    /// [`GaiseModelLimits::context_window`]). Absent for entries whose docs
    /// publish no token window (image generation, TTS) and for embedding
    /// models, whose per-input limit lives in `embedding.max_input_tokens`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// Vendor-documented maximum generated tokens per response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    /// Vendor-documented maximum input characters per request (speech).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_input_characters: Option<u64>,
    /// Embedding model profile (dimension options, limits, task control);
    /// drives `resolve_embedding` in every adapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<EmbeddingProfile>,
    #[serde(default)]
    pub gaise_support: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelRegistry {
    pub schema_version: u32,
    pub audited_on: String,
    pub providers: Vec<RegistryProvider>,
    pub models: Vec<RegistryModel>,
}

/// Result of classifying one entry's flat `capabilities` list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClassifiedCapabilities {
    pub input: Vec<GaiseModality>,
    pub output: Vec<GaiseModality>,
    pub operations: Vec<GaiseOperation>,
    pub tools: GaiseSupport,
    pub reasoning: GaiseSupport,
    pub structured_output: GaiseSupport,
    /// Feature terms that are not modalities or operations (`image_editing`,
    /// `adaptive_reasoning`, ...), preserved for callers that want them.
    pub features: Vec<String>,
}

/// Every term the `capabilities` vocabulary accepts.
pub const CAPABILITY_VOCABULARY: &[&str] = &[
    "text",
    "image_input",
    "image_output",
    "image_editing",
    "audio_input",
    "audio_output",
    "video_input",
    "files",
    "embeddings",
    "multimodal_embeddings",
    "reasoning",
    "adaptive_reasoning",
    "manual_reasoning",
    "streaming",
    "tools",
    "realtime",
    "speech",
];

/// Classify a flat capability list into typed modalities and operations.
pub fn classify_capabilities(terms: &[String]) -> Result<ClassifiedCapabilities, String> {
    let mut out = ClassifiedCapabilities::default();
    let mut text_like = false;
    let mut embeddings = false;
    let mut realtime = false;
    let mut streaming = false;
    let mut speech = false;
    for term in terms {
        match term.as_str() {
            "text" => {
                text_like = true;
                push(&mut out.input, GaiseModality::Text);
                push(&mut out.output, GaiseModality::Text);
            }
            "image_input" => push(&mut out.input, GaiseModality::Image),
            "image_output" => push(&mut out.output, GaiseModality::Image),
            "audio_input" => push(&mut out.input, GaiseModality::Audio),
            "audio_output" => push(&mut out.output, GaiseModality::Audio),
            "video_input" => push(&mut out.input, GaiseModality::Video),
            "files" => push(&mut out.input, GaiseModality::File),
            "embeddings" => {
                embeddings = true;
                push(&mut out.input, GaiseModality::Text);
                push(&mut out.output, GaiseModality::Embedding);
            }
            "multimodal_embeddings" => {
                embeddings = true;
                push(&mut out.input, GaiseModality::Image);
                push(&mut out.input, GaiseModality::Audio);
                push(&mut out.input, GaiseModality::Video);
                push(&mut out.output, GaiseModality::Embedding);
                out.features.push(term.clone());
            }
            "reasoning" => out.reasoning = GaiseSupport::Supported,
            "adaptive_reasoning" | "manual_reasoning" => {
                out.reasoning = GaiseSupport::Supported;
                out.features.push(term.clone());
            }
            "streaming" => streaming = true,
            "tools" => out.tools = GaiseSupport::Supported,
            "realtime" => realtime = true,
            "speech" => {
                speech = true;
                push(&mut out.input, GaiseModality::Text);
                push(&mut out.output, GaiseModality::Audio);
            }
            "image_editing" => out.features.push(term.clone()),
            other => return Err(format!("unknown capability term '{other}'")),
        }
    }

    if realtime {
        push(&mut out.operations, GaiseOperation::Live);
    } else if text_like && out.output.contains(&GaiseModality::Text) {
        // Text-producing chat models are driven through instruct; image-only
        // generators (no `text`) are outside the instruct surface.
        push(&mut out.operations, GaiseOperation::Instruct);
        if streaming {
            push(&mut out.operations, GaiseOperation::InstructStream);
        }
    }
    if embeddings {
        push(&mut out.operations, GaiseOperation::Embeddings);
    }
    if speech {
        push(&mut out.operations, GaiseOperation::Speech);
    }
    if !terms.is_empty() {
        // Terms were enumerated; anything not listed is explicitly absent.
        if out.tools.is_unknown() {
            out.tools = GaiseSupport::Unsupported;
        }
        if out.reasoning.is_unknown() {
            out.reasoning = GaiseSupport::Unsupported;
        }
    }
    Ok(out)
}

fn push<T: PartialEq + Ord>(items: &mut Vec<T>, item: T) {
    if !items.contains(&item) {
        items.push(item);
        items.sort();
    }
}

/// Map a registry `status` string onto the common lifecycle enum.
pub fn map_status(status: &str) -> GaiseModelStatus {
    match status {
        "active"
        | "stable"
        | "short_term_active"
        | "limited_availability"
        | "dynamic_active_family"
        | "dynamic_local" => GaiseModelStatus::Active,
        "preview" | "preview_legacy" => GaiseModelStatus::Preview,
        "deprecated" => GaiseModelStatus::Deprecated,
        "legacy" => GaiseModelStatus::Legacy,
        "retired" => GaiseModelStatus::Retired,
        _ => GaiseModelStatus::Unknown,
    }
}

impl RegistryModel {
    pub fn classified(&self) -> Result<ClassifiedCapabilities, String> {
        let mut classified = classify_capabilities(&self.capabilities)
            .map_err(|e| format!("{}::{}: {e}", self.provider, self.model))?;
        if let Some(overrides) = &self.operations {
            let mut operations = Vec::new();
            for value in overrides {
                let op = GaiseOperation::parse(value).ok_or_else(|| {
                    format!(
                        "{}::{}: unknown operation '{value}'",
                        self.provider, self.model
                    )
                })?;
                push(&mut operations, op);
            }
            classified.operations = operations;
        }
        Ok(classified)
    }

    pub fn status(&self) -> GaiseModelStatus {
        map_status(&self.status)
    }

    /// Every limit this entry documents, in the common shape.
    pub fn limits(&self) -> GaiseModelLimits {
        GaiseModelLimits {
            context_window: self.context_window,
            max_input_tokens: self
                .embedding
                .as_ref()
                .and_then(|p| p.max_input_tokens)
                .map(u64::from),
            max_output_tokens: self.max_output_tokens,
            max_input_characters: self.max_input_characters,
            embedding_dimensions: self.embedding.as_ref().and_then(|p| p.default_dimensions),
        }
    }

    /// This entry as one row of the limits matrix.
    pub fn limits_entry(&self) -> GaiseModelLimitsEntry {
        GaiseModelLimitsEntry {
            id: format!("{}::{}", self.provider, self.model),
            provider: self.provider.clone(),
            aliases: self.aliases.clone(),
            status: self.status(),
            operations: self.classified().map(|c| c.operations).unwrap_or_default(),
            limits: self.limits(),
            notes: self.notes.clone(),
        }
    }

    /// Does this entry describe `model_id`?
    ///
    /// Matches, in order: exact id or alias; `*` glob on id or alias; a
    /// provider snapshot suffix appended to the id or alias (`-YYYY-MM-DD`,
    /// `-YYYYMMDD`, Bedrock `-v1:0` / `-YYYYMMDD-v1:0`). Bedrock cross-region
    /// inference-profile prefixes (`us.`, `global.`, ...) are stripped first.
    pub fn matches(&self, model_id: &str) -> bool {
        let candidate = if self.provider == "bedrock" {
            strip_inference_profile_prefix(model_id)
        } else {
            model_id
        };
        // Ollama resolves an untagged name to `name:latest`; match the
        // `name:*` family patterns the same way.
        let tagged = (self.provider == "ollama" && !candidate.contains(':'))
            .then(|| format!("{candidate}:latest"));
        std::iter::once(self.model.as_str())
            .chain(self.aliases.iter().map(String::as_str))
            .any(|pattern| {
                pattern == candidate
                    || glob_match(pattern, candidate)
                    || snapshot_of(pattern, candidate)
                    || tagged.as_deref().is_some_and(|t| glob_match(pattern, t))
            })
    }

    /// Build a model record purely from the registry entry.
    pub fn to_gaise_model(&self) -> GaiseModel {
        let mut model = GaiseModel::new(self.provider.clone(), self.model.clone());
        model.status = self.status();
        model.retires_on = self.shutdown_date.clone();
        model.retirement_not_before = self.retirement_not_before.clone();
        model.replacement = self.replacement.clone();
        model.notes = join_notes(self.gaise_support.as_deref(), self.notes.as_deref());
        model.limits = self.limits();
        if let Ok(classified) = self.classified() {
            model.capabilities = GaiseModelCapabilities {
                input: classified.input,
                output: classified.output,
                operations: classified.operations,
                tools: classified.tools,
                reasoning: classified.reasoning,
                reasoning_values: self.reasoning_values.clone(),
                structured_output: classified.structured_output,
                sources: vec![GaiseMetadataSource::Registry],
            };
        }
        model
    }

    /// Fill the unknown parts of `model` from this entry. Returns `true` if
    /// anything was applied.
    ///
    /// Rules:
    /// - Modalities are a *union*: provider modality vocabularies are often
    ///   incomplete (Bedrock cannot express documents, Gemini reports none),
    ///   so the registry may add modalities but never removes one.
    /// - Operations and tri-state flags are filled only when unknown: a
    ///   provider that says "no streaming" or "no tools" is believed.
    /// - Limits are filled field by field only when unknown: a provider that
    ///   reports its own context window is believed over the registry.
    /// - Lifecycle fields are filled only when absent.
    pub fn overlay(&self, model: &mut GaiseModel) -> bool {
        let mut applied = false;
        let caps = &mut model.capabilities;
        if let Ok(classified) = self.classified() {
            for modality in classified.input {
                if !caps.input.contains(&modality) {
                    caps.add_input(modality);
                    applied = true;
                }
            }
            for modality in classified.output {
                if !caps.output.contains(&modality) {
                    caps.add_output(modality);
                    applied = true;
                }
            }
            let heuristic_operations = caps.sources.contains(&GaiseMetadataSource::Heuristic);
            if self.operations.is_some() && heuristic_operations {
                // An explicit registry override beats a name heuristic.
                if caps.operations != classified.operations {
                    caps.operations = classified.operations;
                    applied = true;
                }
            } else if caps.operations.is_empty() && !classified.operations.is_empty() {
                caps.operations = classified.operations;
                applied = true;
            }
            applied |= caps.tools.fill(classified.tools);
            applied |= caps.reasoning.fill(classified.reasoning);
            applied |= caps.structured_output.fill(classified.structured_output);
        }
        if caps.reasoning_values.is_none() && self.reasoning_values.is_some() {
            caps.reasoning_values = self.reasoning_values.clone();
            applied = true;
        }
        applied |= model.limits.fill(&self.limits());
        if model.status == GaiseModelStatus::Unknown {
            let status = self.status();
            if status != GaiseModelStatus::Unknown {
                model.status = status;
                applied = true;
            }
        }
        if model.retires_on.is_none() && self.shutdown_date.is_some() {
            model.retires_on = self.shutdown_date.clone();
            applied = true;
        }
        if model.retirement_not_before.is_none() && self.retirement_not_before.is_some() {
            model.retirement_not_before = self.retirement_not_before.clone();
            applied = true;
        }
        if model.replacement.is_none() && self.replacement.is_some() {
            model.replacement = self.replacement.clone();
            applied = true;
        }
        if model.notes.is_none() {
            model.notes = join_notes(self.gaise_support.as_deref(), self.notes.as_deref());
            applied |= model.notes.is_some();
        }
        if applied {
            model.capabilities.add_source(GaiseMetadataSource::Registry);
        }
        applied
    }
}

fn join_notes(support: Option<&str>, notes: Option<&str>) -> Option<String> {
    match (support, notes) {
        (Some(s), Some(n)) => Some(format!("{s}. {n}")),
        (Some(s), None) => Some(s.to_string()),
        (None, Some(n)) => Some(n.to_string()),
        (None, None) => None,
    }
}

const INFERENCE_PROFILE_PREFIXES: &[&str] = &[
    "global.", "us.", "eu.", "apac.", "ap.", "jp.", "au.", "ca.", "il.", "us-gov.",
];

/// Strip a Bedrock cross-region inference-profile prefix from a model id.
pub fn strip_inference_profile_prefix(model_id: &str) -> &str {
    for prefix in INFERENCE_PROFILE_PREFIXES {
        if let Some(rest) = model_id.strip_prefix(prefix) {
            return rest;
        }
    }
    model_id
}

/// Minimal `*` glob matcher (no character classes).
pub fn glob_match(pattern: &str, value: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == value;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut rest = value;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(pos) => rest = &rest[pos + part.len()..],
                None => return false,
            }
        }
    }
    true
}

/// Is `value` equal to `base` plus a dated-snapshot or Bedrock version suffix?
fn snapshot_of(base: &str, value: &str) -> bool {
    let Some(suffix) = value.strip_prefix(base) else {
        return false;
    };
    if let Some(minor) = suffix.strip_prefix(':') {
        // `anthropic.claude-opus-4-6-v1` -> `anthropic.claude-opus-4-6-v1:0`
        return !minor.is_empty()
            && minor
                .chars()
                .all(|c| c.is_ascii_digit() || c == ':' || c == 'k');
    }
    let Some(suffix) = suffix.strip_prefix('-') else {
        return false;
    };
    let suffix = match strip_date(suffix) {
        Some(rest) => rest,
        None => suffix,
    };
    if suffix.is_empty() {
        return true;
    }
    // Bedrock: `-v1:0`, `-v2:0:200k`
    let Some(version) = suffix
        .strip_prefix("-v")
        .or_else(|| suffix.strip_prefix('v'))
    else {
        return false;
    };
    !version.is_empty()
        && version
            .chars()
            .all(|c| c.is_ascii_digit() || c == ':' || c == 'k')
}

/// Strip a leading `YYYY-MM-DD` or `YYYYMMDD` date; returns the remainder.
fn strip_date(value: &str) -> Option<&str> {
    let bytes = value.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes.len() >= range.end && bytes[range].iter().all(u8::is_ascii_digit)
    };
    if digits(0..4)
        && bytes.get(4) == Some(&b'-')
        && digits(5..7)
        && bytes.get(7) == Some(&b'-')
        && digits(8..10)
    {
        return Some(&value[10..]);
    }
    if digits(0..8) {
        return Some(&value[8..]);
    }
    None
}

impl ModelRegistry {
    /// Parse a registry document.
    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: RawRegistry = toml::from_str(text).map_err(|e| e.to_string())?;
        let registry = Self {
            schema_version: raw.schema_version,
            audited_on: raw.audited_on,
            providers: raw.providers,
            models: raw.models,
        };
        for entry in &registry.models {
            entry.classified()?;
        }
        Ok(registry)
    }

    /// The bundled registry, parsed once.
    pub fn bundled() -> &'static ModelRegistry {
        static REGISTRY: OnceLock<ModelRegistry> = OnceLock::new();
        REGISTRY.get_or_init(|| {
            ModelRegistry::parse(MODEL_REGISTRY_TOML)
                .expect("bundled model-registry.toml must parse and use the known vocabulary")
        })
    }

    pub fn provider(&self, key: &str) -> Option<&RegistryProvider> {
        self.providers.iter().find(|p| p.key == key)
    }

    pub fn models_for(&self, provider: &str) -> impl Iterator<Item = &RegistryModel> {
        self.models.iter().filter(move |m| m.provider == provider)
    }

    /// Find the best entry for a provider model id. Exact/alias matches win
    /// over glob and snapshot matches.
    pub fn find(&self, provider: &str, model_id: &str) -> Option<&RegistryModel> {
        let candidates: Vec<&RegistryModel> = self
            .models_for(provider)
            .filter(|m| m.matches(model_id))
            .collect();
        let bedrock_id = if provider == "bedrock" {
            strip_inference_profile_prefix(model_id)
        } else {
            model_id
        };
        candidates
            .iter()
            .copied()
            .find(|m| m.model == bedrock_id || m.aliases.iter().any(|a| a == bedrock_id))
            .or_else(|| {
                // Prefer the longest (most specific) pattern.
                candidates
                    .into_iter()
                    .max_by_key(|m| m.model.replace('*', "").len())
            })
    }

    /// Embedding profile for a provider model, if the registry knows it.
    /// Adapters call this (through the bundled registry) to resolve
    /// dimensions, task control, and normalization; `None` means pass-through.
    pub fn embedding_profile(&self, provider: &str, model_id: &str) -> Option<&EmbeddingProfile> {
        self.find(provider, model_id)?.embedding.as_ref()
    }

    /// The model × limits matrix: one row per registry entry (optionally one
    /// provider), in registry order. Needs no credentials and contacts no
    /// provider; `GET /v1/models` remains the place to see what a provider
    /// reports live.
    pub fn limits_matrix(&self, provider: Option<&str>) -> GaiseModelLimitsMatrix {
        GaiseModelLimitsMatrix {
            audited_on: self.audited_on.clone(),
            source: GaiseMetadataSource::Registry,
            models: self
                .models
                .iter()
                .filter(|m| provider.is_none_or(|p| m.provider == p))
                .map(RegistryModel::limits_entry)
                .collect(),
        }
    }

    /// Overlay registry knowledge onto a provider-sourced model.
    pub fn enrich(&self, model: &mut GaiseModel) -> bool {
        match self.find(&model.provider, &model.id) {
            Some(entry) => entry.overlay(model),
            None => false,
        }
    }
}

/// Convenience: enrich with the bundled registry.
pub fn enrich(model: &mut GaiseModel) -> bool {
    ModelRegistry::bundled().enrich(model)
}

/// Convenience: the limits matrix of the bundled registry.
pub fn limits_matrix(provider: Option<&str>) -> GaiseModelLimitsMatrix {
    ModelRegistry::bundled().limits_matrix(provider)
}

/// Convenience: embedding profile from the bundled registry.
pub fn embedding_profile(provider: &str, model_id: &str) -> Option<&'static EmbeddingProfile> {
    ModelRegistry::bundled().embedding_profile(provider, model_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_registry_parses_and_uses_closed_vocabulary() {
        let registry = ModelRegistry::bundled();
        assert!(registry.schema_version >= 2);
        assert!(!registry.models.is_empty());
        for model in &registry.models {
            for term in &model.capabilities {
                assert!(
                    CAPABILITY_VOCABULARY.contains(&term.as_str()),
                    "{}::{} uses unknown term {term}",
                    model.provider,
                    model.model
                );
            }
            assert!(
                !model.status.is_empty(),
                "{}::{} has no status",
                model.provider,
                model.model
            );
            let known_status = map_status(&model.status) != GaiseModelStatus::Unknown;
            assert!(
                known_status,
                "{}::{} status '{}' is unmapped",
                model.provider, model.model, model.status
            );
        }
        for provider in [
            "openai",
            "anthropic",
            "gemini",
            "vertexai",
            "bedrock",
            "ollama",
            "elevenlabs",
        ] {
            assert!(
                registry.provider(provider).is_some(),
                "missing provider {provider}"
            );
        }
    }

    #[test]
    fn classification_splits_modalities_and_operations() {
        let terms: Vec<String> = [
            "realtime",
            "text",
            "image_input",
            "audio_input",
            "audio_output",
            "reasoning",
            "tools",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let c = classify_capabilities(&terms).unwrap();
        assert_eq!(
            c.input,
            vec![
                GaiseModality::Text,
                GaiseModality::Image,
                GaiseModality::Audio
            ]
        );
        assert_eq!(c.output, vec![GaiseModality::Text, GaiseModality::Audio]);
        assert_eq!(c.operations, vec![GaiseOperation::Live]);
        assert_eq!(c.tools, GaiseSupport::Supported);
        assert_eq!(c.reasoning, GaiseSupport::Supported);

        let chat: Vec<String> = [
            "text",
            "image_input",
            "files",
            "adaptive_reasoning",
            "streaming",
            "tools",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let c = classify_capabilities(&chat).unwrap();
        assert_eq!(
            c.operations,
            vec![GaiseOperation::Instruct, GaiseOperation::InstructStream]
        );
        assert_eq!(
            c.input,
            vec![
                GaiseModality::Text,
                GaiseModality::Image,
                GaiseModality::File
            ]
        );
        assert_eq!(c.features, vec!["adaptive_reasoning"]);

        let embed: Vec<String> = vec!["embeddings".into()];
        let c = classify_capabilities(&embed).unwrap();
        assert_eq!(c.operations, vec![GaiseOperation::Embeddings]);
        assert_eq!(c.output, vec![GaiseModality::Embedding]);
        assert_eq!(c.tools, GaiseSupport::Unsupported);

        let image_only: Vec<String> = vec![
            "image_input".into(),
            "image_output".into(),
            "image_editing".into(),
        ];
        let c = classify_capabilities(&image_only).unwrap();
        assert!(c.operations.is_empty());

        assert!(classify_capabilities(&["bogus".to_string()]).is_err());
        assert_eq!(
            classify_capabilities(&[]).unwrap().tools,
            GaiseSupport::Unknown
        );
    }

    #[test]
    fn lookup_matches_exact_alias_glob_and_snapshots() {
        let registry = ModelRegistry::parse(
            r#"
schema_version = 2
audited_on = "2026-01-01"
[[providers]]
key = "openai"
[[models]]
provider = "openai"
model = "gpt-5.6"
aliases = ["gpt-5.6-sol"]
status = "active"
capabilities = ["text", "streaming", "tools"]
[[models]]
provider = "openai"
model = "gpt-5.6-terra"
status = "active"
capabilities = ["text", "streaming"]
[[models]]
provider = "bedrock"
model = "anthropic.claude-fable-5"
status = "active"
capabilities = ["text", "streaming", "tools"]
[[models]]
provider = "bedrock"
model = "amazon.nova-2-*"
status = "dynamic_active_family"
capabilities = ["text", "streaming"]
[[models]]
provider = "bedrock"
model = "amazon.nova-2-lite-v1:0"
status = "active"
capabilities = ["text", "streaming", "tools"]
[[models]]
provider = "ollama"
model = "qwen3:*"
status = "dynamic_local"
capabilities = ["text", "reasoning", "streaming", "tools"]
"#,
        )
        .unwrap();

        assert_eq!(registry.find("openai", "gpt-5.6").unwrap().model, "gpt-5.6");
        assert_eq!(
            registry.find("openai", "gpt-5.6-sol").unwrap().model,
            "gpt-5.6"
        );
        assert_eq!(
            registry.find("openai", "gpt-5.6-2026-03-05").unwrap().model,
            "gpt-5.6"
        );
        assert_eq!(
            registry.find("openai", "gpt-5.6-terra").unwrap().model,
            "gpt-5.6-terra"
        );
        assert!(registry.find("openai", "gpt-5.6-pro").is_none());
        assert!(registry.find("openai", "gpt-5").is_none());

        assert_eq!(
            registry
                .find("bedrock", "anthropic.claude-fable-5-v1:0")
                .unwrap()
                .model,
            "anthropic.claude-fable-5"
        );
        assert_eq!(
            registry
                .find("bedrock", "us.anthropic.claude-fable-5-20260601-v1:0")
                .unwrap()
                .model,
            "anthropic.claude-fable-5"
        );
        assert_eq!(
            registry
                .find("bedrock", "amazon.nova-2-pro-v1:0")
                .unwrap()
                .model,
            "amazon.nova-2-*"
        );
        // Exact entry beats the family glob.
        assert_eq!(
            registry
                .find("bedrock", "global.amazon.nova-2-lite-v1:0")
                .unwrap()
                .model,
            "amazon.nova-2-lite-v1:0"
        );
        assert_eq!(
            registry.find("ollama", "qwen3:8b").unwrap().model,
            "qwen3:*"
        );
        assert!(registry.find("ollama", "llama3:8b").is_none());
    }

    #[test]
    fn overlay_fills_unknowns_without_overriding_provider_facts() {
        let registry = ModelRegistry::bundled();
        let mut model = GaiseModel::new("anthropic", "claude-opus-4-6");
        model.capabilities.add_input(GaiseModality::Text);
        model.capabilities.add_input(GaiseModality::Image);
        model.capabilities.tools = GaiseSupport::Unsupported; // provider said no: keep it
        model.capabilities.add_source(GaiseMetadataSource::Provider);

        assert!(registry.enrich(&mut model));
        assert_eq!(
            model.capabilities.input,
            vec![
                GaiseModality::Text,
                GaiseModality::Image,
                GaiseModality::File
            ],
            "registry may add modalities (documents) the provider could not express"
        );
        assert_eq!(model.capabilities.tools, GaiseSupport::Unsupported);
        assert_eq!(model.capabilities.reasoning, GaiseSupport::Supported);
        assert!(
            model
                .capabilities
                .operations
                .contains(&GaiseOperation::InstructStream)
        );
        assert_eq!(model.status, GaiseModelStatus::Active);
        assert!(model.retirement_not_before.is_some());
        assert_eq!(
            model.capabilities.sources,
            vec![GaiseMetadataSource::Provider, GaiseMetadataSource::Registry]
        );

        let mut unknown = GaiseModel::new("openai", "totally-new-model");
        assert!(!registry.enrich(&mut unknown));
        assert!(unknown.capabilities.sources.is_empty());
    }

    #[test]
    fn bundled_lookups_resolve_expected_entries() {
        let registry = ModelRegistry::bundled();
        let find = |provider: &str, id: &str| {
            registry
                .find(provider, id)
                .map(|m| m.model.as_str())
                .unwrap_or_else(|| panic!("no registry entry for {provider}::{id}"))
        };
        assert_eq!(find("anthropic", "claude-opus-5"), "claude-opus-5");
        assert_eq!(
            find("anthropic", "claude-sonnet-4-5"),
            "claude-sonnet-4-5-20250929"
        );
        assert_eq!(find("openai", "gpt-5.6-sol"), "gpt-5.6");
        assert_eq!(find("openai", "gpt-5.4-2026-03-05"), "gpt-5.4");
        assert_eq!(
            find("gemini", "gemini-2.5-flash-lite"),
            "gemini-2.5-flash-lite"
        );
        assert_eq!(
            find("vertexai", "gemini-embedding-2-preview"),
            "gemini-embedding-2"
        );
        assert_eq!(
            find("bedrock", "us.anthropic.claude-opus-5-v1:0"),
            "anthropic.claude-opus-5"
        );
        assert_eq!(
            find("bedrock", "global.anthropic.claude-opus-4-6-v1:0"),
            "anthropic.claude-opus-4-6-v1"
        );
        assert_eq!(
            find("bedrock", "global.amazon.nova-2-lite-v1:0"),
            "amazon.nova-2-lite-v1:0"
        );
        assert_eq!(
            find("bedrock", "us.amazon.nova-pro-v1:0"),
            "amazon.nova-pro-v1:0"
        );
        assert_eq!(find("bedrock", "amazon.nova-ultra-v1:0"), "amazon.nova-*");
        assert_eq!(
            find("bedrock", "amazon.nova-reel-v1:1"),
            "amazon.nova-reel-v1:*"
        );
        assert_eq!(
            find("bedrock", "amazon.nova-canvas-v1:0"),
            "amazon.nova-canvas-v1:0"
        );
        assert_eq!(find("bedrock", "cohere.embed-v4:0"), "cohere.embed-v4:0");
        assert_eq!(
            find("bedrock", "cohere.embed-multilingual-v3"),
            "cohere.embed-english-v3"
        );
        assert_eq!(find("bedrock", "cohere.embed-light-v3"), "cohere.embed-*");
        assert_eq!(
            find("bedrock", "us.amazon.titan-embed-text-v2:0"),
            "amazon.titan-embed-text-v2:0"
        );
        assert_eq!(find("ollama", "gpt-oss:20b"), "gpt-oss:*");
        assert_eq!(find("ollama", "nomic-embed-text"), "nomic-embed-text:*");
        assert_eq!(
            find("ollama", "snowflake-arctic-embed2:latest"),
            "snowflake-arctic-embed2:*"
        );
        assert_eq!(
            find("ollama", "snowflake-arctic-embed:335m"),
            "snowflake-arctic-embed:*"
        );
        assert_eq!(
            find("elevenlabs", "eleven_english_sts_v2"),
            "eleven_multilingual_sts_v2"
        );
        let v3 = registry
            .find("elevenlabs", "eleven_v3")
            .unwrap()
            .classified()
            .unwrap();
        assert_eq!(
            v3.operations,
            vec![GaiseOperation::Speech, GaiseOperation::Live]
        );
        assert_eq!(v3.input, vec![GaiseModality::Text]);
        assert_eq!(v3.output, vec![GaiseModality::Audio]);
        let conversational = registry
            .find("elevenlabs", "eleven_v3_conversational")
            .unwrap()
            .classified()
            .unwrap();
        assert_eq!(conversational.operations, vec![GaiseOperation::Live]);
        assert!(registry.find("openai", "gpt-5.6-cyber").is_none());

        // Explicit operation overrides: describable but not drivable.
        let pro = registry.find("openai", "gpt-5.5-pro").unwrap();
        assert!(pro.classified().unwrap().operations.is_empty());
        let tts = registry
            .find("gemini", "gemini-3.1-flash-tts-preview")
            .unwrap();
        assert!(tts.classified().unwrap().operations.is_empty());
        let vertex_live = registry
            .find("vertexai", "gemini-live-2.5-flash-native-audio")
            .unwrap();
        assert!(vertex_live.classified().unwrap().operations.is_empty());

        // A heuristic claim is replaced by an explicit override.
        let mut model = GaiseModel::new("openai", "gpt-5.5-pro");
        model.capabilities.add_operation(GaiseOperation::Instruct);
        model.capabilities.add_source(GaiseMetadataSource::Provider);
        model
            .capabilities
            .add_source(GaiseMetadataSource::Heuristic);
        registry.enrich(&mut model);
        assert!(model.capabilities.operations.is_empty());
    }

    #[test]
    fn glob_and_snapshot_helpers() {
        assert!(glob_match("qwen3:*", "qwen3:8b"));
        assert!(glob_match(
            "amazon.titan-embed-*",
            "amazon.titan-embed-text-v2:0"
        ));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b", "ac"));
        assert!(snapshot_of("gpt-5.4", "gpt-5.4-2026-03-05"));
        assert!(snapshot_of("claude-opus-4-6", "claude-opus-4-6-20260205"));
        assert!(snapshot_of(
            "anthropic.claude-sonnet-4-5",
            "anthropic.claude-sonnet-4-5-20250929-v1:0"
        ));
        assert!(snapshot_of(
            "amazon.titan-embed-text",
            "amazon.titan-embed-text-v2:0"
        ));
        assert!(snapshot_of(
            "anthropic.claude-opus-4-6-v1",
            "anthropic.claude-opus-4-6-v1:0"
        ));
        assert!(!snapshot_of("gpt-5.4", "gpt-5.4-mini"));
        assert_eq!(
            strip_inference_profile_prefix("us.anthropic.x"),
            "anthropic.x"
        );
        assert_eq!(strip_inference_profile_prefix("anthropic.x"), "anthropic.x");
    }

    #[test]
    fn embedding_profiles_resolve_through_the_registry() {
        use crate::contracts::{DimensionRule, EmbeddingTaskControl};
        let registry = ModelRegistry::bundled();
        let large = registry
            .embedding_profile("openai", "text-embedding-3-large")
            .expect("profile");
        assert_eq!(large.default_dimensions, Some(3072));
        assert_eq!(
            large.dimension_rule,
            DimensionRule::Range { min: 1, max: 3072 }
        );
        assert!(large.normalizes_truncation);

        let g2 = registry
            .embedding_profile("gemini", "gemini-embedding-2-preview")
            .expect("alias resolves");
        assert_eq!(g2.task_control, EmbeddingTaskControl::PromptInstruction);

        let v001 = registry
            .embedding_profile("vertexai", "gemini-embedding-001")
            .expect("profile");
        assert!(v001.single_input);
        assert!(!v001.normalizes_truncation);

        let nomic = registry
            .embedding_profile("ollama", "nomic-embed-text:v1.5")
            .expect("glob resolves");
        assert!(matches!(
            &nomic.task_control,
            EmbeddingTaskControl::Prefix { query: Some(q), .. } if q == "search_query: "
        ));

        let qwen = registry
            .embedding_profile("ollama", "qwen3-embedding:4b")
            .expect("profile");
        assert!(matches!(
            &qwen.task_control,
            EmbeddingTaskControl::QueryInstruction(i) if i.ends_with("\nQuery:")
        ));

        let titan = registry
            .embedding_profile("bedrock", "eu.amazon.titan-embed-text-v2:0")
            .expect("profile prefix stripped");
        assert_eq!(
            titan.dimension_rule,
            DimensionRule::Set(vec![256, 512, 1024])
        );

        let cohere_other = registry
            .embedding_profile("bedrock", "cohere.embed-light-v3")
            .expect("catch-all");
        assert_eq!(cohere_other.task_control, EmbeddingTaskControl::InputType);
        assert_eq!(cohere_other.dimension_rule, DimensionRule::Fixed);

        assert!(registry.embedding_profile("openai", "gpt-5.6").is_none());

        // list_models limits come from the profile.
        let model = registry
            .find("openai", "text-embedding-3-small")
            .unwrap()
            .to_gaise_model();
        assert_eq!(model.limits.embedding_dimensions, Some(1536));
        assert_eq!(model.limits.max_input_tokens, Some(8192));
    }

    #[test]
    fn limits_overlay_fills_unknowns_and_keeps_provider_values() {
        let registry = ModelRegistry::parse(
            r#"
schema_version = 2
audited_on = "2026-08-21"

[[models]]
provider = "openai"
model = "gpt-5.6"
status = "active"
capabilities = ["text", "streaming", "tools"]
context_window = 1050000
max_output_tokens = 128000
"#,
        )
        .unwrap();

        // Registry-only record carries both figures.
        let entry = registry.find("openai", "gpt-5.6-2026-03-05").unwrap();
        let model = entry.to_gaise_model();
        assert_eq!(model.limits.context_window, Some(1_050_000));
        assert_eq!(model.limits.max_output_tokens, Some(128_000));
        assert_eq!(model.limits.max_input_tokens, None);

        // Provider-reported window wins; the missing output cap is filled.
        let mut reported = GaiseModel::new("openai", "gpt-5.6");
        reported.limits.context_window = Some(400_000);
        reported
            .capabilities
            .add_source(GaiseMetadataSource::Provider);
        assert!(registry.enrich(&mut reported));
        assert_eq!(reported.limits.context_window, Some(400_000));
        assert_eq!(reported.limits.max_output_tokens, Some(128_000));
        assert!(
            reported
                .capabilities
                .sources
                .contains(&GaiseMetadataSource::Registry)
        );

        // A fully reported model is left alone.
        let mut complete = GaiseModel::new("openai", "gpt-5.6");
        complete.limits.context_window = Some(400_000);
        complete.limits.max_output_tokens = Some(64_000);
        complete.capabilities.add_input(GaiseModality::Text);
        complete.capabilities.add_output(GaiseModality::Text);
        complete
            .capabilities
            .add_operation(GaiseOperation::Instruct);
        complete
            .capabilities
            .add_operation(GaiseOperation::InstructStream);
        complete.capabilities.tools = GaiseSupport::Supported;
        complete.capabilities.reasoning = GaiseSupport::Unsupported;
        complete.capabilities.structured_output = GaiseSupport::Supported;
        complete.status = GaiseModelStatus::Active;
        complete.notes = Some("provider".into());
        assert!(!registry.enrich(&mut complete));
        assert_eq!(complete.limits.max_output_tokens, Some(64_000));
    }

    #[test]
    fn limits_matrix_lists_every_entry_with_routable_ids() {
        let registry = ModelRegistry::bundled();
        let matrix = registry.limits_matrix(None);
        assert_eq!(matrix.audited_on, registry.audited_on);
        assert_eq!(matrix.source, GaiseMetadataSource::Registry);
        assert_eq!(matrix.models.len(), registry.models.len());

        let row = |id: &str| {
            matrix
                .models
                .iter()
                .find(|m| m.id == id)
                .unwrap_or_else(|| panic!("no row for {id}"))
        };
        let opus = row("anthropic::claude-opus-5");
        assert_eq!(opus.provider, "anthropic");
        assert_eq!(opus.limits.context_window, Some(1_000_000));
        assert_eq!(opus.limits.max_output_tokens, Some(128_000));
        assert!(opus.operations.contains(&GaiseOperation::Instruct));

        let embed = row("openai::text-embedding-3-small");
        assert_eq!(embed.limits.max_input_tokens, Some(8192));
        assert_eq!(embed.limits.embedding_dimensions, Some(1536));
        assert_eq!(embed.limits.context_window, None);

        let ollama = registry.limits_matrix(Some("ollama"));
        assert!(!ollama.models.is_empty());
        assert!(ollama.models.iter().all(|m| m.provider == "ollama"));
        assert!(ollama.models.iter().any(|m| m.id == "ollama::qwen3:*"));

        assert!(registry.limits_matrix(Some("nope")).models.is_empty());
    }

    #[test]
    fn every_driveable_text_model_documents_a_context_window() {
        // Every entry GAISe can drive through instruct or live must record the
        // vendor's context window (or, for speech models, the per-request
        // character limit), so `GET /v1/models/limits` has no silent gaps.
        // Embedding entries carry their per-input limit in the profile;
        // image-generation and STT entries have no token window.
        // Entries whose vendor publishes no per-request figure at all; the
        // provider's model API reports one live where it exists.
        const UNDOCUMENTED: &[&str] = &["elevenlabs::eleven_v3_conversational"];
        let registry = ModelRegistry::bundled();
        let mut missing = Vec::new();
        for entry in &registry.models {
            let id = format!("{}::{}", entry.provider, entry.model);
            if UNDOCUMENTED.contains(&id.as_str()) {
                continue;
            }
            let ops = entry.classified().unwrap().operations;
            let text_model =
                ops.contains(&GaiseOperation::Instruct) || ops.contains(&GaiseOperation::Live);
            let documented = entry.context_window.is_some() || entry.max_input_characters.is_some();
            // Retired models whose vendor pages are gone keep whatever was
            // last documented; nothing is invented for them.
            if text_model && !documented && entry.status() != GaiseModelStatus::Retired {
                missing.push(id);
            }
            if entry.embedding.is_some() && entry.context_window.is_some() {
                panic!(
                    "{}::{}: embedding entries record max_input_tokens in the profile, not context_window",
                    entry.provider, entry.model
                );
            }
        }
        assert!(
            missing.is_empty(),
            "registry entries without context_window: {missing:?}"
        );
    }
}
