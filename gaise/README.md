# GAISe (Generative AI Service)

GAISe is a Rust-based abstraction service that standardizes requests and responses across multiple Generative AI service providers. It allows you to write your application once and easily switch between providers like OpenAI, VertexAI, and Ollama.

## Table of Contents
- [Features](#features)
- [Supported Providers](#supported-providers)
- [Installation](#installation)
- [Usage Examples](#usage-examples)
  - [Basic Instruct Request](#basic-instruct-request)
  - [Streaming Responses](#streaming-responses)
  - [Embeddings](#embeddings)
  - [Multi-modality (Images, Audio, Files)](#multi-modality-images-audio-files)
  - [Tool Calling](#tool-calling)
  - [Structured Responses (JSON Schema)](#structured-responses-json-schema)
- [Logging and Correlation ID](#logging-and-correlation-id)
- [Project Structure](#project-structure)

## Features

- **Standardized API**: Unified models for Instruct, Streaming, and Embeddings.
- **Provider Agnostic**: Switch between cloud (VertexAI) and local (Ollama) providers with minimal code changes.
- **Multi-modal Support**: Handle Text, Images, Audio, and Files seamlessly.
- **Tool Calling**: Support for function calling and tool integration.
- **Async First**: Built on top of `tokio` and `async-trait`.

## Supported Providers

| Provider | Crate | Description |
|----------|-------|-------------|
| **Ollama** | `gaise-provider-ollama` | Local LLM execution. |
| **VertexAI** | `gaise-provider-vertexai` | Google Cloud's Generative AI platform. |
| **OpenAI** | `gaise-provider-openai` | OpenAI's API integration. |
| **Bedrock** | `gaise-provider-bedrock` | AWS Bedrock's Generative AI platform. |

## Installation

Add the following to your `Cargo.toml`:

```toml
[dependencies]
gaise-core = { path = "./gaise-core" }
gaise-provider-ollama = { path = "./gaise-provider-ollama" }
# Or bedrock, vertexai, openai
# gaise-provider-bedrock = { path = "./gaise-provider-bedrock" }
# gaise-provider-vertexai = { path = "./gaise-provider-vertexai" }
# gaise-provider-openai = { path = "./gaise-provider-openai" }
tokio = { version = "1", features = ["full"] }
```

## Usage Examples

### Basic Instruct Request

```rust
use gaise_core::GaiseClient;
use gaise_core::contracts::{GaiseInstructRequest, OneOrMany, GaiseMessage, GaiseContent};
use gaise_provider_ollama::ollama_client::GaiseClientOllama;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = GaiseClientOllama::new("http://localhost:11434".to_string());
    
    let request = GaiseInstructRequest {
        model: "llama3".to_string(),
        correlation_id: Some("unique-id".to_string()),
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
    if let OneOrMany::One(message) = response.output {
        if let Some(OneOrMany::One(GaiseContent::Text { text })) = message.content {
            println!("Response: {}", text);
        }
    }
    
    Ok(())
}
```

### Streaming Responses

```rust
use gaise_core::GaiseClient;
use gaise_core::contracts::{GaiseInstructRequest, OneOrMany, GaiseMessage, GaiseContent, GaiseStreamChunk};
use gaise_provider_ollama::ollama_client::GaiseClientOllama;
use futures_util::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = GaiseClientOllama::new("http://localhost:11434".to_string());
    
    let request = GaiseInstructRequest {
        model: "llama3".to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Write a poem about Rust.".to_string(),
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let mut stream = client.instruct_stream(&request).await?;
    while let Some(chunk_res) = stream.next().await {
        let response = chunk_res?;
        if let GaiseStreamChunk::Text(text) = response.chunk {
            print!("{}", text);
        }
    }
    
    Ok(())
}
```

### Embeddings

```rust
use gaise_core::GaiseClient;
use gaise_core::contracts::{GaiseEmbeddingsRequest, OneOrMany};
use gaise_provider_ollama::ollama_client::GaiseClientOllama;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = GaiseClientOllama::new("http://localhost:11434".to_string());
    
    let request = GaiseEmbeddingsRequest {
        model: "all-minilm".to_string(),
        correlation_id: Some("embedding-request-123".to_string()),
        input: OneOrMany::One("Generative AI is amazing.".to_string()),
    };

    let response = client.embeddings(&request).await?;
    println!("Embedding size: {}", response.embeddings.len());
    
    Ok(())
}
```

### AWS Bedrock Example

```rust
use gaise_core::GaiseClient;
use gaise_core::contracts::{GaiseInstructRequest, OneOrMany, GaiseMessage, GaiseContent};
use gaise_provider_bedrock::GaiseClientBedrock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Bedrock client uses default AWS configuration
    let client = GaiseClientBedrock::new().await;
    
    let request = GaiseInstructRequest {
        model: "amazon.titan-text-express-v1".to_string(),
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
    // ... handle response ...
    Ok(())
}
```

### Multi-modality (Images, Audio, Files)

GAISe supports sending binary data for images, audio, and files.

```rust
use gaise_core::contracts::{GaiseContent, GaiseMessage, OneOrMany};

// Example of an image content
let image_content = GaiseContent::Image {
    data: std::fs::read("image.png")?,
    format: Some("image/png".to_string()),
};

// Example of audio content
let audio_content = GaiseContent::Audio {
    data: std::fs::read("audio.mp3")?,
    format: Some("audio/mp3".to_string()),
};

let message = GaiseMessage {
    role: "user".to_string(),
    content: Some(OneOrMany::Many(vec![
        GaiseContent::Text { text: "Describe this image and audio.".to_string() },
        image_content,
        audio_content,
    ])),
    ..Default::default()
};
```

### Tool Calling

Define tools and pass them in the instruct request.

```rust
use gaise_core::contracts::{GaiseTool, GaiseToolParameter};
use std::collections::HashMap;

let mut properties = HashMap::new();
properties.insert("location".to_string(), GaiseToolParameter {
    r#type: Some("string".to_string()),
    description: Some("The city and state, e.g. San Francisco, CA".to_string()),
    ..Default::default()
});

let weather_tool = GaiseTool {
    name: "get_current_weather".to_string(),
    description: Some("Get the current weather in a given location".to_string()),
    parameters: Some(GaiseToolParameter {
        r#type: Some("object".to_string()),
        properties: Some(properties),
        required: Some(vec!["location".to_string()]),
    }),
};

let request = GaiseInstructRequest {
    model: "llama3".to_string(),
    tools: Some(vec![weather_tool]),
    // ... input ...
    ..Default::default()
};
```

### Structured Responses (JSON Schema)

You can force the model to respond with a specific JSON structure by providing a JSON schema in the `generation_config`.

```rust
use gaise_core::contracts::{GaiseInstructRequest, GaiseGenerationConfig, GaiseToolParameter, OneOrMany, GaiseMessage, GaiseContent};
use std::collections::HashMap;

let mut properties = HashMap::new();
properties.insert("num_of_people".to_string(), GaiseToolParameter {
    r#type: Some("string".to_string()),
    description: Some("The number of people in the image".to_string()),
    ..Default::default()
});

let request = GaiseInstructRequest {
    model: "openai::gpt-4o".to_string(),
    input: OneOrMany::One(GaiseMessage {
        role: "user".to_string(),
        content: Some(OneOrMany::One(GaiseContent::Text {
            text: "How many people are in this image?".to_string(),
        })),
        ..Default::default()
    }),
    generation_config: Some(GaiseGenerationConfig {
        response_format: Some(GaiseToolParameter {
            r#type: Some("object".to_string()),
            properties: Some(properties),
            required: Some(vec!["num_of_people".to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    }),
    ..Default::default()
};
```

### Logging and Correlation ID

GAISe provides a logging infrastructure to track requests and responses. You can use the built-in `ConsoleGaiseLogger` or implement the `IGaiseLogger` trait for custom logging.

The `correlation_id` is an optional field in `GaiseInstructRequest` and `GaiseEmbeddingsRequest` that helps link logs together across different services or request stages.

#### Configuring the Logger

When using `GaiseClientService`, you can provide a logger in the configuration:

```rust
use std::sync::Arc;
use gaise_client::{GaiseClientService, GaiseClientConfig};
use gaise_core::logging::ConsoleGaiseLogger;

let config = GaiseClientConfig {
    // ... other provider configurations ...
    logger: Some(Arc::new(ConsoleGaiseLogger::default())),
    ..Default::default()
};

let service = GaiseClientService::new(config);
```

#### Using Correlation ID

```rust
use gaise_core::contracts::GaiseInstructRequest;

let request = GaiseInstructRequest {
    model: "openai::gpt-4o".to_string(),
    correlation_id: Some("my-unique-correlation-id".to_string()),
    // ... other fields ...
    ..Default::default()
};
```

## Project Structure

- `gaise-core`: The core library containing traits and standardized models.
- `gaise-provider-ollama`: Ollama implementation of the `GaiseClient`.
- `gaise-provider-vertexai`: VertexAI implementation of the `GaiseClient`.
- `gaise-provider-openai`: OpenAI implementation of the `GaiseClient`.
- `gaise-provider-bedrock`: AWS Bedrock implementation of the `GaiseClient`.
- `gaise-chatbot`: A sample CLI chatbot application using GAISe.

---
Developed with GAISe - Standardizing Generative AI across providers.
