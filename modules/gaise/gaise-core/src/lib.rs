use async_trait::async_trait;

use crate::contracts::{
    GaiseEmbeddingsRequest, GaiseEmbeddingsResponse, GaiseInstructRequest, GaiseInstructResponse,
    GaiseInstructStreamResponse, GaiseListModelsRequest, GaiseListModelsResponse, GaiseLiveConfig,
    GaiseLiveSession, GaiseSpeechRequest, GaiseSpeechResponse, GaiseSpeechStreamResponse,
};
pub mod contracts;
pub mod logging;
pub mod registry;

#[async_trait]
pub trait GaiseClient: Send + Sync {
    async fn instruct_stream(
        &self,
        request: &GaiseInstructRequest,
    ) -> Result<
        std::pin::Pin<
            Box<
                dyn futures_util::Stream<
                        Item = Result<
                            GaiseInstructStreamResponse,
                            Box<dyn std::error::Error + Send + Sync>,
                        >,
                    > + Send,
            >,
        >,
        Box<dyn std::error::Error + Send + Sync>,
    >;

    async fn instruct(
        &self,
        request: &GaiseInstructRequest,
    ) -> Result<GaiseInstructResponse, Box<dyn std::error::Error + Send + Sync>>;
    async fn embeddings(
        &self,
        request: &GaiseEmbeddingsRequest,
    ) -> Result<GaiseEmbeddingsResponse, Box<dyn std::error::Error + Send + Sync>>;

    /// List the models this client can reach.
    ///
    /// Adapters return bare provider model identifiers and only the
    /// capabilities the provider API actually reports; see
    /// [`contracts::GaiseModel`] for the unknown-vs-unsupported rules and
    /// [`registry`] for the advisory overlay applied by the router. The
    /// default implementation reports that listing is unsupported so custom
    /// clients keep compiling.
    async fn list_models(
        &self,
        request: &GaiseListModelsRequest,
    ) -> Result<GaiseListModelsResponse, Box<dyn std::error::Error + Send + Sync>> {
        let _ = request;
        Err("model listing is not supported by this client".into())
    }
}

#[async_trait]
pub trait GaiseLiveClient: Send + Sync {
    async fn live_connect(
        &self,
        config: &GaiseLiveConfig,
    ) -> Result<GaiseLiveSession, Box<dyn std::error::Error + Send + Sync>>;
}

/// Text-to-speech. Realtime text-in/audio-out streaming uses
/// [`GaiseLiveClient`] with `GaiseLiveInput::Text` and `GaiseLiveEvent::Audio`.
#[async_trait]
pub trait GaiseSpeechClient: Send + Sync {
    async fn speech(
        &self,
        request: &GaiseSpeechRequest,
    ) -> Result<GaiseSpeechResponse, Box<dyn std::error::Error + Send + Sync>>;

    async fn speech_stream(
        &self,
        request: &GaiseSpeechRequest,
    ) -> Result<
        std::pin::Pin<
            Box<
                dyn futures_util::Stream<
                        Item = Result<
                            GaiseSpeechStreamResponse,
                            Box<dyn std::error::Error + Send + Sync>,
                        >,
                    > + Send,
            >,
        >,
        Box<dyn std::error::Error + Send + Sync>,
    >;
}
