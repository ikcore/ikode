macro_rules! muse {
    ($module:ident, {$($item:ident),* $(,)?}) => {
        pub mod $module;
        pub use $module::{ $($item),* };
    };
}

muse!(gaise_content, {
    audio_media_type,
    file_media_type,
    image_media_type,
    GaiseContent
});
muse!(gaise_message, { GaiseMessage });
muse!(gaise_usage, { GaiseUsage });

muse!(gaise_reasoning, { GaiseReasoningEffort });
muse!(gaise_connection, { GaiseConnection, redact_secrets });
muse!(gaise_generation_config, {
    GaiseGenerationConfig,
    GaiseImageConfig
});
muse!(gaise_instruct_request, { GaiseInstructRequest });
muse!(gaise_instruct_response, { GaiseInstructResponse });
muse!(gaise_instruct_stream_response, {GaiseInstructStreamResponse, GaiseStreamChunk, GaiseStreamAccumulator});
muse!(gaise_embeddings_request, { DimensionRule, EmbeddingProfile, EmbeddingTaskControl, GaiseEmbeddingTask, GaiseEmbeddingsRequest, ResolvedEmbedding, normalize_l2, resolve_dimensions, resolve_embedding, snap_dimensions });
muse!(gaise_embeddings_response, { GaiseEmbeddingsResponse });
muse!(gaise_model, {
    GaiseListModelsRequest,
    GaiseListModelsResponse,
    GaiseMetadataSource,
    GaiseModality,
    GaiseModel,
    GaiseModelCapabilities,
    GaiseModelLimits,
    GaiseModelLimitsEntry,
    GaiseModelLimitsMatrix,
    GaiseModelStatus,
    GaiseOperation,
    GaiseProviderError,
    GaiseSupport,
    rfc3339_from_unix
});

muse!(gaise_tool_config, { GaiseToolConfig });
muse!(gaise_tool_call, {GaiseToolCall, GaiseFunctionCall});
muse!(gaise_tool_parameter, {GaiseToolParameter, GaiseTool});

muse!(gaise_speech, {
    GaiseSpeechAlignment,
    GaiseSpeechChunk,
    GaiseSpeechRequest,
    GaiseSpeechResponse,
    GaiseSpeechStreamResponse,
    GaiseVoiceSettings
});

muse!(gaise_live_config, {GaiseLiveConfig, GaiseLiveModality, GaiseVadConfig, GaiseTranscriptionConfig});
muse!(gaise_live_event, { GaiseLiveEvent });
muse!(gaise_live_input, { GaiseLiveInput });
muse!(gaise_live_session, {GaiseLiveSession, GaiseLiveEventStream});

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

impl<T: Default> Default for OneOrMany<T> {
    fn default() -> Self {
        Self::One(T::default())
    }
}
