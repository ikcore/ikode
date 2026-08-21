# gaise-provider-bedrock

[![crates.io](https://img.shields.io/crates/v/gaise-provider-bedrock.svg)](https://crates.io/crates/gaise-provider-bedrock)
[![docs.rs](https://docs.rs/gaise-provider-bedrock/badge.svg)](https://docs.rs/gaise-provider-bedrock)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)

AWS Bedrock provider for [GAISe](https://crates.io/crates/gaise) — implements the `GaiseClient` trait using the AWS Bedrock Runtime SDK.

## Supported Features

- Text, image, audio, and document inputs via `ConverseStream` / `Converse`
- Streaming text, reasoning, tool calls, returned media, and usage
- Nested function tools and multimodal tool results
- Claude and Nova reasoning configuration
- Titan and Cohere text embeddings via `InvokeModel`
- Generation config (temperature, top_p, and max_tokens)
- Usage with input/output/request totals and cache read/write/TTL counters
- AWS credential chain authentication (environment, profile, IAM role, etc.)

## Usage

```rust
use gaise_core::GaiseClient;
use gaise_core::contracts::*;
use gaise_provider_bedrock::bedrock_client::GaiseClientBedrock;

// Uses default AWS credential chain
let client = GaiseClientBedrock::new().await;

let request = GaiseInstructRequest {
    model: "anthropic.claude-fable-5".to_string(),
    input: OneOrMany::One(GaiseMessage {
        role: "user".to_string(),
        content: Some(OneOrMany::One(GaiseContent::Text {
            text: "Hello from Bedrock!".to_string(),
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
| `BEDROCK_REGION` | AWS region (e.g., `us-east-1`) |
| `AWS_ACCESS_KEY_ID` | AWS access key (or use IAM roles) |
| `AWS_SECRET_ACCESS_KEY` | AWS secret key |

## Part of [GAISe](https://github.com/ikcore/gaise)

License: AGPL-3.0-only
