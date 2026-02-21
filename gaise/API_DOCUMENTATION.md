# GAISe API Documentation

GAISe (Generative AI Service) is an abstraction service that provides a standardized API for multiple Generative AI providers (OpenAI, VertexAI, Ollama).

## Base URL

The default base URL for the API is `http://localhost:3000`.

## Model Naming Convention

All requests require a `model` field. The format for the model name is:
`provider::model_name`

Examples:
- `ollama::llama3`
- `vertexai::gemini-1.5-flash`

## Endpoints

### 1. Instruct (Non-Streaming)

Generates a completion for a given prompt.

- **URL:** `/v1/instruct`
- **Method:** `POST`
- **Content-Type:** `application/json`

#### Simple Instruct Example

**Request:**
```json
{
  "model": "ollama::llama3",
  "input": {
    "role": "user",
    "content": {
      "type": "text",
      "text": "Why is the sky blue?"
    }
  }
}
```

**Response:**
```json
{
  "output": {
    "role": "assistant",
    "content": {
      "type": "text",
      "text": "The sky appears blue due to a phenomenon called Rayleigh scattering..."
    }
  },
  "external_id": "...",
  "usage": {
    "input": { "prompt_tokens": 10 },
    "output": { "completion_tokens": 50 }
  }
}
```

#### Multi-Turn Conversation Example

**Request:**
```json
{
  "model": "ollama::llama3",
  "input": [
    {
      "role": "user",
      "content": { "type": "text", "text": "Hello!" }
    },
    {
      "role": "assistant",
      "content": { "type": "text", "text": "Hi there! How can I help you today?" }
    },
    {
      "role": "user",
      "content": { "type": "text", "text": "Tell me a joke." }
    }
  ]
}
```

#### Tool Handling Example

You can define tools (functions) that the model can choose to call.

**Request:**
```json
{
  "model": "ollama::llama3",
  "tools": [
    {
      "name": "get_weather",
      "description": "Get the current weather in a given location",
      "parameters": {
        "type": "object",
        "properties": {
          "location": {
            "type": "string",
            "description": "The city and state, e.g. San Francisco, CA"
          },
          "unit": {
            "type": "string",
            "enum": ["celsius", "fahrenheit"]
          }
        },
        "required": ["location"]
      }
    }
  ],
  "input": {
    "role": "user",
    "content": { "type": "text", "text": "What's the weather like in London?" }
  }
}
```

**Response (Model calling a tool):**
```json
{
  "output": {
    "role": "assistant",
    "content": null,
    "tool_calls": [
      {
        "id": "call_123",
        "type": "function",
        "function": {
          "name": "get_weather",
          "arguments": "{\"location\": \"London\"}"
        }
      }
    ]
  },
  "external_id": "...",
  "usage": { "input": { "prompt_tokens": 10 }, "output": { "completion_tokens": 5 } }
}
```

---

### 2. Instruct (Streaming)

Streams the response using Server-Sent Events (SSE).

- **URL:** `/v1/instruct/stream`
- **Method:** `POST`
- **Content-Type:** `application/json`

**Request:**
Same as the Instruct endpoint.

**Response (SSE Stream):**
```text
data: {"chunk": {"text": "The"}, "external_id": "..."}

data: {"chunk": {"text": " sky"}, "external_id": "..."}

data: {"chunk": {"text": " is"}, "external_id": "..."}

...

data: {"chunk": {"usage": {"input": {"prompt_tokens": 10}, "output": {"completion_tokens": 5}}}, "external_id": "..."}
```

Each data packet is a `GaiseInstructStreamResponse` JSON object.

---

### 3. Embeddings

Generates vector embeddings for the provided input text.

- **URL:** `/v1/embeddings`
- **Method:** `POST`
- **Content-Type:** `application/json`

#### Single Embedding Example

**Request:**
```json
{
  "model": "ollama::all-minilm",
  "input": "The quick brown fox jumps over the lazy dog."
}
```

**Response:**
```json
{
  "output": [
    [0.0123, -0.456, 0.789]
  ],
  "external_id": "...",
  "usage": { "input": { "prompt_tokens": 9 }, "output": { "completion_tokens": 0 } }
}
```

#### Multi Embedding Example

**Request:**
```json
{
  "model": "ollama::all-minilm",
  "input": [
    "First sentence to embed.",
    "Second sentence to embed."
  ]
}
```

**Response:**
```json
{
  "output": [
    [0.1, 0.2, 0.3],
    [0.4, 0.5, 0.6]
  ],
  "external_id": "...",
  "usage": { "input": { "prompt_tokens": 10 }, "output": { "completion_tokens": 0 } }
}
```

## Configuration

The API server is configured via environment variables:

| Variable | Description | Default |
|----------|-------------|---------|
| `OLLAMA_URL` | The URL of the Ollama service | `http://localhost:11434` |
| `VERTEXAI_API_URL` | The URL of the Vertex AI API | (empty) |
| `VERTEXAI_SA_PATH` | Path to the Google Cloud Service Account JSON file | (none) |

---

## Technical Details

### Content Types
The `content` field in messages supports multiple modalities:
- `text`: `{ "type": "text", "text": "..." }`
- `image`: `{ "type": "image", "data": [bytes], "format": "image/png" }`
- `audio`: `{ "type": "audio", "data": [bytes], "format": "audio/wav" }`
- `file`: `{ "type": "file", "data": [bytes], "name": "filename.pdf" }`

### OneOrMany
Many fields use a `OneOrMany<T>` pattern, meaning you can provide either a single item or an array of items.
- `input` in `GaiseEmbeddingsRequest` (string or array of strings)
- `input` in `GaiseInstructRequest` (message or array of messages)
- `content` in `GaiseMessage` (content object or array of content objects)
- `output` in `GaiseInstructResponse` (message or array of messages)
