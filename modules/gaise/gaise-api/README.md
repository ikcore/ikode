# gaise-api

[![crates.io](https://img.shields.io/crates/v/gaise-api.svg)](https://crates.io/crates/gaise-api)
[![docs.rs](https://docs.rs/gaise-api/badge.svg)](https://docs.rs/gaise-api)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)

Axum HTTP server for [GAISe](https://crates.io/crates/gaise) — exposes all GenAI providers behind a unified REST API with SSE streaming.

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/v1/instruct` | Non-streaming instruct request |
| `POST` | `/v1/instruct/stream` | Server-Sent Events streaming |
| `POST` | `/v1/embeddings` | Generate embedding vectors |
| `GET` | `/v1/models` | List models across configured providers (`?provider=`, `?operation=`, `?include_details=`, `?include_raw=`) |
| `GET` | `/v1/models/{provider}::{id}` | One model record |
| `GET` | `/v1/live` | WebSocket for real-time audio/text sessions (feature = `live`) |

## Quick Start

```bash
# Set provider keys
export OPENAI_API_KEY="sk-..."
export ANTHROPIC_API_KEY="sk-ant-..."
export GEMINI_API_KEY="AIza..."

# Run the server
cargo run -p gaise-api
# Listening on 0.0.0.0:3000
```

```bash
# Call any provider via the same endpoint
curl -X POST http://localhost:3000/v1/instruct \
  -H "Content-Type: application/json" \
  -d '{
    "model": "openai::gpt-5.6-terra",
    "input": {
      "role": "user",
      "content": {"type": "text", "text": "Hello!"}
    }
  }'
```

Change `"model"` to `"anthropic::claude-sonnet-5"` or `"gemini::gemini-3.6-flash"` — same endpoint, same format.

### Live / Realtime (feature = "live")

Enable with `cargo run -p gaise-api --features live`. The `/v1/live` endpoint upgrades to a WebSocket for bidirectional audio + text streaming.

**Protocol:**
1. Client sends a JSON `GaiseLiveConfig` as the first message (model, voice, modalities, tools, etc.)
2. Server connects to the provider and begins forwarding:
   - **Client -> Server:** JSON text frames (`GaiseLiveInput`) or binary frames (raw PCM16 audio at 16kHz)
   - **Server -> Client:** JSON text frames (`GaiseLiveEvent`) or binary frames (PCM audio with 4-byte LE sample rate header)

## Environment Variables

| Variable | Description |
|----------|-------------|
| `GAISE_PORT` | Server port (default: `3000`) |
| `OLLAMA_URL` | Ollama API URL |
| `OPENAI_API_URL` / `OPENAI_API_KEY` | OpenAI credentials |
| `ANTHROPIC_API_URL` / `ANTHROPIC_API_KEY` | Anthropic credentials |
| `GEMINI_API_URL` / `GEMINI_API_KEY` | Gemini credentials |
| `VERTEXAI_API_URL` / `VERTEXAI_SA_PATH` | Vertex AI credentials |
| `BEDROCK_REGION` | AWS Bedrock region |

## As a Library

```rust
use std::sync::Arc;
use gaise_api::{create_app, AppState};
use gaise_client::{GaiseClientService, GaiseClientConfig};

let config = GaiseClientConfig { /* ... */ ..Default::default() };
let state = Arc::new(AppState {
    client_service: GaiseClientService::new(config),
});

let app = create_app(state);
// Mount into your own Axum server
```

## Part of [GAISe](https://github.com/ikcore/gaise)

License: AGPL-3.0-only
