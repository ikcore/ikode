# Examples

> Part of the [GAISe wiki](README.md) · [Rust SDK](sdk.md) · [HTTP API](api.md) · [Capabilities](capabilities.md) · [Models](models.md) · [Flows](flows.md)

Rust examples for every contract surface. They use `GaiseClientService`, so model IDs include a provider prefix; with a [direct client](sdk.md#direct-provider-clients), remove it. Byte vectors are placeholders; load and validate media in your application. The JSON equivalents of these requests are in [api.md](api.md) and, ready to run, in the [Postman collection](../gaise_postman_collection.json).

## Contents

- [Text](#text) · [System instruction and conversation history](#system-instruction-and-conversation-history)
- [Image input](#image-input) · [Audio input](#audio-input) · [File or document input](#file-or-document-input) · [Nested ordered parts](#nested-ordered-parts)
- [Reasoning controls](#reasoning-controls) · [Generated image or image edit](#generated-image-or-image-edit)
- [Tool declaration](#tool-declaration) · [Tool result](#tool-result)
- [Streaming](#streaming) · [Embeddings](#embeddings) · [Usage inspection](#usage-inspection)
- [Model discovery](#model-discovery)
- [Speech](#speech)
- [Live / realtime](#live--realtime)

```rust
use gaise_core::{GaiseClient, GaiseLiveClient};
use gaise_core::contracts::*;
use futures_util::StreamExt;
```

## Text

```rust
let request = GaiseInstructRequest {
    model: "openai::gpt-5.6-terra".into(),
    correlation_id: Some("example-text-1".into()),
    input: OneOrMany::One(GaiseMessage {
        role: "user".into(),
        content: Some(OneOrMany::One(GaiseContent::Text {
            text: "Explain ownership in Rust in three bullets.".into(),
        })),
        ..Default::default()
    }),
    ..Default::default()
};

let response = service.instruct(&request).await?;
```

## System instruction and conversation history

```rust
let request = GaiseInstructRequest {
    model: "anthropic::claude-sonnet-5".into(),
    input: OneOrMany::Many(vec![
        GaiseMessage {
            role: "system".into(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Answer as a concise Rust reviewer.".into(),
            })),
            ..Default::default()
        },
        GaiseMessage {
            role: "user".into(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: "Review this API design.".into(),
            })),
            ..Default::default()
        },
    ]),
    ..Default::default()
};
```

## Image input

```rust
let request = GaiseInstructRequest {
    model: "gemini::gemini-3.6-flash".into(),
    generation_config: Some(GaiseGenerationConfig {
        input_media_resolution: Some("high".into()),
        ..Default::default()
    }),
    input: OneOrMany::One(GaiseMessage {
        role: "user".into(),
        content: Some(OneOrMany::Many(vec![
            GaiseContent::Text { text: "Describe the image.".into() },
            GaiseContent::Image {
                data: image_bytes,
                format: Some("image/png".into()),
            },
        ])),
        ..Default::default()
    }),
    ..Default::default()
};
```

For OpenAI, use `input_image_detail` instead of `input_media_resolution`:

```rust
generation_config: Some(GaiseGenerationConfig {
    input_image_detail: Some("original".into()),
    ..Default::default()
})
```

Only use `original` with a model that advertises it.

## Audio input

```rust
let request = GaiseInstructRequest {
    model: "gemini::gemini-3.6-flash".into(),
    input: OneOrMany::One(GaiseMessage {
        role: "user".into(),
        content: Some(OneOrMany::Many(vec![
            GaiseContent::Text { text: "Transcribe and summarize this clip.".into() },
            GaiseContent::Audio {
                data: audio_bytes,
                format: Some("audio/wav".into()),
            },
        ])),
        ..Default::default()
    }),
    ..Default::default()
};
```

OpenAI Chat, Gemini, Vertex AI, and model-dependent Bedrock paths map supported audio input. Anthropic and Ollama produce an explicit unsupported marker rather than silently dropping it.

## File or document input

```rust
let request = GaiseInstructRequest {
    model: "anthropic::claude-sonnet-5".into(),
    input: OneOrMany::One(GaiseMessage {
        role: "user".into(),
        content: Some(OneOrMany::Many(vec![
            GaiseContent::Text { text: "Summarize the report.".into() },
            GaiseContent::File {
                data: pdf_bytes,
                name: Some("quarterly-report.pdf".into()),
            },
        ])),
        ..Default::default()
    }),
    ..Default::default()
};
```

Provider document formats differ. The common OpenAI client uses Chat Completions: UTF-8 files become tagged text and binary files become a visible unsupported marker. Native OpenAI binary files require a future Responses adapter.

## Nested ordered parts

```rust
let content = GaiseContent::Parts {
    parts: vec![
        GaiseContent::Text { text: "First".into() },
        GaiseContent::Parts {
            parts: vec![
                GaiseContent::Image { data: first_image, format: Some("jpeg".into()) },
                GaiseContent::Text { text: "then this".into() },
            ],
        },
        GaiseContent::Image { data: second_image, format: Some("image/webp".into()) },
    ],
};
```

Adapters recursively flatten this to the provider's ordered part list.

## Reasoning controls

```rust
let request = GaiseInstructRequest {
    model: "anthropic::claude-sonnet-5".into(),
    generation_config: Some(GaiseGenerationConfig {
        max_tokens: Some(16_000),
        thinking_tokens: Some(8_000),
        thinking_effort: Some("high".into()),
        include_thoughts: Some(true),
        ..Default::default()
    }),
    input: OneOrMany::One(GaiseMessage {
        role: "user".into(),
        content: Some(OneOrMany::One(GaiseContent::Text {
            text: "Work through this planning problem.".into(),
        })),
        ..Default::default()
    }),
    ..Default::default()
};
```

The exact mapping is model-aware. Returned `Reasoning` is a provider-approved summary, not a promise of private chain-of-thought. Replay signatures and `RedactedReasoning` blocks unchanged when continuing the same provider conversation.

## Generated image or image edit

```rust
let request = GaiseInstructRequest {
    model: "gemini::gemini-3.1-flash-image".into(),
    generation_config: Some(GaiseGenerationConfig {
        response_modalities: Some(vec!["TEXT".into(), "IMAGE".into()]),
        image_config: Some(GaiseImageConfig {
            aspect_ratio: Some("16:9".into()),
            image_size: Some("2K".into()),
        }),
        ..Default::default()
    }),
    input: OneOrMany::One(GaiseMessage {
        role: "user".into(),
        content: Some(OneOrMany::Many(vec![
            GaiseContent::Text {
                text: "Restyle this as a watercolor landscape.".into(),
            },
            GaiseContent::Image {
                data: source_image,
                format: Some("image/png".into()),
            },
        ])),
        ..Default::default()
    }),
    ..Default::default()
};
```

Gemini and Vertex serialize the controls to the current `responseFormat.image` wire shape. Returned bytes appear as `GaiseContent::Image`. The OpenAI Chat adapter does not wrap the Images API or Responses image-generation tool.

## Tool declaration

```rust
use std::collections::BTreeMap;

let tool = GaiseTool {
    name: "get_weather".into(),
    description: Some("Get weather for several locations".into()),
    parameters: Some(GaiseToolParameter {
        r#type: Some("object".into()),
        properties: Some(BTreeMap::from([(
            "locations".into(),
            GaiseToolParameter {
                r#type: Some("array".into()),
                description: Some("City names".into()),
                items: Some(Box::new(GaiseToolParameter {
                    r#type: Some("string".into()),
                    ..Default::default()
                })),
                ..Default::default()
            },
        )])),
        required: Some(vec!["locations".into()]),
        ..Default::default()
    }),
};

let request = GaiseInstructRequest {
    model: "vertexai::gemini-3.5-flash".into(),
    tools: Some(vec![tool]),
    tool_config: Some(GaiseToolConfig { mode: Some("auto".into()) }),
    input: OneOrMany::One(GaiseMessage {
        role: "user".into(),
        content: Some(OneOrMany::One(GaiseContent::Text {
            text: "Compare London and Paris weather.".into(),
        })),
        ..Default::default()
    }),
    ..Default::default()
};
```

## Tool result

Preserve the assistant message returned by the provider, including call ID and thought signature, then append a result:

```rust
let tool_result = GaiseMessage {
    role: "tool".into(),
    tool_call_id: Some("call-123".into()),
    tool_name: Some("get_weather".into()),
    content: Some(OneOrMany::Many(vec![
        GaiseContent::Text {
            text: r#"{"London":18,"Paris":21}"#.into(),
        },
        GaiseContent::Image {
            data: weather_map_png,
            format: Some("image/png".into()),
        },
    ])),
    ..Default::default()
};

let continuation = GaiseInstructRequest {
    model: "gemini::gemini-3.6-flash".into(),
    input: OneOrMany::Many(vec![original_user_message, assistant_tool_call, tool_result]),
    ..Default::default()
};
```

Multimodal tool results are strongest on Anthropic, Gemini/Vertex, and Bedrock. Supplying both call ID and name is the portable choice.

## Streaming

```rust
let mut stream = service.instruct_stream(&request).await?;
let mut accumulator = GaiseStreamAccumulator::new();

while let Some(item) = stream.next().await {
    let item = item?;
    match &item.chunk {
        GaiseStreamChunk::Text(text) => print!("{text}"),
        GaiseStreamChunk::Content(content) => println!("content: {content:?}"),
        GaiseStreamChunk::ToolCall { index, name, arguments, .. } => {
            println!("tool[{index}] {name:?}: {arguments:?}");
        }
        GaiseStreamChunk::Usage(usage) => println!("usage snapshot: {usage:?}"),
    }
    accumulator.push(&item);
}

let final_usage = accumulator.usage.clone();
let final_message = accumulator.finish();
```

The accumulator replaces repeated usage counters because provider usage events are cumulative snapshots. It does not add them together.

## Embeddings

```rust
let request = GaiseEmbeddingsRequest {
    model: "openai::text-embedding-3-small".into(),
    correlation_id: Some("embed-1".into()),
    input: OneOrMany::Many(vec![
        "First document".into(),
        "Second document".into(),
    ]),
};

let response = service.embeddings(&request).await?;
for vector in response.output {
    println!("dimensions: {}", vector.len());
}
```

Supported routes are OpenAI, Gemini, Vertex AI, Bedrock Titan/Cohere, and Ollama. Anthropic has no embeddings endpoint. The shared request is text-only.

## Usage inspection

```rust
if let Some(usage) = response.usage {
    let input_total = usage.input.as_ref().and_then(|m| {
        m.get("prompt_tokens").or_else(|| m.get("input_tokens"))
    });
    let image_input = usage.input.as_ref().and_then(|m| m.get("image_tokens"));
    let audio_output = usage.output.as_ref().and_then(|m| m.get("audio_tokens"));
    let reasoning = usage.output.as_ref().and_then(|m| m.get("reasoning_tokens"));
    let request_total = usage.total.as_ref().and_then(|m| m.get("total_tokens"));
    println!("{input_total:?} {image_input:?} {audio_output:?} {reasoning:?} {request_total:?}");
}
```

Do not treat a missing modality counter as zero. That provider surface may only return an aggregate.

## Model discovery

```rust
use gaise_core::contracts::{GaiseListModelsRequest, GaiseModality, GaiseOperation, GaiseSupport};

// Everything every configured provider can list, routable and enriched.
let catalog = service.list_models(&GaiseListModelsRequest::default()).await?;
for model in &catalog.models {
    println!(
        "{:<48} status={:?} ops={:?} in={:?} out={:?} tools={:?} sources={:?}",
        model.id,
        model.status,
        model.capabilities.operations,
        model.capabilities.input,
        model.capabilities.output,
        model.capabilities.tools,
        model.capabilities.sources,
    );
}
for error in &catalog.errors {
    eprintln!("{} could not be listed: {}", error.provider, error.message);
}

// Vision-capable streaming chat models from one provider, with the raw record attached.
let vision = service
    .list_models(&GaiseListModelsRequest {
        provider: Some("anthropic".into()),
        operation: Some(GaiseOperation::InstructStream),
        include_raw: true,
        ..Default::default()
    })
    .await?;
let ids: Vec<&str> = vision
    .models
    .iter()
    .filter(|m| m.capabilities.input.contains(&GaiseModality::Image))
    .filter(|m| m.capabilities.tools != GaiseSupport::Unsupported)
    .map(|m| m.id.as_str())
    .collect();

// Installed Ollama tags with /api/show detail (one extra call per tag).
let local = service
    .list_models(&GaiseListModelsRequest {
        provider: Some("ollama".into()),
        include_details: true,
        ..Default::default()
    })
    .await?;
```

`capabilities.sources` tells you whether a claim came from the provider API, the bundled registry, or a name heuristic; `GaiseSupport::Unknown` and empty modality lists mean "nobody said" — see [capabilities.md#model-discovery](capabilities.md#model-discovery). HTTP equivalent: [`GET /v1/models`](api.md#get-v1models).

## Speech

```rust
use gaise_core::GaiseSpeechClient;
use gaise_core::contracts::{GaiseSpeechChunk, GaiseSpeechRequest, GaiseVoiceSettings};

let request = GaiseSpeechRequest {
    model: "elevenlabs::eleven_flash_v2_5".into(),
    voice: Some(std::env::var("ELEVENLABS_VOICE_ID")?), // discover with GaiseClientElevenLabs::list_voices
    input: "The build is green. Deploying now.".into(),
    format: Some("audio/pcm".into()),
    sample_rate: Some(24_000),
    language: Some("en".into()),
    voice_settings: Some(GaiseVoiceSettings {
        stability: Some(0.5),
        similarity: Some(0.75),
        ..Default::default()
    }),
    include_alignment: true,
    ..Default::default()
};

// Whole clip.
let clip = service.speech(&request).await?;
std::fs::write("clip.pcm", &clip.audio)?;
if let Some(alignment) = &clip.alignment {
    println!("{} characters timed", alignment.characters.len());
}

// Chunked as rendered.
let mut stream = service.speech_stream(&request).await?;
while let Some(item) = stream.next().await {
    match item?.chunk {
        GaiseSpeechChunk::Audio { data, .. } => player.write(&data),
        GaiseSpeechChunk::Alignment(a) => subtitles.push(a),
        GaiseSpeechChunk::Usage(u) => println!("{u:?}"),
    }
}
```

Realtime voice: open a [live session](#live--realtime) with `model: "elevenlabs::eleven_flash_v2_5"` and `voice` set, send `GaiseLiveInput::Text` fragments as they are produced (for example from an `instruct_stream`), and play `GaiseLiveEvent::Audio` frames (24 kHz PCM). Send `AudioStreamEnd` at the end of a sentence to flush. HTTP equivalents: [`POST /v1/speech`](api.md#post-v1speech), [`/v1/speech/stream`](api.md#post-v1speechstream), [`/v1/speech/audio`](api.md#post-v1speechaudio).

## Live / realtime

```rust
let config = GaiseLiveConfig {
    model: "openai::gpt-realtime-2.1".into(),
    system_instruction: Some("Be concise.".into()),
    voice: Some("marin".into()),
    modalities: vec![GaiseLiveModality::Audio],
    generation_config: Some(GaiseGenerationConfig {
        thinking_effort: Some("medium".into()),
        ..Default::default()
    }),
    vad_config: Some(GaiseVadConfig {
        enabled: true,
        silence_duration_ms: Some(500),
        prefix_padding_ms: Some(300),
        ..Default::default()
    }),
    transcription: Some(GaiseTranscriptionConfig {
        input: true,
        output: true,
    }),
    ..Default::default()
};

let mut session = service.live_connect(&config).await?;

session.tx.send(GaiseLiveInput::Text {
    text: "What can you see?".into(),
}).await?;

session.tx.send(GaiseLiveInput::Image {
    data: jpeg_frame,
    mime_type: "image/jpeg".into(),
    detail: Some("high".into()),
}).await?;

session.tx.send(GaiseLiveInput::Audio {
    data: pcm_24khz_mono,
    sample_rate: 24_000,
}).await?;

while let Some(event) = session.rx.next().await {
    match event? {
        GaiseLiveEvent::SessionStarted { session_id, model } => {
            println!("started {session_id} with {model}");
        }
        GaiseLiveEvent::Text { text } => print!("{text}"),
        GaiseLiveEvent::Audio { data, sample_rate } => play(data, sample_rate),
        GaiseLiveEvent::Transcript { role, text } => println!("{role}: {text}"),
        GaiseLiveEvent::Reasoning { text, signature } => {
            println!("reasoning summary: {text}; signature={signature:?}");
        }
        GaiseLiveEvent::ToolCall { id, function } => {
            println!("call {id}: {} {:?}", function.name, function.arguments);
        }
        GaiseLiveEvent::ToolCallCancelled { ids } => println!("cancelled: {ids:?}"),
        GaiseLiveEvent::Usage(usage) => println!("usage: {usage:?}"),
        GaiseLiveEvent::Interrupted => stop_playback(),
        GaiseLiveEvent::TurnComplete => break,
        GaiseLiveEvent::Error { message } => eprintln!("live error: {message}"),
        GaiseLiveEvent::SessionEnded => break,
    }
}
```

OpenAI's common audio path requires 24 kHz PCM and accepts PNG/JPEG images. Gemini Live accepts image frames through its realtime video input and supports its own audio rates/formats through the adapter.

### Live tool response

```rust
session.tx.send(GaiseLiveInput::ToolResponse {
    call_id: "call-123".into(),
    name: "get_weather".into(),
    result: serde_json::json!({"temperature": 18}),
}).await?;
```

### Live controls

```rust
session.tx.send(GaiseLiveInput::ActivityStart).await?;
session.tx.send(GaiseLiveInput::ActivityEnd).await?;
session.tx.send(GaiseLiveInput::AudioStreamEnd).await?;
session.tx.send(GaiseLiveInput::ClearAudio).await?;
session.tx.send(GaiseLiveInput::CancelResponse).await?;
session.tx.send(GaiseLiveInput::Close).await?;
```

Only send manual activity boundaries when provider-side automatic activity detection is disabled. Clear/cancel are currently OpenAI-specific; Gemini returns an explicit unsupported error for those variants.

