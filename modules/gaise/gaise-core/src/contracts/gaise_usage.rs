use std::collections::HashMap;

/// Provider-reported usage counters.
///
/// `input` and `output` keep the counters on the side to which they belong,
/// including any modality or cache breakdowns exposed by the provider.  The
/// maps may contain overlapping counters (for example, `input_tokens` includes
/// `cached_tokens` for OpenAI), so callers must not sum every value in a map.
/// Provider-reported request-wide counters live in `total`; they are never
/// placed in `output` merely because they arrive with the final response.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct GaiseUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<HashMap<String, usize>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<HashMap<String, usize>>,

    /// Request-wide counters such as `total_tokens`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<HashMap<String, usize>>,
}
