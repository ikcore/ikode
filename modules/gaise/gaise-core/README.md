# gaise

[![crates.io](https://img.shields.io/crates/v/gaise.svg)](https://crates.io/crates/gaise)
[![docs.rs](https://docs.rs/gaise/badge.svg)](https://docs.rs/gaise)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)

Core trait and contracts for **GAISe** (Generative AI Service) — a unified Rust abstraction across GenAI providers.

Write your application once, swap between OpenAI, Anthropic, Gemini, Vertex AI, Bedrock, and Ollama with a single model string change.

## The `GaiseClient` Trait

```rust
#[async_trait]
pub trait GaiseClient: Send + Sync {
    async fn instruct(&self, request: &GaiseInstructRequest)
        -> Result<GaiseInstructResponse, Box<dyn Error + Send + Sync>>;

    async fn instruct_stream(&self, request: &GaiseInstructRequest)
        -> Result<Pin<Box<dyn Stream<Item = Result<GaiseInstructStreamResponse, ...>> + Send>>, ...>;

    async fn embeddings(&self, request: &GaiseEmbeddingsRequest)
        -> Result<GaiseEmbeddingsResponse, Box<dyn Error + Send + Sync>>;

    // Default implementation reports "not supported" so custom clients keep compiling.
    async fn list_models(&self, request: &GaiseListModelsRequest)
        -> Result<GaiseListModelsResponse, Box<dyn Error + Send + Sync>>;
}
```

Every provider crate implements this trait. Your application depends on `gaise` for the contracts and picks whichever provider crates it needs.

### Model discovery and the bundled registry

`list_models` returns `GaiseModel` records: routable id, lifecycle status and dates, token limits, and a `GaiseModelCapabilities` block with input/output modalities, the GAISe operations that can drive the model (`instruct`, `instruct_stream`, `embeddings`, `live`), tri-state `tools` / `reasoning` / `structured_output` flags, and a `sources` list saying whether each claim came from the provider API, the registry, or a name heuristic. Provider model APIs differ widely in what they report, so `unknown` is a first-class answer.

`gaise_core::registry` compiles `model-registry.toml` into the crate and exposes `ModelRegistry::bundled()`, `find(provider, id)` (exact, alias, `*` glob, dated-snapshot, and Bedrock profile-prefix matching), and `enrich(&mut GaiseModel)`, which fills in what a provider left unknown without overriding provider facts.

### `GaiseLiveClient` Trait (real-time sessions)

```rust
#[async_trait]
pub trait GaiseLiveClient: Send + Sync {
    async fn live_connect(&self, config: &GaiseLiveConfig)
        -> Result<GaiseLiveSession, Box<dyn Error + Send + Sync>>;
}
```

Returns a `GaiseLiveSession` with a `tx` channel (send audio/text/tool responses) and an `rx` stream (receive audio/text/transcripts/tool calls). Implemented by `gaise-provider-openai` and `gaise-provider-gemini` when their `live` feature is enabled.

## Key Types

| Type | Purpose |
|------|---------|
| `GaiseInstructRequest` | Input: model, messages, tools, generation config |
| `GaiseInstructResponse` | Output: messages, usage |
| `GaiseInstructStreamResponse` | Streaming chunk: text delta, tool call delta, or usage |
| `GaiseEmbeddingsRequest/Response` | Embedding vectors |
| `GaiseContent` | Enum: `Text`, `Image`, `Audio`, `File`, `Parts` |
| `GaiseMessage` | Role + content + optional tool calls |
| `GaiseGenerationConfig` | Temperature, max_tokens, thinking_effort, thinking_tokens |
| `GaiseTool` | Function calling definition with JSON Schema params |
| `OneOrMany<T>` | Flexible single-or-array wrapper |
| `GaiseStreamAccumulator` | Collects stream chunks into a complete message |
| `GaiseLiveConfig` | Live session config: model, voice, modalities, VAD, transcription, tools |
| `GaiseLiveSession` | Active session: `tx` (send inputs) + `rx` (receive events) |
| `GaiseLiveEvent` | Server event: `Audio`, `Text`, `Transcript`, `ToolCall`, `TurnComplete`, etc. |
| `GaiseLiveInput` | Client input: `Audio`, `Text`, `ToolResponse`, `Close` |

## Quick Start

```rust
use gaise_core::GaiseClient;
use gaise_core::contracts::*;

let request = GaiseInstructRequest {
    model: "my-model".to_string(),
    input: OneOrMany::One(GaiseMessage {
        role: "user".to_string(),
        content: Some(OneOrMany::One(GaiseContent::Text {
            text: "Hello!".to_string(),
        })),
        ..Default::default()
    }),
    ..Default::default()
};

// Pass `request` to any GaiseClient implementation
let response = client.instruct(&request).await?;
```

## Provider Crates

| Crate | Provider |
|-------|----------|
| [`gaise-provider-openai`](https://crates.io/crates/gaise-provider-openai) | OpenAI (GPT-4o, GPT-5, o3, o4-mini) |
| [`gaise-provider-anthropic`](https://crates.io/crates/gaise-provider-anthropic) | Anthropic (Claude 4.x, extended thinking) |
| [`gaise-provider-gemini`](https://crates.io/crates/gaise-provider-gemini) | Google Gemini (v1beta API) |
| [`gaise-provider-vertexai`](https://crates.io/crates/gaise-provider-vertexai) | Google Vertex AI |
| [`gaise-provider-bedrock`](https://crates.io/crates/gaise-provider-bedrock) | AWS Bedrock |
| [`gaise-provider-ollama`](https://crates.io/crates/gaise-provider-ollama) | Ollama (local) |
| [`gaise-client`](https://crates.io/crates/gaise-client) | Router — `"provider::model"` string routing |
| [`gaise-api`](https://crates.io/crates/gaise-api) | Axum HTTP server with SSE streaming |

## Reasoning / Thinking

GAISe standardises reasoning across providers via `GaiseGenerationConfig`:

```rust
generation_config: Some(GaiseGenerationConfig {
    thinking_effort: Some("high".to_string()),  // low, medium, high
    thinking_tokens: Some(10000),                // explicit budget (Anthropic, Gemini 2.5)
    max_tokens: Some(32000),
    ..Default::default()
}),
```

| GAISe field | OpenAI | Anthropic | Gemini |
|---|---|---|---|
| `thinking_effort` | `reasoning_effort` | `thinking.type` | `thinkingConfig.thinkingLevel` |
| `thinking_tokens` | N/A | `thinking.budget_tokens` | `thinkingConfig.thinkingBudget` |
| `max_tokens` | `max_completion_tokens` | `max_tokens` | `maxOutputTokens` |

## License

AGPL-3.0-only
