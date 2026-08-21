use super::{GaiseLiveEvent, GaiseLiveInput};
use futures_util::Stream;
use std::pin::Pin;

pub type GaiseLiveEventStream = Pin<
    Box<
        dyn Stream<Item = Result<GaiseLiveEvent, Box<dyn std::error::Error + Send + Sync>>> + Send,
    >,
>;

pub struct GaiseLiveSession {
    pub tx: tokio::sync::mpsc::Sender<GaiseLiveInput>,
    pub rx: GaiseLiveEventStream,
}
