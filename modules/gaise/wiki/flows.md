# Flows

> Part of the [GAISe wiki](README.md) · [HTTP API](api.md) · [Rust SDK](sdk.md) · [Capabilities](capabilities.md) · [Models](models.md) · [Examples](examples.md) · Vendors: [OpenAI](vendor-openai.md) · [Anthropic](vendor-anthropic.md) · [Gemini](vendor-gemini.md) · [Vertex AI](vendor-vertexai.md) · [Bedrock](vendor-bedrock.md) · [Ollama](vendor-ollama.md) · [ElevenLabs](vendor-elevenlabs.md)

These diagrams describe the current implementation boundaries. Route-level sequences (one per HTTP endpoint) are in [api.md](api.md); vendor-specific sequences (auth, endpoint paths, event names) are on each vendor page under "Flow".

## Contents

- [Client routing](#client-routing)
- [Non-streaming instruct](#non-streaming-instruct)
- [Multimodal mapping](#multimodal-mapping)
- [Streaming](#streaming)
- [Tool-call loop](#tool-call-loop)
- [Usage normalization](#usage-normalization)
- [Embeddings](#embeddings)
- [Model discovery](#model-discovery)
- [Speech](#speech)
- [Live session](#live-session)
- [Retry and errors](#retry-and-errors)

## Client routing

```mermaid
flowchart TD
    A[Gaise request with provider::model] --> B{Split first ::}
    B -->|Known provider| C[Remove prefix]
    B -->|Missing or unknown| X[Return routing error]
    C --> D{Client cached?}
    D -->|Yes| E[Reuse Arc client]
    D -->|No| F[Initialize from GaiseClientConfig]
    F --> E
    E --> G[Call instruct, stream, embeddings, list_models, or live]
```

Source: [`GaiseClientService::get_client`](../gaise-client/src/lib.rs). Construction details per provider: [sdk.md#direct-provider-clients](sdk.md#direct-provider-clients).

## Non-streaming instruct

```mermaid
sequenceDiagram
    participant App
    participant Core as GAISe contract
    participant Adapter
    participant Provider
    App->>Core: GaiseInstructRequest
    Core->>Adapter: Provider-neutral messages/config/tools
    Adapter->>Adapter: Flatten parts and map MIME, roles, tools, reasoning
    Adapter->>Provider: Provider request
    Provider-->>Adapter: Provider response + usage
    Adapter->>Adapter: Preserve order, IDs, signatures, media
    Adapter-->>Core: GaiseInstructResponse
    Core-->>App: Messages + external_id + input/output/total usage
```

## Multimodal mapping

```mermaid
flowchart LR
    P[Ordered GaiseContent] --> F[Recursive flatten]
    F --> T[Text]
    F --> I[Image + normalized MIME]
    F --> A[Audio + normalized MIME]
    F --> D[File + inferred MIME]
    F --> R[Reasoning + signature]
    F --> RR[Opaque redacted reasoning]
    T --> W[Provider wire parts]
    I --> W
    A --> W
    D --> W
    R --> W
    RR --> W
    W --> S{Selected endpoint supports part?}
    S -->|Yes| N[Native provider block]
    S -->|Safe text fallback| M[Explicit tagged/unsupported marker]
    S -->|No valid mapping| E[Return error]
```

Adapters do not silently discard unsupported content. Whether they can use a text marker or must fail depends on the provider's valid message schema — see the per-provider table in [capabilities.md#modalities-by-provider](capabilities.md#modalities-by-provider) and MIME helpers in [`gaise_content.rs`](../gaise-core/src/contracts/gaise_content.rs).

## Streaming

```mermaid
flowchart TD
    A[HTTP response byte chunks] --> B[Provider framing buffer]
    B --> C{Complete SSE/NDJSON frame?}
    C -->|No| B
    C -->|Yes| D[Deserialize provider event]
    D --> E{Event kind}
    E -->|Text delta| T[GaiseStreamChunk::Text]
    E -->|Reasoning/media| M[GaiseStreamChunk::Content]
    E -->|Tool delta| F[Indexed ToolCall chunk]
    E -->|Usage snapshot| U[GaiseStreamChunk::Usage]
    T --> A2[GaiseStreamAccumulator]
    M --> A2
    F --> A2
    U --> A2
    A2 --> O[Ordered message + assembled tool calls]
    A2 --> US[Latest usage counters by key]
```

The raw transport chunks are not assumed to align with SSE lines or JSON objects. Buffers retain an incomplete trailing frame until the next network chunk. Chunk variants and the accumulator API: [sdk.md#streaming](sdk.md#streaming); framing per provider: [capabilities.md#streaming](capabilities.md#streaming).

## Tool-call loop

```mermaid
sequenceDiagram
    participant App
    participant GAISe
    participant Model
    participant Tool
    App->>GAISe: Request + recursive tool schema
    GAISe->>Model: Provider-native tool declaration
    Model-->>GAISe: Call ID + name + arguments + optional signature
    GAISe-->>App: GaiseToolCall
    App->>Tool: Validate and execute arguments
    Tool-->>App: JSON and/or supported media result
    App->>GAISe: History + assistant call + tool result with ID and name
    GAISe->>Model: Provider-native function/tool result
    Model-->>GAISe: Final text/reasoning/media + usage
    GAISe-->>App: Normalized response
```

Parallel streaming tool calls are correlated by their provider index. Gemini/Vertex names and thought signatures must be preserved alongside IDs.

## Usage normalization

```mermaid
flowchart TD
    P[Provider usage object] --> I[Input map]
    P --> O[Output map]
    P --> T[Request-wide total map]
    I --> IA[Aggregate provider input counter]
    I --> IM[Reported text/image/audio/video/document details]
    I --> IC[Reported cache and tool-prompt details]
    O --> OA[Aggregate provider output counter]
    O --> OM[Reported text/image/audio details]
    O --> OR[Reported reasoning/prediction/tool details]
    T --> TT[total_tokens]
    IA --> G[GaiseUsage]
    IM --> G
    IC --> G
    OA --> G
    OM --> G
    OR --> G
    TT --> G
```

Detail counters overlap aggregates. Missing modality detail stays missing; it is never inferred from content type or aggregate tokens.

## Embeddings

```mermaid
flowchart LR
    A[OneOrMany text strings] --> B{Provider}
    B --> O[OpenAI embeddings]
    B --> G[Gemini batchEmbedContents]
    B --> V[Vertex prediction endpoint]
    B --> D[Bedrock Titan/Cohere InvokeModel]
    B --> L[Ollama api/embed]
    O --> N[Vec of float vectors]
    G --> N
    V --> N
    D --> N
    L --> N
    N --> R[GaiseEmbeddingsResponse + available usage]
```

Anthropic has no embeddings branch. The common embedding input is currently text-only. Models and usage per provider: [capabilities.md#embeddings](capabilities.md#embeddings).

## Model discovery

```mermaid
flowchart TD
    Q["GaiseListModelsRequest<br/>provider? operation? include_details? include_raw?"] --> P{provider set?}
    P -->|yes| ONE["get_client(provider)"]
    P -->|no| FAN["configured_providers()<br/>credentials present + add_client keys"]
    FAN --> JOIN["join_all(list_provider_models)"]
    ONE --> AD
    JOIN --> AD
    subgraph AD["adapter list_models — provider facts only"]
        direction LR
        OAI["OpenAI GET /models<br/>id, created, shutdown_date<br/>+ name heuristics"]
        ANT["Anthropic GET /models<br/>capabilities, limits<br/>cursor paging"]
        GEM["Gemini GET /models<br/>methods to ops, thinking, limits"]
        VAI["Vertex v1beta1 publishers list<br/>name, launchStage"]
        BED["Bedrock ListFoundationModels<br/>+ ListInferenceProfiles"]
        OLL["Ollama /api/tags<br/>+ /api/show when include_details"]
    end
    AD --> EN["registry.enrich(model)<br/>union modalities · fill unknowns · lifecycle"]
    EN --> ID["id = provider::id"]
    ID --> FIL["retain_operation(filter)"]
    FIL --> OUT["GaiseListModelsResponse<br/>models + per-provider errors"]
```

Provider failures inside a fan-out become entries in `errors`; a single-provider request propagates the error. Enrichment runs before the operation filter so registry-supplied operations count. Sources: [`gaise-client/src/lib.rs`](../gaise-client/src/lib.rs), [`gaise-core/src/registry.rs`](../gaise-core/src/registry.rs); semantics: [capabilities.md#model-discovery](capabilities.md#model-discovery); HTTP: [api.md#get-v1models](api.md#get-v1models).

```mermaid
flowchart LR
    A["model id"] --> B{"bedrock?"}
    B -->|yes| C["strip us. eu. apac. ap. jp. au. ca. il. global. us-gov."]
    B -->|no| D[keep]
    C --> E
    D --> E{"exact id or alias?"}
    E -->|yes| W[entry]
    E -->|no| F{"glob match?"}
    F -->|yes| G[longest pattern]
    F -->|no| H{"snapshot suffix?<br/>-YYYY-MM-DD · -YYYYMMDD · -vN:M · :N"}
    H -->|yes| W
    H -->|no| N[no entry]
    G --> W
```

## Speech

```mermaid
flowchart LR
    R["GaiseSpeechRequest<br/>model · voice · input · format"] --> F["resolve output format<br/>MIME + rate → provider format"]
    F --> B["build provider body<br/>model rules: language_code, speed, seed"]
    B --> E{mode}
    E -->|speech| C["POST text-to-speech<br/>(+ with-timestamps)"]
    E -->|speech_stream| S["POST …/stream<br/>(+ NDJSON timestamps)"]
    C --> O["GaiseSpeechResponse<br/>audio · format · alignment · usage"]
    S --> K["Audio / Alignment chunks<br/>then Usage"]
    K --> H["HTTP: SSE or raw audio body"]
```

Realtime voice is the [live session](#live-session) flow with text in and audio out ([vendor-elevenlabs.md#live--realtime](vendor-elevenlabs.md#live--realtime)). Sources: [`gaise_speech.rs`](../gaise-core/src/contracts/gaise_speech.rs), [`elevenlabs_client.rs`](../gaise-provider-elevenlabs/src/elevenlabs_client.rs); HTTP: [api.md#post-v1speech](api.md#post-v1speech).

## Live session

```mermaid
sequenceDiagram
    participant App
    participant Live as GaiseLiveClient
    participant WS as Provider WebSocket
    App->>Live: live_connect(config)
    Live->>WS: Connect and send setup/session update
    WS-->>Live: Setup/session confirmation
    Live-->>App: GaiseLiveSession + SessionStarted
    par Input task
        App->>Live: Text / audio / image / control
        Live->>WS: Provider client event
        App->>Live: ToolResponse
        Live->>WS: Provider function response
    and Output task
        WS-->>Live: Text/audio/transcript/tool/usage events
        Live-->>App: GaiseLiveEvent stream
    end
    App->>Live: Close
    Live->>WS: Close frame
    WS-->>Live: Closed
    Live-->>App: SessionEnded
```

OpenAI uses the current GA nested audio/session shape. Gemini uses `setup`, `realtimeInput`, and `toolResponse`; images are sent as realtime video frames. ElevenLabs sessions carry text in and audio out over `stream-input` (or text-to-dialogue for `eleven_v3*`). A setup error or early socket close is surfaced instead of being reported as a started session. Contracts: [sdk.md#live](sdk.md#live); WebSocket route: [api.md#get-v1live](api.md#get-v1live); providers: [vendor-openai.md#live--realtime](vendor-openai.md#live--realtime), [vendor-gemini.md#live--realtime](vendor-gemini.md#live--realtime).

## Retry and errors

```mermaid
flowchart TD
    A[Send request] --> B{HTTP status}
    B -->|Success| C[Parse response]
    B -->|Transient status| D{Retry budget remains?}
    D -->|Yes| E[Backoff] --> A
    D -->|No| F[Return provider error]
    B -->|Caller/schema/auth error| F
    C -->|Valid shape| G[Map response]
    C -->|Invalid shape| H[Return parse error with bounded context]
```

Retries are adapter-specific and limited to transient classes: today only OpenAI retries (429/5xx/transport, plus one retry of the GPT-5.6 function-tool `reasoning_effort` incompatibility — [`send_with_retry`](../gaise-provider-openai/src/openai_client.rs)). Invalid inputs, authentication failures, and unsupported content are not made to look successful. HTTP status mapping: [api.md#errors](api.md#errors).

