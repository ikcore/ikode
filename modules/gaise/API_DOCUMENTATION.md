# GAISe HTTP API

The `gaise-api` crate exposes the common GAISe contracts as JSON, Server-Sent Events (SSE), and an optional live WebSocket. The default local address is `http://localhost:3000`.

This file is the short wire-contract summary. The full reference — every route with a sequence diagram, the Rust SDK, per-vendor mapping details, capability matrices, and the complete model catalog — is the [wiki](wiki/README.md): [api.md](wiki/api.md) · [sdk.md](wiki/sdk.md) · [capabilities.md](wiki/capabilities.md) · [models.md](wiki/models.md) · [flows.md](wiki/flows.md) · [examples.md](wiki/examples.md). Ready-to-run requests for every route are in [`gaise_postman_collection.json`](gaise_postman_collection.json).

## Model routing

HTTP requests use `provider::model-id`:

- `openai::gpt-5.6`
- `anthropic::claude-sonnet-5`
- `gemini::gemini-3.6-flash`
- `vertexai::gemini-3.5-flash`
- `bedrock::us.anthropic.claude-sonnet-5`
- `ollama::qwen3:8b`

The router removes the provider prefix before forwarding the request. GAISe does not restrict IDs to a hard-coded allowlist; use [`GET /v1/models`](#get-v1models) for live discovery and consult [`gaise-core/model-registry.toml`](gaise-core/model-registry.toml) and the provider catalog for lifecycle guidance.

## `POST /v1/instruct`

Generates one or more assistant messages.

```json
{
  "model": "gemini::gemini-3.6-flash",
  "correlation_id": "request-123",
  "generation_config": {
    "max_tokens": 4096,
    "thinking_effort": "medium",
    "include_thoughts": true
  },
  "input": {
    "role": "user",
    "content": {
      "type": "text",
      "text": "Explain Rayleigh scattering."
    }
  }
}
```

Typical response:

```json
{
  "output": [
    {
      "role": "assistant",
      "content": [
        {
          "type": "reasoning",
          "text": "I should explain the wavelength dependence clearly.",
          "signature": "provider-opaque-signature"
        },
        {
          "type": "text",
          "text": "Shorter wavelengths are scattered more strongly..."
        }
      ]
    }
  ],
  "external_id": "provider-response-id",
  "usage": {
    "input": { "prompt_tokens": 12, "text_tokens": 12 },
    "output": { "candidates_tokens": 87, "text_tokens": 70, "reasoning_tokens": 17 },
    "total": { "total_tokens": 99 }
  }
}
```

`input`, message `content`, and response `output` use `OneOrMany<T>` and therefore accept either one object or an array.

Usage has independent `input`, `output`, and request-wide `total` maps. Counters in one map can overlap: an aggregate such as `prompt_tokens` includes its `text_tokens`, `image_tokens`, `audio_tokens`, and cached subsets. Do not sum every map value. GAISe returns a modality counter only when the provider reports it; it does not label an undifferentiated prompt as text or infer an image/audio split. See [`wiki/sdk.md`](wiki/sdk.md#usage) for provider-specific fields.

### Generation configuration

All fields are optional:

| Field | Type | Meaning |
|---|---|---|
| `temperature` | number | Sampling temperature when the selected model permits it |
| `top_k` | unsigned integer | Provider sampling control |
| `top_p` | number | Nucleus sampling control |
| `max_tokens` | unsigned integer | Maximum provider output budget |
| `thinking_tokens` | unsigned integer | Manual reasoning budget where supported |
| `thinking_effort` | string | `low`, `medium`, `high`, `xhigh`, `max`, or a provider-specific value |
| `include_thoughts` | boolean | Request returned thought/reasoning summaries where supported |
| `response_modalities` | string array | For example, `["TEXT", "IMAGE"]` |
| `image_config.aspect_ratio` | string | Provider-native value such as `16:9` |
| `image_config.image_size` | string | Provider-native value such as `1K`, `2K`, or `4K` |
| `input_image_detail` | string | OpenAI image detail: `low`, `high`, `auto`, or `original` |
| `input_media_resolution` | string | Gemini/Vertex input image, video, or PDF resolution such as `low`, `medium`, `high`, or the full `MEDIA_RESOLUTION_*` value |
| `cache_key` | string | Stable prompt-cache identifier/hint where supported |

Unsupported controls are omitted or handled according to the provider adapter. Some newer models reject sampling parameters entirely; the model-aware mappers suppress those fields.

### Content objects

JSON byte data is an array of integers from 0 through 255.

```json
{ "type": "text", "text": "hello" }
```

```json
{ "type": "image", "data": [137, 80, 78, 71], "format": "image/png" }
```

```json
{ "type": "audio", "data": [82, 73, 70, 70], "format": "audio/wav" }
```

```json
{ "type": "file", "data": [37, 80, 68, 70], "name": "report.pdf" }
```

```json
{
  "type": "reasoning",
  "text": "A provider-returned summary",
  "signature": "opaque-value-to-preserve"
}
```

```json
{
  "type": "redacted_reasoning",
  "data": [111, 112, 97, 113, 117, 101]
}
```

`redacted_reasoning` is opaque provider data. Preserve and replay it byte-for-byte; do not display or alter it.

```json
{
  "type": "parts",
  "parts": [
    { "type": "text", "text": "Describe this:" },
    { "type": "image", "data": [1, 2, 3], "format": "png" }
  ]
}
```

Nested parts are flattened by provider adapters while retaining their order. Actual media and document support depends on the chosen model. In particular, binary OpenAI file input requires the Responses API and is not available through the current Chat Completions adapter.

### Image-output request

```json
{
  "model": "gemini::gemini-3.1-flash-image",
  "generation_config": {
    "response_modalities": ["TEXT", "IMAGE"],
    "image_config": {
      "aspect_ratio": "16:9",
      "image_size": "2K"
    }
  },
  "input": {
    "role": "user",
    "content": { "type": "text", "text": "Create a watercolor landscape." }
  }
}
```

Generated bytes are returned as normal `image`, `audio`, or `file` content objects.

### Tool calling

```json
{
  "model": "anthropic::claude-sonnet-5",
  "tool_config": { "mode": "auto" },
  "tools": [
    {
      "name": "get_weather",
      "description": "Get current weather",
      "parameters": {
        "type": "object",
        "properties": {
          "location": {
            "type": "string",
            "description": "City and country"
          },
          "options": {
            "type": "object",
            "properties": {
              "units": { "type": "string" }
            }
          }
        },
        "required": ["location"]
      }
    }
  ],
  "input": {
    "role": "user",
    "content": { "type": "text", "text": "Weather in London?" }
  }
}
```

A tool call has this shape:

```json
{
  "id": "call-123",
  "type": "function",
  "function": {
    "name": "get_weather",
    "arguments": "{\"location\":\"London\"}"
  },
  "thought_signature": "optional-provider-signature"
}
```

Return tool output as a new message with `tool_call_id`, `tool_name`, and result `content`:

```json
{
  "role": "tool",
  "tool_call_id": "call-123",
  "tool_name": "get_weather",
  "content": { "type": "text", "text": "{\"temperature\":18}" }
}
```

`tool_name` is optional for OpenAI, Anthropic, and Bedrock, but current Gemini and Vertex function responses require the name in addition to the optional provider call ID. Supplying both is portable.

## `POST /v1/instruct/stream`

Accepts the same request as `/v1/instruct` and returns SSE. Each `data:` value is a `GaiseInstructStreamResponse`.

```text
data: {"chunk":{"text":"The"},"external_id":"response-1"}

data: {"chunk":{"content":{"type":"image","data":[1,2,3],"format":"image/png"}}}

data: {"chunk":{"tool_call":{"index":0,"id":"call-1","name":"get_weather","arguments":"{","thought_signature":null}}}

data: {"chunk":{"usage":{"input":{"prompt_tokens":10},"output":{"completion_tokens":5},"total":{"total_tokens":15}}}}
```

Chunk variants are:

- `text`: a text delta.
- `content`: a complete reasoning or media part.
- `tool_call`: an indexed tool-call delta.
- `usage`: provider usage counters.

The Rust `GaiseStreamAccumulator` can collect these chunks into a complete ordered message.

## `POST /v1/embeddings`

```json
{
  "model": "openai::text-embedding-3-small",
  "correlation_id": "embed-123",
  "input": ["First document", "Second document"]
}
```

```json
{
  "output": [
    [0.0123, -0.0456, 0.0789],
    [0.0987, -0.0654, 0.0321]
  ],
  "external_id": "provider-response-id",
  "usage": {
    "input": { "prompt_tokens": 4 },
    "total": { "total_tokens": 4 }
  }
}
```

The common embeddings contract currently accepts text even where the provider offers multimodal embedding models. Usage is present only when that embedding endpoint reports it; Gemini `batchEmbedContents` and Bedrock Cohere responses currently leave it absent rather than estimating tokens.

## `GET /v1/models`

Lists the models reachable through every configured provider, normalized to the common `GaiseModel` record. IDs are returned in routable `provider::id` form.

| Query parameter | Meaning |
|---|---|
| `provider` | Restrict to one provider key (`openai`, `anthropic`, `gemini`, `vertexai`, `bedrock`, `ollama`, or a custom key registered with `add_client`). |
| `operation` | Keep only models whose `capabilities.operations` include `instruct`, `instruct_stream`, `embeddings`, or `live`. |
| `include_details` | Fetch per-model detail where that costs extra requests (Ollama `/api/show`). Default `false`. |
| `include_raw` | Attach each provider's native record as `raw`. Default `false`. |
| `correlation_id` | Logged with the request. |

```json
{
  "models": [
    {
      "id": "anthropic::claude-opus-5",
      "provider": "anthropic",
      "display_name": "Claude Opus 5",
      "created_at": "2026-07-24T00:00:00Z",
      "status": "active",
      "retirement_not_before": "2027-07-24",
      "capabilities": {
        "input": ["text", "image", "file"],
        "output": ["text"],
        "operations": ["instruct", "instruct_stream"],
        "tools": "supported",
        "reasoning": "supported",
        "reasoning_values": ["low", "medium", "high", "xhigh", "max"],
        "structured_output": "supported",
        "sources": ["provider", "registry"]
      },
      "limits": { "max_input_tokens": 1000000, "max_output_tokens": 128000 }
    },
    {
      "id": "openai::text-embedding-3-small",
      "provider": "openai",
      "created_at": "2024-01-22T18:43:17Z",
      "status": "active",
      "capabilities": {
        "input": ["text"],
        "output": ["embedding"],
        "operations": ["embeddings"],
        "tools": "unsupported",
        "reasoning": "unsupported",
        "structured_output": "unknown",
        "sources": ["provider", "heuristic", "registry"]
      },
      "limits": {}
    }
  ],
  "errors": [
    { "provider": "ollama", "message": "error sending request for url (http://localhost:11434/api/tags)" }
  ]
}
```

Semantics:

- `tools`, `reasoning`, and `structured_output` are tri-state: `supported`, `unsupported`, or `unknown`. Empty `input`/`output` lists mean *unknown*, never *none*.
- `operations` lists the GAISe trait methods that can drive the model. Image-generation-only, TTS, and transcription models appear with an empty list.
- `sources` records where the claims came from: `provider` (the vendor's model API), `registry` (the bundled `model-registry.toml` overlay), `heuristic` (an adapter name rule, used for OpenAI and Vertex AI whose APIs report no capabilities).
- Without `provider`, the listing covers every provider with credentials in the configuration; providers that fail appear in `errors` and do not hide the others. With `provider`, a failure returns HTTP 500.
- Bedrock lists both foundation models and system-defined cross-region inference profiles (`us.`, `global.` ...), which inherit the foundation model's capabilities.

`GET /v1/models/{provider}::{id}` returns the single matching record or 404. Unknown `operation` values and IDs without the `::` separator return 400.

## `POST /v1/speech`, `POST /v1/speech/stream`, `POST /v1/speech/audio`

Text-to-speech (ElevenLabs). `/v1/speech` returns `GaiseSpeechResponse` as JSON (`audio` byte array, `format`, `sample_rate`, optional `alignment`, `usage`); `/v1/speech/stream` returns SSE chunks (`audio`, `alignment`, `usage`); `/v1/speech/audio` returns the raw audio body with `Content-Type` and `X-Gaise-Sample-Rate`. A `voice` id is required. Full reference: [`wiki/api.md`](wiki/api.md#post-v1speech).

```json
{ "model": "elevenlabs::eleven_flash_v2_5", "voice": "<voice_id>", "input": "Hello.", "format": "audio/pcm", "sample_rate": 24000 }
```

## `GET /v1/live`

When the API is built with live-provider features, this WebSocket route proxies the common live protocol to OpenAI Realtime or Gemini Live. Live sessions are provider- and model-specific and require credentials. They are not exercised by the default test suite.

Client-to-server input variants are `text`, `audio`, `image`, `tool_response`, `activity_start`, `activity_end`, `audio_stream_end`, `clear_audio`, `cancel_response`, and `close`. Server-to-client event variants are `session_started`, `text`, `audio`, `transcript`, `reasoning`, `tool_call`, `tool_call_cancelled`, `turn_complete`, `interrupted`, `usage`, `error`, and `session_ended`.

OpenAI Realtime accepts 24 kHz PCM audio in the common audio path and PNG/JPEG still images. Gemini Live accepts audio and image/video frames through its realtime streams. Manual activity, clear, and cancel operations remain provider-specific; unsupported operations produce an explicit error event rather than a fabricated success.

The full live protocol, wire examples, and provider differences are documented in [`wiki/examples.md`](wiki/examples.md#live--realtime) and [`wiki/flows.md`](wiki/flows.md#live-session).

## Per-request connection overrides

Any request body may include `"connection": {"api_url": "...", "api_key": "...", "region": "...", "service_account": {...}}` to override the configured endpoint/credentials for that call; unset fields fall back to the environment. See [`wiki/api.md`](wiki/api.md#per-request-connection-overrides).

## Configuration

| Variable | Description | Default |
|---|---|---|
| `OPENAI_API_KEY` | OpenAI credential | none |
| `OPENAI_API_URL` | OpenAI base URL | `https://api.openai.com/v1` |
| `ANTHROPIC_API_KEY` | Anthropic credential | none |
| `ANTHROPIC_API_URL` | Anthropic base URL | `https://api.anthropic.com/v1` |
| `GEMINI_API_KEY` | Gemini credential | none |
| `GEMINI_API_URL` | Gemini base URL | provider default |
| `VERTEXAI_SA_PATH` | Google service-account JSON | none |
| `VERTEXAI_API_URL` | Vertex endpoint override | none |
| `BEDROCK_REGION` | AWS region | AWS SDK resolution |
| `OLLAMA_URL` | Ollama endpoint | `http://localhost:11434` |
| `ELEVENLABS_API_KEY` | ElevenLabs credential | none |
| `ELEVENLABS_API_URL` | ElevenLabs base URL | `https://api.elevenlabs.io` |
| `GAISE_PORT` | HTTP listen port | `3000` |
