//! Provider-neutral model catalog contracts.
//!
//! Every provider exposes some way of listing models, but the amount of
//! capability metadata they return varies enormously: Anthropic reports input
//! modalities, thinking and effort support; Bedrock reports input/output
//! modalities and streaming; Ollama reports capability tags per installed tag;
//! Gemini reports supported generation methods and token limits; OpenAI and
//! Vertex AI report little more than the identifier.
//!
//! The contract therefore distinguishes *unknown* from *unsupported*
//! ([`GaiseSupport::Unknown`], empty modality lists) and records where every
//! answer came from ([`GaiseMetadataSource`]). Adapters return only what the
//! provider actually said; the registry overlay in `gaise_core::registry`
//! fills the remaining gaps and tags them as such.

use serde::{Deserialize, Serialize};

/// A content modality a model can consume or produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GaiseModality {
    Text,
    Image,
    Audio,
    Video,
    File,
    Embedding,
}

/// A GAISe operation — one of the trait surfaces a model can be driven through.
///
/// This is deliberately about what GAISe can do with the model, not what the
/// vendor advertises. A model that emits audio through an API GAISe does not
/// map will still list `audio` as an output modality, but not `live`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GaiseOperation {
    Instruct,
    InstructStream,
    Embeddings,
    /// Text-to-speech (`speech` / `speech_stream`).
    Speech,
    Live,
}

impl GaiseOperation {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "instruct" => Some(Self::Instruct),
            "instruct_stream" | "stream" | "streaming" => Some(Self::InstructStream),
            "embeddings" | "embedding" | "embed" => Some(Self::Embeddings),
            "speech" | "tts" | "text_to_speech" => Some(Self::Speech),
            "live" | "realtime" => Some(Self::Live),
            _ => None,
        }
    }
}

/// Tri-state support flag. `Unknown` means no source made a claim either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GaiseSupport {
    Supported,
    Unsupported,
    #[default]
    Unknown,
}

impl GaiseSupport {
    pub fn from_bool(value: bool) -> Self {
        if value {
            Self::Supported
        } else {
            Self::Unsupported
        }
    }

    pub fn from_option(value: Option<bool>) -> Self {
        value.map(Self::from_bool).unwrap_or_default()
    }

    pub fn is_unknown(self) -> bool {
        self == Self::Unknown
    }

    /// Fill this value from `other` only when it is still unknown.
    pub fn fill(&mut self, other: GaiseSupport) -> bool {
        if self.is_unknown() && !other.is_unknown() {
            *self = other;
            true
        } else {
            false
        }
    }
}

/// Where a capability claim came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GaiseMetadataSource {
    /// Reported by the provider's model API.
    Provider,
    /// Filled from the bundled `model-registry.toml`.
    #[default]
    Registry,
    /// Inferred from the model identifier by an adapter rule.
    Heuristic,
}

/// Lifecycle state of a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GaiseModelStatus {
    Active,
    Preview,
    Deprecated,
    Legacy,
    Retired,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GaiseModelCapabilities {
    /// Input modalities. An empty list means *unknown*: every model accepts at
    /// least one modality, so emptiness can never mean "none".
    #[serde(default)]
    pub input: Vec<GaiseModality>,
    /// Output modalities. Empty means unknown, as for `input`.
    #[serde(default)]
    pub output: Vec<GaiseModality>,
    /// GAISe operations this model can be driven through. Empty means no known
    /// GAISe surface maps to the model (for example image-generation-only
    /// models) *or* that nothing is known; consult `sources`.
    #[serde(default)]
    pub operations: Vec<GaiseOperation>,
    #[serde(default)]
    pub tools: GaiseSupport,
    #[serde(default)]
    pub reasoning: GaiseSupport,
    /// Accepted reasoning control values where the provider or registry
    /// enumerates them (effort levels, thinking levels, booleans).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_values: Option<Vec<String>>,
    #[serde(default)]
    pub structured_output: GaiseSupport,
    /// Provenance of the claims above, in the order they were applied.
    #[serde(default)]
    pub sources: Vec<GaiseMetadataSource>,
}

impl GaiseModelCapabilities {
    pub fn supports(&self, operation: GaiseOperation) -> bool {
        self.operations.contains(&operation)
    }

    pub fn add_input(&mut self, modality: GaiseModality) {
        push_unique(&mut self.input, modality);
    }

    pub fn add_output(&mut self, modality: GaiseModality) {
        push_unique(&mut self.output, modality);
    }

    pub fn add_operation(&mut self, operation: GaiseOperation) {
        push_unique(&mut self.operations, operation);
    }

    pub fn add_source(&mut self, source: GaiseMetadataSource) {
        push_unique(&mut self.sources, source);
    }
}

fn push_unique<T: PartialEq + Ord>(items: &mut Vec<T>, item: T) {
    if !items.contains(&item) {
        items.push(item);
        items.sort();
    }
}

/// Token and size limits for one model.
///
/// `None` always means *unknown*, never "unlimited". Values come from the
/// provider's model API where it reports them (Anthropic `max_input_tokens` /
/// `max_tokens`, Gemini `inputTokenLimit` / `outputTokenLimit`, Ollama
/// `context_length`) and otherwise from the bundled registry, which records the
/// vendor-documented figures; `capabilities.sources` says which applied.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GaiseModelLimits {
    /// The model's context window in tokens: the vendor-documented total a
    /// single request can hold. OpenAI, Anthropic, Bedrock, and Ollama
    /// document one shared window for prompt plus generation; Google
    /// documents an *input* token limit with a separate output limit, and
    /// that input limit is recorded here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// Maximum tokens in one input where the provider reports an input-side
    /// ceiling distinct from the window: the per-text limit of embedding
    /// models, Anthropic's `max_input_tokens`, Gemini's `inputTokenLimit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u64>,
    /// Maximum tokens the model can generate in one response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    /// Maximum input characters per request, for models billed and bounded
    /// by characters rather than tokens (text-to-speech).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_input_characters: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_dimensions: Option<u32>,
}

impl GaiseModelLimits {
    /// Fill every unknown field from `other`. Returns `true` if anything was
    /// applied; known values are never replaced.
    pub fn fill(&mut self, other: &GaiseModelLimits) -> bool {
        let mut applied = false;
        if self.context_window.is_none() && other.context_window.is_some() {
            self.context_window = other.context_window;
            applied = true;
        }
        if self.max_input_tokens.is_none() && other.max_input_tokens.is_some() {
            self.max_input_tokens = other.max_input_tokens;
            applied = true;
        }
        if self.max_output_tokens.is_none() && other.max_output_tokens.is_some() {
            self.max_output_tokens = other.max_output_tokens;
            applied = true;
        }
        if self.max_input_characters.is_none() && other.max_input_characters.is_some() {
            self.max_input_characters = other.max_input_characters;
            applied = true;
        }
        if self.embedding_dimensions.is_none() && other.embedding_dimensions.is_some() {
            self.embedding_dimensions = other.embedding_dimensions;
            applied = true;
        }
        applied
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// One row of the registry limits matrix: the documented limits of one
/// registry entry, independent of any provider credentials.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GaiseModelLimitsEntry {
    /// Routable `provider::model` identifier (registry wildcards such as
    /// `ollama::qwen3:*` are kept verbatim).
    pub id: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub status: GaiseModelStatus,
    /// GAISe operations the entry maps to (see [`GaiseModelCapabilities`]).
    #[serde(default)]
    pub operations: Vec<GaiseOperation>,
    #[serde(flatten)]
    pub limits: GaiseModelLimits,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// The registry's model × limits matrix, as served by `GET /v1/models/limits`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GaiseModelLimitsMatrix {
    /// Date the bundled registry was last audited against vendor docs.
    pub audited_on: String,
    /// Provenance of every row; always `registry`.
    pub source: GaiseMetadataSource,
    #[serde(default)]
    pub models: Vec<GaiseModelLimitsEntry>,
}

/// A model as seen through GAISe.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GaiseModel {
    /// Model identifier. Adapters return the bare provider identifier; the
    /// router rewrites it to the routable `provider::model` form.
    pub id: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// RFC 3339 creation/release timestamp when the provider reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default)]
    pub status: GaiseModelStatus,
    /// Announced shutdown date (`YYYY-MM-DD`), after which requests fail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retires_on: Option<String>,
    /// Earliest date the provider has committed to keep the model available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retirement_not_before: Option<String>,
    /// Recommended replacement for deprecated or retired models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
    /// Free-text GAISe support notes (registry `gaise_support` / `notes`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default)]
    pub capabilities: GaiseModelCapabilities,
    #[serde(default)]
    pub limits: GaiseModelLimits,
    /// The provider's native record, untouched, when `include_raw` was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
}

impl GaiseModel {
    pub fn new(provider: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            provider: provider.into(),
            ..Default::default()
        }
    }

    /// The routable `provider::id` string accepted by `GaiseClientService`.
    pub fn routing_id(&self) -> String {
        format!("{}::{}", self.provider, self.id)
    }
}

/// Format a Unix timestamp (seconds) as an RFC 3339 UTC string.
///
/// Providers report creation times as epoch seconds (OpenAI) or RFC 3339
/// strings (Anthropic); the contract normalizes on the latter.
pub fn rfc3339_from_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m_ = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m_ <= 2 { y + 1 } else { y };
    format!("{y:04}-{m_:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GaiseListModelsRequest {
    /// Restrict to one provider key (`openai`, `anthropic`, ...). `None`
    /// lists every provider the router has credentials for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Keep only models whose capabilities include this operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<GaiseOperation>,
    /// Fetch per-model detail where that costs extra requests (Ollama
    /// `/api/show`). Off by default to avoid N+1 calls.
    #[serde(default)]
    pub include_details: bool,
    /// Attach the provider's native record to each model.
    #[serde(default)]
    pub include_raw: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// Per-request provider endpoint/credential overrides (take precedence
    /// over the router configuration for this call only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<super::GaiseConnection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaiseProviderError {
    pub provider: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GaiseListModelsResponse {
    #[serde(default)]
    pub models: Vec<GaiseModel>,
    /// Providers that were asked and failed. Aggregated listings succeed
    /// partially rather than silently dropping a provider.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<GaiseProviderError>,
}

impl GaiseListModelsResponse {
    pub fn from_models(models: Vec<GaiseModel>) -> Self {
        Self {
            models,
            errors: Vec::new(),
        }
    }

    /// Apply the request's operation filter in place.
    pub fn retain_operation(&mut self, operation: Option<GaiseOperation>) {
        if let Some(operation) = operation {
            self.models.retain(|m| m.capabilities.supports(operation));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_fill_only_replaces_unknown() {
        let mut value = GaiseSupport::Unknown;
        assert!(value.fill(GaiseSupport::Unsupported));
        assert_eq!(value, GaiseSupport::Unsupported);
        assert!(!value.fill(GaiseSupport::Supported));
        assert_eq!(value, GaiseSupport::Unsupported);
    }

    #[test]
    fn modality_lists_are_unique_and_sorted() {
        let mut caps = GaiseModelCapabilities::default();
        caps.add_input(GaiseModality::Image);
        caps.add_input(GaiseModality::Text);
        caps.add_input(GaiseModality::Image);
        assert_eq!(caps.input, vec![GaiseModality::Text, GaiseModality::Image]);
    }

    #[test]
    fn serializes_with_snake_case_and_omits_unknown_optionals() {
        let mut model = GaiseModel::new("openai", "gpt-5.6");
        model.capabilities.add_operation(GaiseOperation::InstructStream);
        model.capabilities.add_input(GaiseModality::Text);
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(json["capabilities"]["operations"][0], "instruct_stream");
        assert_eq!(json["capabilities"]["input"][0], "text");
        assert_eq!(json["capabilities"]["tools"], "unknown");
        assert_eq!(json["status"], "unknown");
        assert!(json.get("raw").is_none());
        assert!(json.get("retires_on").is_none());
        assert_eq!(model.routing_id(), "openai::gpt-5.6");
    }

    #[test]
    fn operation_parse_accepts_aliases() {
        assert_eq!(GaiseOperation::parse("Embedding"), Some(GaiseOperation::Embeddings));
        assert_eq!(GaiseOperation::parse("realtime"), Some(GaiseOperation::Live));
        assert_eq!(GaiseOperation::parse("stream"), Some(GaiseOperation::InstructStream));
        assert_eq!(GaiseOperation::parse("nope"), None);
    }

    #[test]
    fn formats_unix_timestamps_as_rfc3339() {
        assert_eq!(rfc3339_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_from_unix(1_686_935_002), "2023-06-16T17:03:22Z");
        assert_eq!(rfc3339_from_unix(1_709_164_800), "2024-02-29T00:00:00Z");
        assert_eq!(rfc3339_from_unix(1_784_851_200), "2026-07-24T00:00:00Z");
    }

    #[test]
    fn limits_fill_only_replaces_unknown() {
        let mut limits = GaiseModelLimits {
            context_window: Some(200_000),
            ..Default::default()
        };
        let registry = GaiseModelLimits {
            context_window: Some(1_000_000),
            max_output_tokens: Some(128_000),
            ..Default::default()
        };
        assert!(limits.fill(&registry));
        assert_eq!(limits.context_window, Some(200_000), "provider value wins");
        assert_eq!(limits.max_output_tokens, Some(128_000));
        assert!(!limits.fill(&registry), "nothing left to fill");
        assert!(GaiseModelLimits::default().is_empty());
    }

    #[test]
    fn limits_entry_flattens_limits_into_the_row() {
        let entry = GaiseModelLimitsEntry {
            id: "openai::gpt-5.6".into(),
            provider: "openai".into(),
            limits: GaiseModelLimits {
                context_window: Some(400_000),
                max_output_tokens: Some(128_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["context_window"], 400_000);
        assert_eq!(json["max_output_tokens"], 128_000);
        assert!(json.get("max_input_tokens").is_none());
        assert!(json.get("limits").is_none());
        let back: GaiseModelLimitsEntry = serde_json::from_value(json).unwrap();
        assert_eq!(back, entry);
    }

    #[test]
    fn retain_operation_filters_models() {
        let mut text = GaiseModel::new("p", "a");
        text.capabilities.add_operation(GaiseOperation::Instruct);
        let mut embed = GaiseModel::new("p", "b");
        embed.capabilities.add_operation(GaiseOperation::Embeddings);
        let mut response = GaiseListModelsResponse::from_models(vec![text, embed]);
        response.retain_operation(Some(GaiseOperation::Embeddings));
        assert_eq!(response.models.len(), 1);
        assert_eq!(response.models[0].id, "b");
    }
}
