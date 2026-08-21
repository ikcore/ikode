# gaise-provider-ollama

[![crates.io](https://img.shields.io/crates/v/gaise-provider-ollama.svg)](https://crates.io/crates/gaise-provider-ollama)
[![docs.rs](https://docs.rs/gaise-provider-ollama/badge.svg)](https://docs.rs/gaise-provider-ollama)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)

Ollama provider for [GAISe](https://crates.io/crates/gaise) — implements the `GaiseClient` trait for local LLM inference via the [Ollama](https://ollama.com) API.

## Supported Features

- Text, image, and UTF-8 file instruct requests
- Streaming text, reasoning, tool calls, and usage
- Embeddings (`/api/embed`)
- Function calling / tool use (model-dependent)
- Model-dependent thinking controls
- Generation config (temperature, top_k, top_p, stop, num_predict)
- Prompt/completion/total usage (Ollama does not report modality-specific token splits)

## Usage

```rust
use gaise_core::GaiseClient;
use gaise_core::contracts::*;
use gaise_provider_ollama::ollama_client::GaiseClientOllama;

let client = GaiseClientOllama::new("http://localhost:11434".to_string());

let request = GaiseInstructRequest {
    model: "qwen3".to_string(),
    input: OneOrMany::One(GaiseMessage {
        role: "user".to_string(),
        content: Some(OneOrMany::One(GaiseContent::Text {
            text: "What is the capital of France?".to_string(),
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
| `OLLAMA_URL` | Ollama API URL (default: `http://localhost:11434`) |

## Note

Thinking, vision, and tool support depend on the locally installed model tag. GAISe forwards supported thinking settings and returns Ollama's reasoning stream separately from answer text.

## Part of [GAISe](https://github.com/ikcore/gaise)

License: AGPL-3.0-only
