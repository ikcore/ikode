# gaise-provider-vertexai

[![crates.io](https://img.shields.io/crates/v/gaise-provider-vertexai.svg)](https://crates.io/crates/gaise-provider-vertexai)
[![docs.rs](https://docs.rs/gaise-provider-vertexai/badge.svg)](https://docs.rs/gaise-provider-vertexai)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)

Google Vertex AI provider for [GAISe](https://crates.io/crates/gaise) — implements the `GaiseClient` trait for Gemini models via Vertex AI endpoints with service account authentication.

## Supported Features

- Text and multimodal (image, audio, file/PDF) instruct requests
- Streaming via SSE (`streamGenerateContent`)
- Embeddings via Vertex AI prediction endpoint
- Multiple system messages combined into the top-level system instruction
- Function calling with call IDs, names, and thought signatures preserved
- Gemini thinking configuration and generated-image/media output
- Response modalities, current `responseFormat.image`, and input media resolution
- Usage with prompt/cache/tool/reasoning totals, input/output modality details, and embedding metadata
- Service account JWT authentication with auto-refresh

## Usage

```rust
use gaise_core::GaiseClient;
use gaise_core::contracts::*;
use gaise_provider_vertexai::vertexai_client::GaiseClientVertexAI;
use gaise_provider_vertexai::contracts::ServiceAccount;

let sa: ServiceAccount = serde_json::from_str(&std::fs::read_to_string("sa.json")?)?;
let client = GaiseClientVertexAI::new(
    &sa,
    "https://us-central1-aiplatform.googleapis.com/v1/projects/PROJECT/locations/LOCATION/publishers/google/models/{{MODEL}}".to_string(),
).await;

let request = GaiseInstructRequest {
    model: "gemini-3.6-flash".to_string(),
    input: OneOrMany::One(GaiseMessage {
        role: "user".to_string(),
        content: Some(OneOrMany::One(GaiseContent::Text {
            text: "Hello from Vertex AI!".to_string(),
        })),
        ..Default::default()
    }),
    ..Default::default()
};

let response = client.instruct(&request).await?;
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `VERTEXAI_API_URL` | Vertex AI endpoint URL (contains `{{MODEL}}` placeholder) |
| `VERTEXAI_SA_PATH` | Path to service account JSON file |

## Part of [GAISe](https://github.com/ikcore/gaise)

License: AGPL-3.0-only
