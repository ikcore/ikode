#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct GaiseImageConfig {
    /// Provider-native aspect ratio, for example `1:1`, `16:9`, or `9:16`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,

    /// Provider-native output size, for example `1K`, `2K`, or `4K`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_size: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GaiseGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<usize>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<usize>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_tokens: Option<usize>,

    /// Reasoning effort. Canonical values are `none`, `auto`, `minimal`,
    /// `low`, `medium`, `high`, `xhigh`, and `max` (aliases such as `off`,
    /// `adaptive`, `extra_high`, and `maximum` are accepted); any other string
    /// is forwarded to the provider verbatim. See
    /// [`GaiseReasoningEffort`](super::GaiseReasoningEffort) for the mapping
    /// rules each adapter applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_effort: Option<String>,

    /// Whether providers that can expose thought summaries/traces should request
    /// them. Providers may still withhold private chain-of-thought.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_thoughts: Option<bool>,

    /// Requested output modalities, such as `TEXT`, `IMAGE`, or `AUDIO`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_modalities: Option<Vec<String>>,

    /// Output image controls used by multimodal generation models.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_config: Option<GaiseImageConfig>,

    /// OpenAI image-input detail (`low`, `high`, `auto`, or `original`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_image_detail: Option<String>,

    /// Provider-native input media resolution. Gemini and Vertex accept values
    /// such as `MEDIA_RESOLUTION_LOW`, `MEDIA_RESOLUTION_MEDIUM`, and
    /// `MEDIA_RESOLUTION_HIGH` for images, video frames, and PDFs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_media_resolution: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
}

impl GaiseGenerationConfig {
    /// The requested effort interpreted through the canonical vocabulary.
    pub fn reasoning_effort(&self) -> Option<super::GaiseReasoningEffort> {
        self.thinking_effort
            .as_deref()
            .map(super::GaiseReasoningEffort::parse)
    }
}
