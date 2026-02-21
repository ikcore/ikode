use async_trait::async_trait;

use crate::contracts::{GaiseEmbeddingsRequest, GaiseEmbeddingsResponse, GaiseInstructRequest, GaiseInstructResponse, GaiseInstructStreamResponse};
pub mod contracts;
pub mod logging;

#[async_trait]
pub trait GaiseClient : Send + Sync {

    async fn instruct_stream(
        &self,
        request: &GaiseInstructRequest,
    ) -> Result<
        std::pin::Pin<
            Box<
                dyn futures_util::Stream<
                        Item = Result<GaiseInstructStreamResponse, Box<dyn std::error::Error + Send + Sync>>,
                    > + Send,
            >,
        >,
        Box<dyn std::error::Error + Send + Sync>,
    >;

    async fn instruct(&self, request:&GaiseInstructRequest) -> Result<GaiseInstructResponse, Box<dyn std::error::Error + Send + Sync>>;
    async fn embeddings(&self, request:&GaiseEmbeddingsRequest) -> Result<GaiseEmbeddingsResponse, Box<dyn std::error::Error + Send + Sync>>;
}

