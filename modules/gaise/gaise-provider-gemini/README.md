# gaise-provider-gemini

[![crates.io](https://img.shields.io/crates/v/gaise-provider-gemini.svg)](https://crates.io/crates/gaise-provider-gemini)
[![docs.rs](https://docs.rs/gaise-provider-gemini/badge.svg)](https://docs.rs/gaise-provider-gemini)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)

Google Gemini provider for [GAISe](https://crates.io/crates/gaise) — implements the `GaiseClient` trait using the Gemini API v1beta.

## Supported Features

- Text and multimodal (image, audio) instruct via `generateContent`
- Streaming via `streamGenerateContent` (SSE)
- Batch embeddings via `batchEmbedContents`
- System instruction extraction (top-level `systemInstruction`)
- Function calling with provider call IDs, names, and thought signatures preserved
- Thinking config (`thinkingLevel` for 3.x, `thinkingBudget` for 2.5)
- `thoughtSignature` preservation for multi-turn tool conversations
- Generated image/audio/file output, current `responseFormat.image` controls, and input media resolution
- Usage with prompt/cache/tool/reasoning totals and input/output modality details
- Safety settings (all categories default to OFF)
- **Live / Realtime sessions** (feature = `live`) — bidirectional WebSocket audio + text via the Gemini Live API

## Usage

```rust
use gaise_core::GaiseClient;
use gaise_core::contracts::*;
use gaise_provider_gemini::gemini_client::GaiseClientGemini;

let client = GaiseClientGemini::new(
    "https://generativelanguage.googleapis.com/v1beta".to_string(),
    "your-gemini-api-key".to_string(),
);

let request = GaiseInstructRequest {
    model: "gemini-3.6-flash".to_string(),
    input: OneOrMany::One(GaiseMessage {
        role: "user".to_string(),
        content: Some(OneOrMany::One(GaiseContent::Text {
            text: "Hello from GAISe!".to_string(),
        })),
        ..Default::default()
    }),
    ..Default::default()
};

let response = client.instruct(&request).await?;
```

### With Thinking

```rust
let request = GaiseInstructRequest {
    model: "gemini-3.6-flash".to_string(),
    generation_config: Some(GaiseGenerationConfig {
        thinking_effort: Some("high".to_string()),
        max_tokens: Some(32000),
        ..Default::default()
    }),
    // ...
};
```

Maps `thinking_effort` to `thinkingConfig.thinkingLevel` (uppercased) and `thinking_tokens` to `thinkingConfig.thinkingBudget`.

### Embeddings

```rust
let request = GaiseEmbeddingsRequest {
    model: "gemini-embedding-2".to_string(),
    input: OneOrMany::One("Text to embed".to_string()),
    ..Default::default()
};

let response = client.embeddings(&request).await?;
```

### Live / Realtime (feature = "live")

Enable the `live` feature to use the Gemini Live API for bidirectional audio and text streaming:

```toml
[dependencies]
gaise-provider-gemini = { version = "0.2", features = ["live"] }
```

```rust
use gaise_core::GaiseLiveClient;
use gaise_core::contracts::*;
use gaise_provider_gemini::gemini_live_client::GaiseClientGeminiLive;

let client = GaiseClientGeminiLive::new(
    "https://generativelanguage.googleapis.com/v1beta".to_string(),
    "your-gemini-api-key".to_string(),
);

let config = GaiseLiveConfig {
    model: "gemini-3.1-flash-live-preview".to_string(),
    voice: Some("Puck".to_string()),
    modalities: vec![GaiseLiveModality::Audio],
    ..Default::default()
};

let session = client.live_connect(&config).await?;
// session.tx — send audio/text/image frames, activity controls, and tool responses
// session.rx — receive audio/text/transcripts/reasoning/tools/usage
```

## API Endpoints

| Method | URL Pattern |
|--------|-------------|
| Instruct | `POST /models/{model}:generateContent?key={key}` |
| Stream | `POST /models/{model}:streamGenerateContent?alt=sse&key={key}` |
| Embeddings | `POST /models/{model}:batchEmbedContents?key={key}` |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `GEMINI_API_URL` | API base URL (default: `https://generativelanguage.googleapis.com/v1beta`) |
| `GEMINI_API_KEY` | Your Gemini API key |

## Part of [GAISe](https://github.com/ikcore/gaise)

License: AGPL-3.0-only
