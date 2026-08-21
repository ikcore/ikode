# GAISe (Generative AI Service)

[![crates.io](https://img.shields.io/crates/v/gaise.svg)](https://crates.io/crates/gaise)
[![docs.rs](https://docs.rs/gaise/badge.svg)](https://docs.rs/gaise)
[![Rust](https://img.shields.io/badge/rust-1.91%2B-orange.svg)](https://www.rust-lang.org)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)

GAISe is a Rust abstraction over OpenAI, Anthropic, Google Gemini, Vertex AI, Amazon Bedrock, and Ollama. It provides one request/response contract for text, reasoning, tools, images, audio, files, embeddings, and streaming.

Written by Ian Knowles. Project page: [BadAI](https://badai.company/open-source/gaise).

## What is included

- A shared `GaiseClient` trait with `instruct`, `instruct_stream`, and `embeddings`.
- Router-style model names such as `gemini::gemini-3.6-flash`.
- Ordered multimodal content: text, reasoning summaries, images, audio, files, and nested parts.
- Tool calling with nested JSON schemas and provider thought-signature round trips.
- Model-aware reasoning controls, prompt caching, retry handling, modality-aware input/output/total usage reporting, and robust stream framing.
- Generated-image output for Gemini, Vertex AI, and supported Bedrock response shapes.
- Bidirectional OpenAI Realtime and Gemini Live transports.
- Hermetic mapping and parser tests; live or credentialed checks are opt-in and ignored by default.

## Provider coverage

| Provider | Main API surface | Current model examples |
|---|---|---|
| OpenAI | Chat Completions, Embeddings, Realtime | GPT-5.6 family, GPT-5.5/5.4, text-embedding-3, Realtime 2.1 |
| Anthropic | Messages | Claude Fable 5, Opus 4.8, Sonnet 5, Haiku 4.5 |
| Gemini | generateContent, Embeddings, Live | Gemini 3.6 Flash, 3.5 Flash/Lite, 3.1 Pro/image, 3.1 Flash Live |
| Vertex AI | generateContent, Embeddings | Google Cloud Gemini catalog, including image-output models |
| Bedrock | Converse, ConverseStream, InvokeModel | Claude, Amazon Nova, Titan and Cohere embeddings |
| Ollama | Chat and Embeddings | Any installed compatible tag; vision, thinking, and tools are model-dependent |

GAISe also wraps ElevenLabs for text-to-speech and realtime voice (`POST /v1/speech*`, `GET /v1/live`). Model availability changes quickly. [`gaise-core/model-registry.toml`](gaise-core/model-registry.toml) records the 2026-08-20 audit, including provider-specific retirement dates, and is bundled into the `gaise` crate so `list_models` can enrich what each provider's model API leaves out. The [wiki](wiki/README.md) is the complete developer guide: [HTTP API](wiki/api.md), [Rust SDK](wiki/sdk.md), [capabilities](wiki/capabilities.md), the full [model catalog](wiki/models.md), [flow diagrams](wiki/flows.md), [examples](wiki/examples.md), and one page per vendor ([OpenAI](wiki/vendor-openai.md), [Anthropic](wiki/vendor-anthropic.md), [Gemini](wiki/vendor-gemini.md), [Vertex AI](wiki/vendor-vertexai.md), [Bedrock](wiki/vendor-bedrock.md), [Ollama](wiki/vendor-ollama.md), [ElevenLabs](wiki/vendor-elevenlabs.md)).

## Installation

```toml
[dependencies]
gaise = "0.2"
gaise-client = "0.2"
tokio = { version = "1", features = ["full"] }

# Or depend on individual adapters:
# gaise-provider-openai = "0.2"
# gaise-provider-anthropic = "0.2"
# gaise-provider-gemini = "0.2"
# gaise-provider-vertexai = "0.2"
# gaise-provider-bedrock = "0.2"
# gaise-provider-ollama = "0.2"
```

## Router quick start

```rust
use gaise_client::{GaiseClientConfig, GaiseClientService};
use gaise_core::contracts::*;
use gaise_core::GaiseClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let service = GaiseClientService::new(GaiseClientConfig {
        gemini_api_key: Some(std::env::var("GEMINI_API_KEY")?),
        ..Default::default()
    });

    let request = GaiseInstructRequest {
        model: "gemini::gemini-3.6-flash".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Explain why the sky appears blue.".to_string(),
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let response = service.instruct(&request).await?;
    println!("{:?}", response.output);
    Ok(())
}
```

The router strips the provider prefix before calling the adapter. Direct provider clients therefore receive `gemini-3.6-flash`, while `GaiseClientService` receives `gemini::gemini-3.6-flash`.

## Multimodal input

`GaiseContent::Parts` can be nested; adapters flatten it while preserving order. Image and audio `format` values accept a MIME type or common shorthand such as `png`, `jpeg`, `wav`, or `mp3`.

```rust
let message = GaiseMessage {
    role: "user".to_string(),
    content: Some(OneOrMany::Many(vec![
        GaiseContent::Text {
            text: "Compare this image with the attached report.".to_string(),
        },
        GaiseContent::Image {
            data: std::fs::read("photo.png")?,
            format: Some("image/png".to_string()),
        },
        GaiseContent::File {
            data: std::fs::read("report.pdf")?,
            name: Some("report.pdf".to_string()),
        },
    ])),
    ..Default::default()
};
```

Document support is provider-specific. Anthropic, Gemini, Vertex AI, and Bedrock can receive supported document blocks. Ollama falls back to UTF-8 text. The current OpenAI adapter targets Chat Completions, whose message content does not support Responses-style `input_file`; UTF-8 files are tagged as text and binary files become an explicit unsupported-document marker instead of an invalid wire block.

## Image generation and image editing

Gemini and Vertex image-output models use the common response modality and image controls:

```rust
let request = GaiseInstructRequest {
    model: "gemini::gemini-3.1-flash-image".to_string(),
    generation_config: Some(GaiseGenerationConfig {
        response_modalities: Some(vec!["TEXT".to_string(), "IMAGE".to_string()]),
        image_config: Some(GaiseImageConfig {
            aspect_ratio: Some("16:9".to_string()),
            image_size: Some("2K".to_string()),
        }),
        ..Default::default()
    }),
    input: OneOrMany::One(GaiseMessage {
        role: "user".to_string(),
        content: Some(OneOrMany::One(GaiseContent::Text {
            text: "Create a watercolor landscape.".to_string(),
        })),
        ..Default::default()
    }),
    ..Default::default()
};
```

Returned media appears as `GaiseContent::Image`, `Audio`, or `File` in the normal response. During streaming it arrives as `GaiseStreamChunk::Content`. OpenAI image generation requires its Images API or the Responses image-generation tool and is not represented by the current OpenAI Chat Completions adapter.

For OpenAI image input, set `input_image_detail` to `low`, `high`, `auto`, or (on models that support it) `original`.

## Reasoning and thought summaries

```rust
let config = GaiseGenerationConfig {
    thinking_effort: Some("high".to_string()),
    thinking_tokens: Some(8_192),
    include_thoughts: Some(true),
    max_tokens: Some(32_000),
    ..Default::default()
};
```

| Common field | OpenAI | Anthropic | Gemini / Vertex | Bedrock | Ollama |
|---|---|---|---|---|---|
| `thinking_effort` | `reasoning_effort` | `output_config.effort` plus model-aware thinking | Gemini 3 thinking level | Claude effort or Nova reasoning config | Boolean thinking or GPT-OSS level |
| `thinking_tokens` | Included in completion budget | Manual `budget_tokens` where supported | Gemini 2.5 thinking budget; mapped to a level on 3.x | Provider-specific reasoning budget | Enables thinking where supported |
| `include_thoughts` | Reasoning summary where exposed | `thinking.display` | `includeThoughts` | Returned reasoning content where exposed | Returned `thinking` field |
| `max_tokens` | `max_completion_tokens` | `max_tokens` | `maxOutputTokens` | `maxTokens` | `num_predict` |

Providers may return only a summary or opaque signature, not private chain-of-thought. GAISe represents returned summaries as `GaiseContent::Reasoning { text, signature }`; signatures should be preserved when replaying tool calls. Safety-redacted Anthropic/Bedrock reasoning is represented as `GaiseContent::RedactedReasoning { data }` and must be replayed byte-for-byte rather than displayed.

## Streaming

```rust
use futures_util::StreamExt;

let mut stream = service.instruct_stream(&request).await?;
while let Some(item) = stream.next().await {
    match item?.chunk {
        GaiseStreamChunk::Text(text) => print!("{text}"),
        GaiseStreamChunk::Content(content) => println!("media/reasoning: {content:?}"),
        GaiseStreamChunk::ToolCall { name, arguments, .. } => {
            println!("tool: {name:?} {arguments:?}")
        }
        GaiseStreamChunk::Usage(usage) => println!("usage: {usage:?}"),
    }
}
```

`GaiseStreamAccumulator` converts a stream into one ordered `GaiseMessage`, coalescing adjacent text/reasoning deltas while preserving generated media and tool calls.

## Usage reporting

`GaiseUsage` separates `input`, `output`, and request-wide `total` counters. Where a provider supplies modality details, the corresponding map includes `text_tokens`, `image_tokens`, `audio_tokens`, and `reasoning_tokens`; cache and tool counters are retained as well. OpenAI Chat, Anthropic, Bedrock, and Ollama do not expose every modality split, so GAISe returns their honest aggregate and available details rather than guessing.

Counters in a map can overlap: for example, an aggregate prompt count includes its modality and cache subsets. Do not sum every value. Stream usage events are cumulative snapshots; `GaiseStreamAccumulator` replaces repeated counters so repeated final metadata is not double-counted.

## Embeddings

```rust
let request = GaiseEmbeddingsRequest {
    model: "gemini::gemini-embedding-2".to_string(),
    input: OneOrMany::One("Text to embed".to_string()),
    ..Default::default()
};

let response = service.embeddings(&request).await?;
println!("dimensions: {}", response.output[0].len());
```

OpenAI text-embedding-3, Gemini/Vertex embeddings, Bedrock Titan/Cohere, and Ollama embeddings are supported. The common embedding request is currently text-oriented even where a provider offers multimodal embeddings.

## Tool calling

Tool parameters support nested `object` and `array` schemas through recursive `properties` and `items`. A returned tool call contains its provider ID, function name, JSON arguments, and an optional thought signature. When returning a result, set both `tool_call_id` and `tool_name`; the name is optional for providers that only require an ID but is required by current Gemini and Vertex function responses.

```rust
use std::collections::BTreeMap;

let mut properties = BTreeMap::new();
properties.insert(
    "location".to_string(),
    GaiseToolParameter {
        r#type: Some("string".to_string()),
        description: Some("City and country".to_string()),
        ..Default::default()
    },
);

let tool = GaiseTool {
    name: "get_weather".to_string(),
    description: Some("Get current weather".to_string()),
    parameters: Some(GaiseToolParameter {
        r#type: Some("object".to_string()),
        properties: Some(properties),
        required: Some(vec!["location".to_string()]),
        ..Default::default()
    }),
};
```

## HTTP server

```powershell
$env:GEMINI_API_KEY = "..."
cargo run -p gaise-api
```

The Axum service exposes:

- `POST /v1/instruct`
- `POST /v1/instruct/stream` (SSE)
- `POST /v1/embeddings`

See [`API_DOCUMENTATION.md`](API_DOCUMENTATION.md) for the wire format.

## Development and tests

```powershell
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

The default suite does not call provider APIs. Tests requiring credentials, a running Ollama daemon, or external provider initialization are marked `#[ignore]`; do not use `--ignored` unless you intentionally want integration traffic.

## Workspace crates

| Crate | Purpose |
|---|---|
| `gaise` (`gaise-core/`) | Shared trait, contracts, stream accumulator, logging |
| `gaise-client` | Feature-gated provider router |
| `gaise-provider-openai` | OpenAI Chat Completions, Embeddings, Realtime |
| `gaise-provider-anthropic` | Anthropic Messages |
| `gaise-provider-gemini` | Gemini API and Gemini Live |
| `gaise-provider-vertexai` | Vertex AI with service-account authentication |
| `gaise-provider-bedrock` | Bedrock Converse/Invoke and embeddings |
| `gaise-provider-ollama` | Local Ollama chat and embeddings |
| `gaise-api` | Axum JSON/SSE/WebSocket server |
| `gaise-chatbot` | Example CLI chatbot |

## Environment variables

| Variable | Consumer |
|---|---|
| `OPENAI_API_KEY`, `OPENAI_API_URL` | OpenAI |
| `ANTHROPIC_API_KEY`, `ANTHROPIC_API_URL` | Anthropic |
| `GEMINI_API_KEY`, `GEMINI_API_URL` | Gemini |
| `VERTEXAI_SA_PATH`, `VERTEXAI_API_URL` | Vertex AI |
| `BEDROCK_REGION` and normal AWS credential variables | Bedrock |
| `OLLAMA_URL` | Ollama |
| `GAISE_PORT` | API server; defaults to 3000 |

## License

AGPLv3
