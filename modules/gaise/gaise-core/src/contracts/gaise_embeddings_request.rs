//! Provider-neutral embedding contract and the resolution rules every
//! adapter applies — the embedding counterpart of
//! [`GaiseReasoningEffort`](super::GaiseReasoningEffort).
//!
//! - [`GaiseEmbeddingTask`] is the canonical task vocabulary (with aliases
//!   and a custom pass-through).
//! - [`EmbeddingProfile`] describes one model: dimension options, limits,
//!   normalization behaviour, and how it wants the task expressed
//!   ([`EmbeddingTaskControl`]). Profiles live in `model-registry.toml`
//!   (`[models.embedding]`) and are looked up through
//!   `gaise_core::registry::ModelRegistry::embedding_profile`.
//! - [`resolve_embedding`] turns a request plus a profile into what an
//!   adapter should send: prefixed texts, the wire task value, snapped
//!   dimensions, and whether to normalize locally. Unknown models get the
//!   adapter's provider default and pass-through dimensions.

use super::OneOrMany;
use serde::{Deserialize, Serialize};

/// What the vectors are for. Providers that distinguish document and query
/// embeddings produce measurably better retrieval when told; providers
/// without the concept ignore it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GaiseEmbeddingTask {
    /// Text being indexed for later retrieval (the default everywhere).
    Document,
    /// A search query to match against indexed documents.
    Query,
    Classification,
    Clustering,
    /// Symmetric similarity between two texts.
    Similarity,
    /// A natural-language query for code retrieval.
    CodeQuery,
    FactVerification,
    QuestionAnswering,
    /// A vendor-specific value GAISe does not recognise; forwarded verbatim
    /// to providers with a task field, ignored elsewhere.
    Custom(String),
}

impl GaiseEmbeddingTask {
    /// Canonical names and aliases (case-insensitive, `-`/` ` treated as `_`).
    /// Unknown text becomes `Custom`.
    pub fn parse(value: &str) -> Self {
        let v = value.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        match v.as_str() {
            "document" | "retrieval_document" | "search_document" | "index" | "passage"
            | "generic_index" => Self::Document,
            "query" | "retrieval_query" | "search_query" | "search" | "generic_retrieval"
            | "text_retrieval" => Self::Query,
            "classification" | "classify" => Self::Classification,
            "clustering" | "cluster" => Self::Clustering,
            "similarity" | "semantic_similarity" | "sts" | "sentence_similarity" => {
                Self::Similarity
            }
            "code_query" | "code_retrieval_query" | "code" | "code_retrieval" => Self::CodeQuery,
            "fact_verification" | "fact" | "fact_checking" => Self::FactVerification,
            "question_answering" | "qa" => Self::QuestionAnswering,
            _ => Self::Custom(value.trim().to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Document => "document",
            Self::Query => "query",
            Self::Classification => "classification",
            Self::Clustering => "clustering",
            Self::Similarity => "similarity",
            Self::CodeQuery => "code_query",
            Self::FactVerification => "fact_verification",
            Self::QuestionAnswering => "question_answering",
            Self::Custom(raw) => raw,
        }
    }

    /// True for the query side of asymmetric retrieval.
    pub fn is_query_like(&self) -> bool {
        matches!(
            self,
            Self::Query | Self::CodeQuery | Self::QuestionAnswering | Self::FactVerification
        )
    }

    /// Google's documented prompt convention for models that take the task as
    /// an instruction instead of a `taskType` field (`gemini-embedding-2`,
    /// EmbeddingGemma). Documents carry `title: none | text: …`, queries
    /// `task: search result | query: …`, and so on.
    pub fn gemini_instruction(&self, text: &str) -> String {
        match self {
            Self::Document => format!("title: none | text: {text}"),
            Self::Query => format!("task: search result | query: {text}"),
            Self::QuestionAnswering => format!("task: question answering | query: {text}"),
            Self::FactVerification => format!("task: fact checking | query: {text}"),
            Self::CodeQuery => format!("task: code retrieval | query: {text}"),
            Self::Classification => format!("task: classification | text: {text}"),
            Self::Clustering => format!("task: clustering | text: {text}"),
            Self::Similarity => format!("task: sentence similarity | text: {text}"),
            Self::Custom(raw) => format!("task: {raw} | text: {text}"),
        }
    }

    /// Gemini / Vertex `taskType` enum value.
    pub fn google_task_type(&self) -> String {
        match self {
            Self::Document => "RETRIEVAL_DOCUMENT".into(),
            Self::Query => "RETRIEVAL_QUERY".into(),
            Self::Classification => "CLASSIFICATION".into(),
            Self::Clustering => "CLUSTERING".into(),
            Self::Similarity => "SEMANTIC_SIMILARITY".into(),
            Self::CodeQuery => "CODE_RETRIEVAL_QUERY".into(),
            Self::FactVerification => "FACT_VERIFICATION".into(),
            Self::QuestionAnswering => "QUESTION_ANSWERING".into(),
            Self::Custom(raw) => raw.to_ascii_uppercase(),
        }
    }

    /// Cohere `input_type` value.
    pub fn cohere_input_type(&self) -> String {
        match self {
            Self::Classification => "classification".into(),
            Self::Clustering => "clustering".into(),
            Self::Custom(raw) => raw.clone(),
            t if t.is_query_like() => "search_query".into(),
            _ => "search_document".into(),
        }
    }

    /// Amazon Nova `embeddingPurpose` value. GAISe only sends text, so
    /// queries become `TEXT_RETRIEVAL`; pass a custom task
    /// (`GENERIC_RETRIEVAL`, `IMAGE_RETRIEVAL`, …) to search a mixed index.
    pub fn nova_embedding_purpose(&self) -> String {
        match self {
            Self::Document => "GENERIC_INDEX".into(),
            Self::Classification => "CLASSIFICATION".into(),
            Self::Clustering => "CLUSTERING".into(),
            Self::Custom(raw) => raw.to_ascii_uppercase(),
            t if t.is_query_like() => "TEXT_RETRIEVAL".into(),
            _ => "GENERIC_INDEX".into(),
        }
    }
}

impl Serialize for GaiseEmbeddingTask {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GaiseEmbeddingTask {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::parse(&String::deserialize(deserializer)?))
    }
}

/// How a model wants the task expressed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingTaskControl {
    /// The model has no task concept; `task` is ignored.
    #[default]
    None,
    /// Gemini / Vertex `taskType` field.
    TaskType,
    /// Cohere `input_type` field (required by the API).
    InputType,
    /// Amazon Nova `embeddingPurpose` field.
    EmbeddingPurpose,
    /// Google instruction prefix in the text (`task: search result | query: …`).
    PromptInstruction,
    /// Fixed text prefixes per side (nomic: `search_query: `, `search_document: `).
    Prefix {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        document: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        classification: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clustering: Option<String>,
    },
    /// An instruction prepended to queries only (mxbai, Arctic, Qwen3).
    QueryInstruction(String),
}

/// Which output sizes a model offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DimensionRule {
    /// Not selectable; `dimensions` is ignored.
    #[default]
    Fixed,
    /// Any value in `min..=max` (Matryoshka).
    Range { min: u32, max: u32 },
    /// A discrete set; requests snap to the nearest.
    Set(Vec<u32>),
}

/// Everything an adapter needs to know about one embedding model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct EmbeddingProfile {
    /// Vector length when `dimensions` is not set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_dimensions: Option<u32>,
    #[serde(default)]
    pub dimension_rule: DimensionRule,
    /// Longest single input the model accepts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u32>,
    /// Inputs per request the provider accepts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_batch: Option<u32>,
    /// Full-size vectors come back unit-length.
    #[serde(default)]
    pub normalized_output: bool,
    /// Truncated (reduced-dimension) vectors are re-normalized by the provider.
    #[serde(default)]
    pub normalizes_truncation: bool,
    #[serde(default)]
    pub task_control: EmbeddingTaskControl,
    /// The provider accepts exactly one input per call (Vertex `gemini-embedding-001`).
    #[serde(default)]
    pub single_input: bool,
}

/// What to put on the wire for one request, after the profile rules.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEmbedding {
    /// Inputs with any prefix/instruction applied, in order.
    pub texts: Vec<String>,
    /// Value for the provider's task field (`taskType`, `input_type`,
    /// `embeddingPurpose`), when the control is a field.
    pub wire_task: Option<String>,
    /// Dimensions to send, after snapping/clamping; `None` = omit.
    pub dimensions: Option<u32>,
    /// The adapter must L2-normalize the result itself.
    pub normalize_locally: bool,
    /// Send one input per provider call.
    pub single_input: bool,
}

/// Snap a requested dimension count onto the profile's rule.
pub fn resolve_dimensions(rule: &DimensionRule, requested: Option<u32>) -> Option<u32> {
    let requested = requested?;
    match rule {
        DimensionRule::Fixed => None,
        DimensionRule::Range { min, max } => Some(requested.clamp(*min, *max)),
        DimensionRule::Set(options) => snap_dimensions(requested, options),
    }
}

/// Apply the profile rules. `default_control` is the adapter's provider-level
/// control for models without a profile (Cohere always wants `input_type`,
/// Gemini defaults to `taskType`, OpenAI to none).
pub fn resolve_embedding(
    request: &GaiseEmbeddingsRequest,
    profile: Option<&EmbeddingProfile>,
    default_control: &EmbeddingTaskControl,
) -> ResolvedEmbedding {
    let inputs: Vec<String> = match &request.input {
        OneOrMany::One(s) => vec![s.clone()],
        OneOrMany::Many(v) => v.clone(),
    };
    let control = profile
        .map(|p| &p.task_control)
        .unwrap_or(default_control);
    let task = request.task.clone();

    let (texts, wire_task) = match (control, &task) {
        (EmbeddingTaskControl::None, _) | (_, None) => {
            // Field controls that the API requires still need a default value.
            let wire = match control {
                EmbeddingTaskControl::InputType => Some("search_document".to_string()),
                EmbeddingTaskControl::EmbeddingPurpose => Some("GENERIC_INDEX".to_string()),
                _ => None,
            };
            (inputs, wire)
        }
        (EmbeddingTaskControl::TaskType, Some(t)) => (inputs, Some(t.google_task_type())),
        (EmbeddingTaskControl::InputType, Some(t)) => (inputs, Some(t.cohere_input_type())),
        (EmbeddingTaskControl::EmbeddingPurpose, Some(t)) => {
            (inputs, Some(t.nova_embedding_purpose()))
        }
        (EmbeddingTaskControl::PromptInstruction, Some(t)) => (
            inputs.into_iter().map(|x| t.gemini_instruction(&x)).collect(),
            None,
        ),
        (
            EmbeddingTaskControl::Prefix {
                query,
                document,
                classification,
                clustering,
            },
            Some(t),
        ) => {
            let prefix = match t {
                GaiseEmbeddingTask::Classification => classification.as_deref(),
                GaiseEmbeddingTask::Clustering => clustering.as_deref(),
                t if t.is_query_like() => query.as_deref(),
                _ => document.as_deref(),
            }
            .unwrap_or("");
            (
                inputs.into_iter().map(|x| format!("{prefix}{x}")).collect(),
                None,
            )
        }
        (EmbeddingTaskControl::QueryInstruction(instruction), Some(t)) => {
            if t.is_query_like() {
                (
                    inputs
                        .into_iter()
                        .map(|x| format!("{instruction}{x}"))
                        .collect(),
                    None,
                )
            } else {
                (inputs, None)
            }
        }
    };

    let dimensions = match profile {
        Some(p) => resolve_dimensions(&p.dimension_rule, request.dimensions),
        // Unknown model: forward as given and let the provider validate.
        None => request.dimensions,
    };
    let truncated = dimensions.is_some()
        && profile.is_some_and(|p| {
            p.default_dimensions
                .is_none_or(|d| dimensions.is_some_and(|r| r < d))
        });
    let provider_normalizes = profile.is_some_and(|p| {
        if truncated {
            p.normalizes_truncation
        } else {
            p.normalized_output
        }
    });
    let normalize_locally = match request.normalize {
        Some(true) => !provider_normalizes,
        Some(false) => false,
        // Unit-length by default: fix up truncations the provider leaves raw.
        None => truncated && !provider_normalizes,
    };

    ResolvedEmbedding {
        texts,
        wire_task,
        dimensions,
        normalize_locally,
        single_input: profile.is_some_and(|p| p.single_input),
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct GaiseEmbeddingsRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// Per-request provider endpoint/credential overrides (take precedence
    /// over the router configuration for this call only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<super::GaiseConnection>,
    pub input: OneOrMany<String>,
    /// Intended use of the vectors: `document` (default), `query`,
    /// `classification`, `clustering`, `similarity`, `code_query`,
    /// `fact_verification`, `question_answering`, aliases, or a vendor value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<GaiseEmbeddingTask>,
    /// Requested vector length for Matryoshka models; snapped to what the
    /// model offers and ignored by fixed-size models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<u32>,
    /// Unit-length vectors. `true` forces it (natively where the provider has
    /// a flag, locally otherwise); `false` leaves raw magnitudes; unset keeps
    /// the provider's output but repairs truncations the provider leaves
    /// un-normalized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalize: Option<bool>,
}

/// L2-normalize a vector in place (no-op for the zero vector).
pub fn normalize_l2(vector: &mut [f32]) {
    let norm = vector
        .iter()
        .map(|v| (*v as f64) * (*v as f64))
        .sum::<f64>()
        .sqrt();
    if norm > 0.0 {
        for v in vector.iter_mut() {
            *v = (*v as f64 / norm) as f32;
        }
    }
}

/// Snap a requested dimension count onto a discrete set (nearest, ties
/// resolve upward). Returns `None` for an empty set.
pub fn snap_dimensions(requested: u32, supported: &[u32]) -> Option<u32> {
    if supported.is_empty() {
        return None;
    }
    supported
        .iter()
        .copied()
        .min_by_key(|d| (d.abs_diff(requested), u32::from(*d < requested)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(task: Option<&str>, dims: Option<u32>, normalize: Option<bool>) -> GaiseEmbeddingsRequest {
        GaiseEmbeddingsRequest {
            model: "m".into(),
            input: OneOrMany::Many(vec!["a".into(), "b".into()]),
            task: task.map(GaiseEmbeddingTask::parse),
            dimensions: dims,
            normalize,
            ..Default::default()
        }
    }

    #[test]
    fn parses_task_aliases_and_custom() {
        assert_eq!(GaiseEmbeddingTask::parse("search_query"), GaiseEmbeddingTask::Query);
        assert_eq!(GaiseEmbeddingTask::parse("RETRIEVAL_DOCUMENT"), GaiseEmbeddingTask::Document);
        assert_eq!(GaiseEmbeddingTask::parse("semantic-similarity"), GaiseEmbeddingTask::Similarity);
        assert_eq!(GaiseEmbeddingTask::parse("IMAGE_RETRIEVAL"), GaiseEmbeddingTask::Custom("IMAGE_RETRIEVAL".into()));
        let parsed: GaiseEmbeddingTask = serde_json::from_str("\"search-query\"").unwrap();
        assert_eq!(parsed, GaiseEmbeddingTask::Query);
        assert_eq!(serde_json::to_string(&GaiseEmbeddingTask::CodeQuery).unwrap(), "\"code_query\"");
        assert_eq!(GaiseEmbeddingTask::Query.cohere_input_type(), "search_query");
        assert_eq!(GaiseEmbeddingTask::CodeQuery.google_task_type(), "CODE_RETRIEVAL_QUERY");
        assert_eq!(GaiseEmbeddingTask::Query.nova_embedding_purpose(), "TEXT_RETRIEVAL");
        assert_eq!(GaiseEmbeddingTask::parse("image_retrieval").nova_embedding_purpose(), "IMAGE_RETRIEVAL");
        assert_eq!(GaiseEmbeddingTask::Query.gemini_instruction("x"), "task: search result | query: x");
    }

    #[test]
    fn resolves_field_controls_with_required_defaults() {
        let cohere = EmbeddingProfile {
            task_control: EmbeddingTaskControl::InputType,
            dimension_rule: DimensionRule::Set(vec![256, 512, 1024, 1536]),
            default_dimensions: Some(1536),
            ..Default::default()
        };
        let r = resolve_embedding(&request(None, Some(700), None), Some(&cohere), &EmbeddingTaskControl::None);
        assert_eq!(r.wire_task.as_deref(), Some("search_document"), "required field gets a default");
        assert_eq!(r.dimensions, Some(512));
        assert!(r.normalize_locally, "truncated and the provider does not re-normalize");
        let r = resolve_embedding(&request(Some("query"), None, None), Some(&cohere), &EmbeddingTaskControl::None);
        assert_eq!(r.wire_task.as_deref(), Some("search_query"));
        assert!(!r.normalize_locally);

        let google = EmbeddingProfile {
            task_control: EmbeddingTaskControl::TaskType,
            dimension_rule: DimensionRule::Range { min: 128, max: 3072 },
            default_dimensions: Some(3072),
            normalized_output: true,
            ..Default::default()
        };
        let r = resolve_embedding(&request(Some("code"), Some(50), None), Some(&google), &EmbeddingTaskControl::None);
        assert_eq!(r.wire_task.as_deref(), Some("CODE_RETRIEVAL_QUERY"));
        assert_eq!(r.dimensions, Some(128));
        assert!(r.normalize_locally, "001-style model leaves truncations raw");
        let g2 = EmbeddingProfile { normalizes_truncation: true, ..google.clone() };
        assert!(!resolve_embedding(&request(None, Some(768), None), Some(&g2), &EmbeddingTaskControl::None).normalize_locally);
    }

    #[test]
    fn resolves_text_controls() {
        let instruction = EmbeddingProfile { task_control: EmbeddingTaskControl::PromptInstruction, ..Default::default() };
        let r = resolve_embedding(&request(Some("query"), None, None), Some(&instruction), &EmbeddingTaskControl::None);
        assert_eq!(r.texts, vec!["task: search result | query: a", "task: search result | query: b"]);
        assert!(r.wire_task.is_none());

        let nomic = EmbeddingProfile {
            task_control: EmbeddingTaskControl::Prefix {
                query: Some("search_query: ".into()),
                document: Some("search_document: ".into()),
                classification: Some("classification: ".into()),
                clustering: None,
            },
            ..Default::default()
        };
        assert_eq!(resolve_embedding(&request(Some("document"), None, None), Some(&nomic), &EmbeddingTaskControl::None).texts[0], "search_document: a");
        assert_eq!(resolve_embedding(&request(Some("qa"), None, None), Some(&nomic), &EmbeddingTaskControl::None).texts[0], "search_query: a");
        assert_eq!(resolve_embedding(&request(Some("clustering"), None, None), Some(&nomic), &EmbeddingTaskControl::None).texts[0], "a", "missing prefix = no prefix");
        assert_eq!(resolve_embedding(&request(None, None, None), Some(&nomic), &EmbeddingTaskControl::None).texts[0], "a", "no task = untouched");

        let mxbai = EmbeddingProfile { task_control: EmbeddingTaskControl::QueryInstruction("Represent this sentence: ".into()), ..Default::default() };
        assert_eq!(resolve_embedding(&request(Some("query"), None, None), Some(&mxbai), &EmbeddingTaskControl::None).texts[0], "Represent this sentence: a");
        assert_eq!(resolve_embedding(&request(Some("document"), None, None), Some(&mxbai), &EmbeddingTaskControl::None).texts[0], "a");
    }

    #[test]
    fn unknown_models_pass_through_with_the_provider_default() {
        let r = resolve_embedding(&request(Some("query"), Some(999), None), None, &EmbeddingTaskControl::InputType);
        assert_eq!(r.wire_task.as_deref(), Some("search_query"));
        assert_eq!(r.dimensions, Some(999), "forwarded for the API to validate");
        assert!(!r.normalize_locally);
        assert!(!r.single_input);
        let r = resolve_embedding(&request(Some("query"), None, Some(true)), None, &EmbeddingTaskControl::None);
        assert!(r.normalize_locally, "explicit normalize on an unknown model is done locally");
    }

    #[test]
    fn normalizes_and_snaps() {
        let mut v = vec![3.0, 4.0];
        normalize_l2(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
        assert_eq!(snap_dimensions(384, &[256, 512, 1024]), Some(512), "tie resolves upward");
        assert_eq!(resolve_dimensions(&DimensionRule::Fixed, Some(256)), None);
        assert_eq!(resolve_dimensions(&DimensionRule::Range { min: 32, max: 4096 }, Some(5000)), Some(4096));
    }
}
