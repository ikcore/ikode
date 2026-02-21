# gaise-client

`gaise-client` is a provider aggregator for the GAISe (Generative AI Service) project. It allows you to use multiple AI providers (OpenAI, VertexAI, Ollama) through a single interface, routing requests based on a model naming convention.

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

To use only specific providers, disable default features in your `Cargo.toml`:

```toml
[dependencies]
gaise-client = { version = "0.1.0", default-features = false, features = ["openai"] }
```

## Supported Providers

- `openai`
- `vertexai`
- `ollama`

## Usage

### Configuration

First, set up the `GaiseClientConfig` with the necessary credentials and URLs. Note that fields in `GaiseClientConfig` are conditionally compiled based on enabled features.

```rust
use gaise_client::{GaiseClientConfig, GaiseClientService};

let config = GaiseClientConfig {
    #[cfg(feature = "openai")]
    openai_api_key: Some("your-openai-key".to_string()),
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
    model: "openai::gpt-4o".to_string(),
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
