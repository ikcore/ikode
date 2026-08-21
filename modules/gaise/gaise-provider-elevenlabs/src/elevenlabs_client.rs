//! ElevenLabs adapter.
//!
//! - [`GaiseSpeechClient`]: `POST /v1/text-to-speech/{voice_id}` (+`/with-timestamps`)
//!   and `/stream` (+`/stream/with-timestamps`).
//! - [`GaiseClient`]: only `list_models` (`GET /v1/models`); chat and
//!   embeddings return explicit "not supported" errors.
//! - [`GaiseLiveClient`] (feature `live`): realtime text-in / audio-out over
//!   `stream-input`, or the text-to-dialogue WebSocket for `eleven_v3*`.

use std::collections::HashMap;
use std::pin::Pin;

use async_trait::async_trait;
use base64::Engine;
use futures_util::{Stream, StreamExt};
use gaise_core::contracts::{
    GaiseEmbeddingsRequest, GaiseEmbeddingsResponse, GaiseInstructRequest, GaiseInstructResponse,
    GaiseInstructStreamResponse, GaiseListModelsRequest, GaiseListModelsResponse,
    GaiseSpeechAlignment, GaiseSpeechChunk, GaiseSpeechRequest, GaiseSpeechResponse,
    GaiseSpeechStreamResponse, GaiseUsage,
};
use gaise_core::{GaiseClient, GaiseSpeechClient};

use crate::contracts::*;

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

pub const DEFAULT_API_URL: &str = "https://api.elevenlabs.io";

#[derive(Clone)]
pub struct GaiseClientElevenLabs {
    api_url: String,
    api_key: String,
    client: reqwest::Client,
}

/// A voice from `GET /v2/voices` (identity only; enough to pick one).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ElevenLabsVoice {
    pub voice_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct VoicesPage {
    #[serde(default)]
    voices: Vec<ElevenLabsVoice>,
    #[serde(default)]
    next_page_token: Option<String>,
}

impl GaiseClientElevenLabs {
    pub fn new(api_url: String, api_key: String) -> Self {
        let api_url = if api_url.trim().is_empty() {
            DEFAULT_API_URL.to_string()
        } else {
            api_url.trim_end_matches('/').to_string()
        };
        Self {
            api_url,
            api_key,
            client: reqwest::Client::new(),
        }
    }

    pub fn api_url(&self) -> &str {
        &self.api_url
    }

    pub(crate) fn api_key(&self) -> &str {
        &self.api_key
    }

    fn voice_for(request: &GaiseSpeechRequest) -> Result<&str, BoxErr> {
        request
            .voice
            .as_deref()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| {
                "ElevenLabs requires a voice id: set `voice` (list them with GaiseClientElevenLabs::list_voices)"
                    .into()
            })
    }

    /// `POST /v1/text-to-speech/{voice}{suffix}?output_format=…`
    async fn send_tts(
        &self,
        request: &GaiseSpeechRequest,
        suffix: &str,
        format: &ElevenLabsOutputFormat,
    ) -> Result<reqwest::Response, BoxErr> {
        let voice = Self::voice_for(request)?;
        let url = format!(
            "{}/v1/text-to-speech/{}{}?output_format={}",
            self.api_url, voice, suffix, format.output_format
        );
        let response = self
            .client
            .post(url)
            .header("xi-api-key", &self.api_key)
            .json(&build_tts_request(request))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format_error(status.as_u16(), &body).into());
        }
        Ok(response)
    }

    fn usage_from(request: &GaiseSpeechRequest, response: &reqwest::Response) -> GaiseUsage {
        let mut input = HashMap::new();
        input.insert("characters".to_string(), request.input.chars().count());
        let cost = response
            .headers()
            .get("character-cost")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<usize>().ok());
        GaiseUsage {
            input: Some(input),
            output: None,
            total: cost.map(|c| HashMap::from([("character_cost".to_string(), c)])),
        }
    }

    fn request_id(response: &reqwest::Response) -> Option<String> {
        response
            .headers()
            .get("request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    }

    /// Page through `GET /v2/voices`.
    pub async fn list_voices(&self) -> Result<Vec<ElevenLabsVoice>, BoxErr> {
        let mut voices = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut url = format!("{}/v2/voices?page_size=100", self.api_url);
            if let Some(t) = &token {
                url.push_str("&next_page_token=");
                url.push_str(t);
            }
            let response = self
                .client
                .get(url)
                .header("xi-api-key", &self.api_key)
                .send()
                .await?;
            let status = response.status();
            let body = response.text().await?;
            if !status.is_success() {
                return Err(format_error(status.as_u16(), &body).into());
            }
            let page: VoicesPage = serde_json::from_str(&body).map_err(|e| {
                let snippet: String = body.chars().take(300).collect();
                format!("failed to parse ElevenLabs voices response: {e}; body starts: {snippet}")
            })?;
            voices.extend(page.voices);
            match page.next_page_token {
                Some(next) if !next.is_empty() && token.as_deref() != Some(next.as_str()) => {
                    token = Some(next)
                }
                _ => break,
            }
        }
        Ok(voices)
    }

    /// `GET /v1/models`.
    pub async fn list_models_raw(&self) -> Result<Vec<ElevenLabsModelInfo>, BoxErr> {
        let response = self
            .client
            .get(format!("{}/v1/models", self.api_url))
            .header("xi-api-key", &self.api_key)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(format_error(status.as_u16(), &body).into());
        }
        serde_json::from_str(&body).map_err(|e| {
            let snippet: String = body.chars().take(300).collect();
            format!("failed to parse ElevenLabs models response: {e}; body starts: {snippet}")
                .into()
        })
    }
}

/// Incremental newline-delimited JSON splitter for `/stream/with-timestamps`.
pub(crate) struct LineBuffer {
    buffer: Vec<u8>,
}

impl LineBuffer {
    pub(crate) fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Feed bytes; returns every complete line (without the terminator).
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(bytes);
        let mut lines = Vec::new();
        while let Some(pos) = self.buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=pos).collect();
            lines.push(
                String::from_utf8_lossy(&line)
                    .trim_end_matches(['\n', '\r'])
                    .to_string(),
            );
        }
        lines
    }

    /// Flush a trailing unterminated line at end of stream.
    pub(crate) fn finish(&mut self) -> Option<String> {
        if self.buffer.is_empty() {
            None
        } else {
            let line = String::from_utf8_lossy(&self.buffer).to_string();
            self.buffer.clear();
            Some(line)
        }
    }
}

fn timestamp_frame_to_chunks(
    frame: ElevenLabsTimestampedAudio,
    format: &ElevenLabsOutputFormat,
) -> Result<Vec<GaiseSpeechChunk>, BoxErr> {
    let mut chunks = Vec::new();
    if let Some(b64) = frame.audio_base64.filter(|a| !a.is_empty()) {
        let data = base64::prelude::BASE64_STANDARD.decode(b64.as_bytes())?;
        chunks.push(GaiseSpeechChunk::Audio {
            data,
            format: format.mime.clone(),
            sample_rate: Some(format.sample_rate),
        });
    }
    if let Some(alignment) = frame
        .alignment
        .as_ref()
        .or(frame.normalized_alignment.as_ref())
        && !alignment.characters.is_empty()
    {
        chunks.push(GaiseSpeechChunk::Alignment(GaiseSpeechAlignment::from(
            alignment,
        )));
    }
    Ok(chunks)
}

#[async_trait]
impl GaiseSpeechClient for GaiseClientElevenLabs {
    async fn speech(&self, request: &GaiseSpeechRequest) -> Result<GaiseSpeechResponse, BoxErr> {
        let format = resolve_output_format(request.format.as_deref(), request.sample_rate)?;
        let suffix = if request.include_alignment {
            "/with-timestamps"
        } else {
            ""
        };
        let response = self.send_tts(request, suffix, &format).await?;
        let usage = Self::usage_from(request, &response);
        let external_id = Self::request_id(&response);

        if request.include_alignment {
            let body = response.text().await?;
            let frame: ElevenLabsTimestampedAudio = serde_json::from_str(&body).map_err(|e| {
                let snippet: String = body.chars().take(300).collect();
                format!(
                    "failed to parse ElevenLabs timestamp response: {e}; body starts: {snippet}"
                )
            })?;
            let audio = frame
                .audio_base64
                .as_deref()
                .map(|b64| base64::prelude::BASE64_STANDARD.decode(b64.as_bytes()))
                .transpose()?
                .unwrap_or_default();
            let alignment = frame
                .alignment
                .as_ref()
                .or(frame.normalized_alignment.as_ref())
                .map(GaiseSpeechAlignment::from);
            return Ok(GaiseSpeechResponse {
                audio,
                format: format.mime,
                sample_rate: Some(format.sample_rate),
                external_id,
                alignment,
                usage: Some(usage),
            });
        }

        let audio = response.bytes().await?.to_vec();
        Ok(GaiseSpeechResponse {
            audio,
            format: format.mime,
            sample_rate: Some(format.sample_rate),
            external_id,
            alignment: None,
            usage: Some(usage),
        })
    }

    async fn speech_stream(
        &self,
        request: &GaiseSpeechRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<GaiseSpeechStreamResponse, BoxErr>> + Send>>, BoxErr>
    {
        let format = resolve_output_format(request.format.as_deref(), request.sample_rate)?;
        let suffix = if request.include_alignment {
            "/stream/with-timestamps"
        } else {
            "/stream"
        };
        let response = self.send_tts(request, suffix, &format).await?;
        let usage = Self::usage_from(request, &response);
        let external_id = Self::request_id(&response);
        let with_alignment = request.include_alignment;
        let bytes = response.bytes_stream();

        let mime = format.mime.clone();
        let rate = format.sample_rate;
        let ext = external_id.clone();
        let wrap = move |chunk: GaiseSpeechChunk| GaiseSpeechStreamResponse {
            chunk,
            external_id: ext.clone(),
        };

        let audio_stream: Pin<
            Box<dyn Stream<Item = Result<GaiseSpeechStreamResponse, BoxErr>> + Send>,
        > = if with_alignment {
            let format_for_frames = format.clone();
            let wrap_frames = wrap.clone();
            Box::pin(
                bytes
                    .map(|b| b.map_err(BoxErr::from))
                    .chain(futures_util::stream::once(async {
                        Ok(bytes::Bytes::new())
                    }))
                    .scan((LineBuffer::new(), false), move |(buffer, done), item| {
                        let format = format_for_frames.clone();
                        let wrap = wrap_frames.clone();
                        let out: Vec<Result<GaiseSpeechStreamResponse, BoxErr>> = match item {
                            Err(e) => vec![Err(e)],
                            Ok(b) if b.is_empty() && !*done => {
                                // End-of-stream sentinel: flush the tail.
                                *done = true;
                                buffer
                                    .finish()
                                    .into_iter()
                                    .flat_map(|line| parse_line(&line, &format, &wrap))
                                    .collect()
                            }
                            Ok(b) => buffer
                                .push(&b)
                                .into_iter()
                                .flat_map(|line| parse_line(&line, &format, &wrap))
                                .collect(),
                        };
                        futures_util::future::ready(Some(futures_util::stream::iter(out)))
                    })
                    .flatten(),
            )
        } else {
            let wrap_audio = wrap.clone();
            Box::pin(bytes.map(move |b| {
                b.map_err(BoxErr::from).map(|b| {
                    wrap_audio(GaiseSpeechChunk::Audio {
                        data: b.to_vec(),
                        format: mime.clone(),
                        sample_rate: Some(rate),
                    })
                })
            }))
        };

        let usage_tail =
            futures_util::stream::once(async move { Ok(wrap(GaiseSpeechChunk::Usage(usage))) });
        Ok(Box::pin(audio_stream.chain(usage_tail)))
    }
}

fn parse_line(
    line: &str,
    format: &ElevenLabsOutputFormat,
    wrap: &(impl Fn(GaiseSpeechChunk) -> GaiseSpeechStreamResponse + Clone),
) -> Vec<Result<GaiseSpeechStreamResponse, BoxErr>> {
    match parse_timestamp_line(line) {
        Ok(None) => Vec::new(),
        Ok(Some(frame)) => match timestamp_frame_to_chunks(frame, format) {
            Ok(chunks) => chunks.into_iter().map(|c| Ok(wrap(c))).collect(),
            Err(e) => vec![Err(e)],
        },
        Err(e) => vec![Err(format!("invalid ElevenLabs stream frame: {e}").into())],
    }
}

#[async_trait]
impl GaiseClient for GaiseClientElevenLabs {
    async fn instruct_stream(
        &self,
        _request: &GaiseInstructRequest,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<GaiseInstructStreamResponse, BoxErr>> + Send>>,
        BoxErr,
    > {
        Err("ElevenLabs is a speech provider; it has no chat surface. Use speech/speech_stream or live_connect.".into())
    }

    async fn instruct(
        &self,
        _request: &GaiseInstructRequest,
    ) -> Result<GaiseInstructResponse, BoxErr> {
        Err("ElevenLabs is a speech provider; it has no chat surface. Use speech/speech_stream or live_connect.".into())
    }

    async fn embeddings(
        &self,
        _request: &GaiseEmbeddingsRequest,
    ) -> Result<GaiseEmbeddingsResponse, BoxErr> {
        Err("ElevenLabs does not offer text embeddings".into())
    }

    async fn list_models(
        &self,
        request: &GaiseListModelsRequest,
    ) -> Result<GaiseListModelsResponse, BoxErr> {
        let models = self.list_models_raw().await?;
        let mut response = GaiseListModelsResponse::from_models(
            models
                .iter()
                .map(|m| map_elevenlabs_model(m, request.include_raw))
                .collect(),
        );
        response.retain_operation(request.operation);
        Ok(response)
    }
}

#[cfg(feature = "live")]
mod live {
    use super::*;
    use futures_util::SinkExt;
    use gaise_core::GaiseLiveClient;
    use gaise_core::contracts::{
        GaiseLiveConfig, GaiseLiveEvent, GaiseLiveInput, GaiseLiveSession,
    };
    use tokio::sync::mpsc;
    use tokio_tungstenite::tungstenite::Message;

    /// Realtime PCM rate used for live sessions (matches OpenAI Realtime).
    const LIVE_OUTPUT_FORMAT: &str = "pcm_24000";
    const LIVE_SAMPLE_RATE: u32 = 24_000;

    fn ws_base(api_url: &str) -> String {
        api_url
            .replace("https://", "wss://")
            .replace("http://", "ws://")
    }

    /// Build the stream-input or text-to-dialogue URL for a live config.
    pub fn live_url(api_url: &str, model: &str, voice: &str) -> String {
        let base = ws_base(api_url);
        if model_uses_dialogue_websocket(model) {
            format!(
                "{base}/v1/text-to-dialogue/stream-input?model_id={model}&output_format={LIVE_OUTPUT_FORMAT}&sync_alignment=true"
            )
        } else {
            format!(
                "{base}/v1/text-to-speech/{voice}/stream-input?model_id={model}&output_format={LIVE_OUTPUT_FORMAT}&sync_alignment=true&inactivity_timeout=180"
            )
        }
    }

    fn alignment_text(chars: &[String]) -> String {
        chars.concat()
    }

    #[async_trait]
    impl GaiseLiveClient for GaiseClientElevenLabs {
        async fn live_connect(&self, config: &GaiseLiveConfig) -> Result<GaiseLiveSession, BoxErr> {
            let voice = config
                .voice
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .ok_or("ElevenLabs live sessions require `voice` (a voice id)")?
                .to_string();
            let model = config.model.clone();
            let dialogue = model_uses_dialogue_websocket(&model);
            let url = live_url(&self.api_url, &model, &voice);

            let request = http::Request::builder()
                .uri(&url)
                .header("xi-api-key", self.api_key())
                .header("Sec-WebSocket-Version", "13")
                .header(
                    "Sec-WebSocket-Key",
                    tungstenite::handshake::client::generate_key(),
                )
                .header("Connection", "Upgrade")
                .header("Upgrade", "websocket")
                .header(
                    "Host",
                    http::Uri::try_from(&url)?
                        .host()
                        .unwrap_or("api.elevenlabs.io"),
                )
                .body(())?;
            let (ws_stream, _) = tokio_tungstenite::connect_async(request).await?;
            let (mut ws_sink, mut ws_source) = ws_stream.split();

            // Initial frame.
            let init = if dialogue {
                serde_json::to_string(&DialogueInit {
                    voices: vec![voice.clone()],
                    voice_settings: None,
                })?
            } else {
                serde_json::to_string(&StreamInputInit {
                    text: " ".to_string(),
                    voice_settings: None,
                    generation_config: Some(StreamInputGenerationConfig {
                        chunk_length_schedule: vec![120, 160, 250, 290],
                    }),
                })?
            };
            ws_sink.send(Message::Text(init.into())).await?;

            let (input_tx, mut input_rx) = mpsc::channel::<GaiseLiveInput>(256);
            let (event_tx, event_rx) = mpsc::channel::<Result<GaiseLiveEvent, BoxErr>>(256);
            let _ = event_tx
                .send(Ok(GaiseLiveEvent::SessionStarted {
                    session_id: format!("elevenlabs-{}", voice),
                    model: model.clone(),
                }))
                .await;

            // Send loop.
            let send_events = event_tx.clone();
            let send_voice = voice.clone();
            tokio::spawn(async move {
                while let Some(input) = input_rx.recv().await {
                    let frame: Option<String> = match input {
                        GaiseLiveInput::Text { text } => {
                            if dialogue {
                                serde_json::to_string(&DialogueInputs {
                                    inputs: vec![DialogueInputItem {
                                        text,
                                        voice_id: send_voice.clone(),
                                    }],
                                })
                                .ok()
                            } else {
                                let text = if text.ends_with(' ') {
                                    text
                                } else {
                                    format!("{text} ")
                                };
                                serde_json::to_string(&StreamInputText { text, flush: None }).ok()
                            }
                        }
                        GaiseLiveInput::AudioStreamEnd | GaiseLiveInput::ActivityEnd => {
                            if dialogue {
                                Some(r#"{"flush":true}"#.to_string())
                            } else {
                                serde_json::to_string(&StreamInputText {
                                    text: " ".to_string(),
                                    flush: Some(true),
                                })
                                .ok()
                            }
                        }
                        GaiseLiveInput::Close => {
                            let frame = if dialogue {
                                r#"{"close_socket":true}"#.to_string()
                            } else {
                                r#"{"text":""}"#.to_string()
                            };
                            let _ = ws_sink.send(Message::Text(frame.into())).await;
                            break;
                        }
                        GaiseLiveInput::ActivityStart => None,
                        GaiseLiveInput::Audio { .. } => {
                            let _ = send_events
                                .send(Ok(GaiseLiveEvent::Error {
                                    message: "ElevenLabs text-to-speech sessions accept text input only; use speech-to-text or an Agents session for audio input".into(),
                                }))
                                .await;
                            None
                        }
                        GaiseLiveInput::Image { .. } => {
                            let _ = send_events
                                .send(Ok(GaiseLiveEvent::Error {
                                    message: "ElevenLabs sessions do not accept image input".into(),
                                }))
                                .await;
                            None
                        }
                        GaiseLiveInput::ToolResponse { .. } => {
                            let _ = send_events
                                .send(Ok(GaiseLiveEvent::Error {
                                    message: "ElevenLabs text-to-speech sessions have no tools"
                                        .into(),
                                }))
                                .await;
                            None
                        }
                        GaiseLiveInput::ClearAudio | GaiseLiveInput::CancelResponse => {
                            let _ = send_events
                                .send(Ok(GaiseLiveEvent::Error {
                                    message: "ElevenLabs does not support clearing or cancelling an in-flight generation".into(),
                                }))
                                .await;
                            None
                        }
                    };
                    if let Some(frame) = frame
                        && let Err(e) = ws_sink.send(Message::Text(frame.into())).await
                    {
                        let _ = send_events
                            .send(Err(format!("ElevenLabs websocket send failed: {e}").into()))
                            .await;
                        break;
                    }
                }
            });

            // Receive loop.
            let recv_events = event_tx;
            tokio::spawn(async move {
                while let Some(msg) = ws_source.next().await {
                    let text = match msg {
                        Ok(Message::Text(t)) => t.to_string(),
                        Ok(Message::Binary(b)) => String::from_utf8_lossy(&b).to_string(),
                        Ok(Message::Close(frame)) => {
                            if let Some(frame) = frame
                                && frame.code != tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal
                            {
                                let _ = recv_events
                                    .send(Ok(GaiseLiveEvent::Error {
                                        message: format!("ElevenLabs closed the session: {} ({})", frame.reason, frame.code),
                                    }))
                                    .await;
                            }
                            break;
                        }
                        Ok(_) => continue,
                        Err(e) => {
                            let _ = recv_events
                                .send(Err(format!("ElevenLabs websocket error: {e}").into()))
                                .await;
                            break;
                        }
                    };
                    let events = if dialogue {
                        match serde_json::from_str::<DialogueEvent>(&text) {
                            Ok(event) => dialogue_events(event),
                            Err(e) => {
                                vec![Err(format!("invalid ElevenLabs dialogue frame: {e}").into())]
                            }
                        }
                    } else {
                        match serde_json::from_str::<StreamInputEvent>(&text) {
                            Ok(event) => stream_input_events(event),
                            Err(e) => vec![Err(format!(
                                "invalid ElevenLabs stream-input frame: {e}"
                            )
                            .into())],
                        }
                    };
                    for event in events {
                        if recv_events.send(event).await.is_err() {
                            return;
                        }
                    }
                }
                let _ = recv_events.send(Ok(GaiseLiveEvent::SessionEnded)).await;
            });

            Ok(GaiseLiveSession {
                tx: input_tx,
                rx: Box::pin(tokio_stream::wrappers::ReceiverStream::new(event_rx)),
            })
        }
    }

    /// Map one `stream-input` server frame to live events.
    pub fn stream_input_events(event: StreamInputEvent) -> Vec<Result<GaiseLiveEvent, BoxErr>> {
        let mut out = Vec::new();
        if let Some(message) = event.message.or(event.error) {
            out.push(Ok(GaiseLiveEvent::Error { message }));
            return out;
        }
        if let Some(b64) = event.audio.filter(|a| !a.is_empty()) {
            match base64::prelude::BASE64_STANDARD.decode(b64.as_bytes()) {
                Ok(data) => out.push(Ok(GaiseLiveEvent::Audio {
                    data,
                    sample_rate: LIVE_SAMPLE_RATE,
                })),
                Err(e) => out.push(Err(format!("invalid ElevenLabs audio payload: {e}").into())),
            }
        }
        if let Some(alignment) = event.alignment.or(event.normalized_alignment)
            && !alignment.chars.is_empty()
        {
            out.push(Ok(GaiseLiveEvent::Transcript {
                role: "assistant".to_string(),
                text: alignment_text(&alignment.chars),
            }));
        }
        if event.is_final == Some(true) {
            out.push(Ok(GaiseLiveEvent::TurnComplete));
        }
        out
    }

    /// Map one text-to-dialogue server frame to live events.
    pub fn dialogue_events(event: DialogueEvent) -> Vec<Result<GaiseLiveEvent, BoxErr>> {
        let mut out = Vec::new();
        if let Some(message) = event.message.or(event.error) {
            out.push(Ok(GaiseLiveEvent::Error { message }));
            return out;
        }
        if let Some(b64) = event.audio.filter(|a| !a.is_empty()) {
            match base64::prelude::BASE64_STANDARD.decode(b64.as_bytes()) {
                Ok(data) => out.push(Ok(GaiseLiveEvent::Audio {
                    data,
                    sample_rate: LIVE_SAMPLE_RATE,
                })),
                Err(e) => out.push(Err(format!("invalid ElevenLabs audio payload: {e}").into())),
            }
        }
        if let Some(alignment) = event.alignment
            && !alignment.chars.is_empty()
        {
            out.push(Ok(GaiseLiveEvent::Transcript {
                role: "assistant".to_string(),
                text: alignment_text(&alignment.chars),
            }));
        }
        if event.is_final_audio_for_turn == Some(true) || event.is_final == Some(true) {
            out.push(Ok(GaiseLiveEvent::TurnComplete));
        }
        out
    }
}

#[cfg(feature = "live")]
pub use live::{dialogue_events, live_url, stream_input_events};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_buffer_splits_across_chunk_boundaries() {
        let mut buffer = LineBuffer::new();
        assert!(buffer.push(b"{\"audio_base64\":\"AA").is_empty());
        let lines = buffer.push(b"==\"}\r\n{\"audio_base64\":null}\n{\"aud");
        assert_eq!(
            lines,
            vec!["{\"audio_base64\":\"AA==\"}", "{\"audio_base64\":null}"]
        );
        assert_eq!(buffer.finish().as_deref(), Some("{\"aud"));
        assert!(buffer.finish().is_none());
    }

    #[test]
    fn timestamp_frames_become_audio_and_alignment_chunks() {
        let frame = ElevenLabsTimestampedAudio {
            audio_base64: Some("AAEC".into()),
            alignment: Some(ElevenLabsAlignment {
                characters: vec!["H".into(), "i".into()],
                character_start_times_seconds: vec![0.0, 0.1],
                character_end_times_seconds: vec![0.1, 0.2],
            }),
            normalized_alignment: None,
        };
        let format = resolve_output_format(Some("pcm_24000"), None).unwrap();
        let chunks = timestamp_frame_to_chunks(frame, &format).unwrap();
        assert_eq!(chunks.len(), 2);
        match &chunks[0] {
            GaiseSpeechChunk::Audio {
                data,
                format,
                sample_rate,
            } => {
                assert_eq!(data, &[0, 1, 2]);
                assert_eq!(format, "audio/pcm");
                assert_eq!(*sample_rate, Some(24_000));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(
            matches!(&chunks[1], GaiseSpeechChunk::Alignment(a) if a.characters == vec!["H", "i"])
        );
    }

    #[test]
    fn voice_is_required() {
        let request = GaiseSpeechRequest {
            model: "eleven_flash_v2_5".into(),
            input: "hi".into(),
            ..Default::default()
        };
        assert!(GaiseClientElevenLabs::voice_for(&request).is_err());
        let client = GaiseClientElevenLabs::new("".into(), "key".into());
        assert_eq!(client.api_url(), DEFAULT_API_URL);
        let client = GaiseClientElevenLabs::new(
            "https://api.eu.residency.elevenlabs.io/".into(),
            "key".into(),
        );
        assert_eq!(client.api_url(), "https://api.eu.residency.elevenlabs.io");
    }
}
