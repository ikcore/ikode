macro_rules! muse {
    ($module:ident, {$($item:ident),* $(,)?}) => {
        pub mod $module;
        pub use $module::{ $($item),* };
    };
}

muse!(gaise_content, {GaiseContent});
muse!(gaise_message, {GaiseMessage});
muse!(gaise_usage, {GaiseUsage});

muse!(gaise_generation_config, {GaiseGenerationConfig});
muse!(gaise_instruct_request, {GaiseInstructRequest});
muse!(gaise_instruct_response, {GaiseInstructResponse});
muse!(gaise_instruct_stream_response, {GaiseInstructStreamResponse, GaiseStreamChunk, GaiseStreamAccumulator});
muse!(gaise_embeddings_request, {GaiseEmbeddingsRequest});
muse!(gaise_embeddings_response, {GaiseEmbeddingsResponse});

muse!(gaise_tool_config, {GaiseToolConfig});
muse!(gaise_tool_call, {GaiseToolCall, GaiseFunctionCall});
muse!(gaise_tool_parameter, {GaiseToolParameter, GaiseTool});

use serde::{Serialize,Deserialize};

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