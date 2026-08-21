# gaise-client

`gaise-client` is a provider aggregator for the GAISe (Generative AI Service) project. It allows you to use multiple AI providers (OpenAI, Anthropic, Gemini, VertexAI, Ollama, Bedrock) through a single interface, routing requests based on a model naming convention.

## Features

- **Provider Aggregation**: Manage multiple providers in one service.
- **Unified Interface**: Implements the `GaiseClient` trait.
- **Dynamic Routing**: Route requests using the `provider::model` format.
- **Lazy Initialization**: Providers are initialized only when first requested.
- **Feature Flags**: Enable only the providers you need to keep dependencies lean.

## Feature Flags

`gaise-client` uses feature flags to reduce the number of dependencies. By default, all providers are enabled.

- `openai`: Enables the OpenAI provider.
- `vertexai`: Enables the Google VertexAI provider.
- `ollama`: Enables the Ollama provider.
- `bedrock`: Enables the AWS Bedrock provider.
- `anthropic`: Enables the Anthropic Claude provider.
- `gemini`: Enables the Google Gemini provider.
- `live`: Enables real-time WebSocket sessions via `GaiseLiveClient` (currently supports `openai` and `gemini`).

To use only specific providers, disable default features in your `Cargo.toml`:

```toml
[dependencies]
gaise-client = { version = "0.2.0", default-features = false, features = ["openai"] }
```

To enable live/realtime sessions:

```toml
[dependencies]
gaise-client = { version = "0.2.0", features = ["live"] }
```

## Supported Providers

- `openai`
- `vertexai`
- `ollama`
- `bedrock`
- `anthropic`
- `gemini`

## Usage

### Configuration

First, set up the `GaiseClientConfig` with the necessary credentials and URLs. Note that fields in `GaiseClientConfig` are conditionally compiled based on enabled features.

```rust
use gaise_client::{GaiseClientConfig, GaiseClientService};

let config = GaiseClientConfig {
    #[cfg(feature = "openai")]
    openai_api_key: Some("your-openai-key".to_string()),
    #[cfg(feature = "anthropic")]
    anthropic_api_key: Some("your-anthropic-key".to_string()),
    #[cfg(feature = "ollama")]
    ollama_url: Some("http://localhost:11434".to_string()),
    ..Default::default()
};

let service = GaiseClientService::new(config);
```

### Making Requests

Use the `provider::model` format in the `model` field of your requests.

```rust
use gaise_core::contracts::{GaiseInstructRequest, GaiseMessage, GaiseContent, OneOrMany};
use gaise_core::GaiseClient;

let request = GaiseInstructRequest {
    model: "openai::gpt-5.6-terra".to_string(),
    input: OneOrMany::One(GaiseMessage {
        role: "user".to_owned(),
        content: Some(OneOrMany::One(GaiseContent::Text { 
            text: "Hello, how are you?".to_owned() 
        })),
        ..Default::default()
    }),
    ..Default::default()
};

let response = service.instruct(&request).await?;
```

## How it works

The `GaiseClientService` parses the `model` string to identify the provider.
1. It looks for the first occurrence of `::`.
2. The part before `::` is used as the provider ID.
3. The part after `::` is passed to the specific provider as the actual model name.

If you request `ollama::llama3`, the service will:
1. Initialize (or retrieve) the Ollama client.
2. Call the Ollama client with `model: "llama3"`.

### Live / Realtime Sessions (feature = "live")

With the `live` feature enabled, `GaiseClientService` also implements `GaiseLiveClient` for real-time bidirectional WebSocket sessions (audio + text streaming). Currently supported by `openai` and `gemini`.

```rust
use gaise_core::GaiseLiveClient;
use gaise_core::contracts::*;
use futures_util::StreamExt;

let config = GaiseLiveConfig {
    model: "gemini::gemini-3.1-flash-live-preview".to_string(),
    voice: Some("Puck".to_string()),
    modalities: vec![GaiseLiveModality::Audio, GaiseLiveModality::Text],
    ..Default::default()
};

let session = service.live_connect(&config).await?;

// Send inputs via session.tx (audio, text, tool responses)
// Receive events via session.rx (audio, text, transcripts, tool calls, etc.)
while let Some(event) = session.rx.next().await {
    match event? {
        GaiseLiveEvent::Audio { data, sample_rate } => { /* play audio */ }
        GaiseLiveEvent::Text { text } => { /* display text */ }
        GaiseLiveEvent::ToolCall { id, function } => { /* handle tool call */ }
        GaiseLiveEvent::TurnComplete => { /* model finished responding */ }
        _ => {}
    }
}
```

### Example with Anthropic

```rust
let request = GaiseInstructRequest {
    model: "anthropic::claude-sonnet-5".to_string(),
    input: OneOrMany::One(GaiseMessage {
        role: "user".to_owned(),
        content: Some(OneOrMany::One(GaiseContent::Text {
            text: "Hello, Claude!".to_owned()
        })),
        ..Default::default()
    }),
    ..Default::default()
};

let response = service.instruct(&request).await?;
```
