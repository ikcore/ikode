# Models

> Part of the [GAISe wiki](README.md) · [Capabilities](capabilities.md) · [HTTP API](api.md#get-v1models) · [Rust SDK](sdk.md#model-discovery) · [Flows](flows.md#model-discovery) · Vendors: [OpenAI](vendor-openai.md) · [Anthropic](vendor-anthropic.md) · [Google Gemini API](vendor-gemini.md) · [Google Vertex AI](vendor-vertexai.md) · [Amazon Bedrock](vendor-bedrock.md) · [Ollama](vendor-ollama.md) · [ElevenLabs](vendor-elevenlabs.md)

This page is the human-readable view of [`gaise-core/model-registry.toml`](../gaise-core/model-registry.toml) (schema 2, audited **2026-08-20**), which is compiled into the `gaise` crate and applied as an overlay by [`list_models`](api.md#get-v1models). It is advisory: GAISe accepts arbitrary model IDs so new releases work before this file is updated, and the provider's own model API (see [Model discovery](capabilities.md#model-discovery)) is always the first source of truth.

The tables below are generated from the registry with [`cargo run -p gaise --example registry_json`](../gaise-core/examples/registry_json.rs). Columns:

- **Input / Output** — modalities classified from the entry's `capabilities` list by [`classify_capabilities`](../gaise-core/src/registry.rs).
- **Ops** — GAISe operations the entry maps to: `I` instruct, `S` instruct_stream, `E` embeddings, `V` speech (voice), `L` live. Empty means no GAISe surface drives the model (image generation, TTS, bidirectional audio).
- **Tools / Reasoning** — ✓ supported, ✗ not listed, and the `reasoning_values` the provider documents.
- **Dates** — `shutdown` is a published retirement date; `not before` is an availability guarantee. Gemini API and Vertex AI dates are **never** interchangeable.
- **Limits** — context windows, output ceilings, per-input token limits, and character budgets are not repeated here; see [limits.md](limits.md) for the generated model × limits matrix and `GET /v1/models/limits`.

## Contents


- [OpenAI](#openai) — 33 entries
- [Anthropic](#anthropic) — 15 entries
- [Google Gemini API](#gemini) — 24 entries
- [Google Vertex AI](#vertexai) — 19 entries
- [Amazon Bedrock](#bedrock) — 33 entries
- [Ollama](#ollama) — 16 entries
- [ElevenLabs](#elevenlabs) — 9 entries
- [Maintaining the registry](#maintaining-the-registry)
- [Lifecycle calendar](#lifecycle-calendar)

## openai

### OpenAI

Instruct uses **Chat Completions**; Responses-only models (GPT-5.5 Pro, gpt-5.6-cyber, image generation) are listed but cannot be driven. `GET /v1/models` reports identity only, so everything in the Input/Output/Ops columns is registry- or heuristic-sourced at runtime ([`catalog.rs`](../gaise-provider-openai/src/contracts/catalog.rs)).

- Vendor page: [vendor-openai.md](vendor-openai.md) · GAISe surface: Chat Completions, Embeddings, and Realtime
- Discovery: GET /v1/models (id, created, owned_by, shutdown_date only)
- Official catalog: <https://developers.openai.com/api/docs/models> · lifecycle: <https://developers.openai.com/api/docs/deprecations>
- The current instruct client uses Chat Completions. Responses-only features and the Images API are outside that client.

#### Current and preview

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `gpt-5.6` | `gpt-5.6-sol` | `active` | — | text, image | text | IS | ✓ | ✓ (none, low, medium, high, xhigh, max) | chat-compatible features — OpenAI documents gpt-5.6-sol as the snapshot ID and gpt-5.6 as the alias that rout… |
| `gpt-5.6-terra` | — | `active` | — | text, image | text | IS | ✓ | ✓ (none, low, medium, high, xhigh, max) | chat-compatible features — On Chat Completions, function tools require reasoning_effort='none'; the adapter a… |
| `gpt-5.6-luna` | — | `active` | — | text, image | text | IS | ✓ | ✓ (none, low, medium, high, xhigh, max) | chat-compatible features — On Chat Completions, function tools require reasoning_effort='none'; the adapter a… |
| `gpt-5.5` | — | `active` | — | text, image | text | IS | ✓ | ✓ (none, low, medium, high, xhigh) | chat-compatible features — Defaults to medium reasoning effort. |
| `gpt-5.5-pro` | — | `active` | — | text, image | text | — | ✓ | ✓ (medium, high, xhigh) | not reachable through the Chat Completions instruct client — OpenAI lists Chat Completions as not supported;… |
| `gpt-5.4` | — | `active` | — | text, image | text | IS | ✓ | ✓ (none, low, medium, high, xhigh) | chat-compatible features |
| `gpt-5.4-mini` | — | `active` | — | text, image | text | IS | ✓ | ✓ (none, low, medium, high, xhigh) | chat-compatible features |
| `gpt-5.4-nano` | — | `active` | — | text, image | text | IS | ✓ | ✓ (none, low, medium, high, xhigh) | chat-compatible features |
| `text-embedding-3-large` | — | `active` | — | text | embedding | E | ✗ | ✗ | native |
| `text-embedding-3-small` | — | `active` | — | text | embedding | E | ✗ | ✗ | native |
| `text-embedding-ada-002` | — | `active` | — | text | embedding | E | ✗ | ✗ | native — Previous generation; fixed 1536 dimensions, no retirement date published. |
| `gpt-realtime-2.1` | — | `active` | — | text, image, audio | text, audio | L | ✓ | ✓ (minimal, low, medium, high, xhigh) | realtime transport — OpenAI documents configurable reasoning effort without enumerating values for 2.1; the l… |
| `gpt-realtime-2.1-mini` | — | `active` | — | text, image, audio | text, audio | L | ✓ | ✓ (minimal, low, medium, high, xhigh) | realtime transport |
| `gpt-realtime-2` | — | `active` | — | text, image, audio | text, audio | L | ✓ | ✓ (minimal, low, medium, high, xhigh) | realtime transport |
| `gpt-realtime-1.5` | — | `active` | — | text, image, audio | text, audio | L | ✓ | ✗ | realtime transport — No reasoning controls. |
| `gpt-audio-1.5` | — | `active` | — | text, audio | text, audio | IS | ✓ | ✗ | Chat audio input is native; audio output is not mapped by the current instruct client — Chat Completions supp… |
| `gpt-image-2` | — | `active` | — | image | image | — | ✗ | ✗ | not yet native — Requires OpenAI Images or Responses image-generation tooling; the GAISe OpenAI instruct clie… |

#### Deprecated and legacy

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `gpt-5-2025-08-07` | — | `deprecated` | shutdown 2026-12-11 | — | — | — | ? | ? | — |
| `gpt-5-mini-2025-08-07` | — | `deprecated` | shutdown 2026-12-11 | — | — | — | ? | ? | — |
| `gpt-5-nano-2025-08-07` | — | `deprecated` | shutdown 2026-12-11 | — | — | — | ? | ? | — |
| `gpt-5-pro-2025-10-06` | — | `deprecated` | shutdown 2026-12-11 | — | — | — | ? | ? | — |
| `o3-2025-04-16` | — | `deprecated` | shutdown 2026-12-11 | — | — | — | ? | ? | — |
| `o3-pro-2025-06-10` | — | `deprecated` | shutdown 2026-12-11 | — | — | — | ? | ? | — |
| `o4-mini` | `o4-mini-2025-04-16` | `deprecated` | shutdown 2026-10-23 | — | — | — | ? | ? | — |
| `gpt-4.1-nano` | `gpt-4.1-nano-2025-04-14` | `deprecated` | shutdown 2026-10-23 | — | — | — | ? | ? | — |
| `gpt-image-1` | — | `deprecated` | shutdown 2026-10-23 | — | — | — | ? | ? | — |
| `gpt-image-1.5` | `gpt-image-1-mini`, `chatgpt-image-latest` | `deprecated` | shutdown 2026-12-01 | — | — | — | ? | ? | — |
| `gpt-realtime` | `gpt-4o-realtime`, `gpt-realtime-mini`, `gpt-4o-mini-realtime` | `deprecated` | shutdown 2027-01-20 | — | — | — | ? | ? | — |
| `gpt-audio` | `gpt-4o-audio`, `gpt-audio-mini`, `gpt-4o-mini-audio` | `deprecated` | shutdown 2027-01-20 | — | — | — | ? | ? | — |

#### Retired

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `gpt-5-chat-latest` | — | `retired` | shutdown 2026-07-23 | — | — | — | ? | ? | — |
| `gpt-5.1-chat-latest` | — | `retired` | shutdown 2026-07-23 | — | — | — | ? | ? | — |
| `gpt-5.2-chat-latest` | — | `retired` | shutdown 2026-08-10 | — | — | — | ? | ? | — |
| `gpt-5.3-chat-latest` | — | `retired` | shutdown 2026-08-10 | — | — | — | ? | ? | — |

## anthropic

### Anthropic

The Messages API. Anthropic's `GET /v1/models` reports image/PDF input, thinking types, effort levels, structured outputs, and token limits, so at runtime the registry contributes only lifecycle dates and notes ([`catalog.rs`](../gaise-provider-anthropic/src/contracts/catalog.rs)). Thinking and sampling rules by family are enforced in [`anthropic_client.rs`](../gaise-provider-anthropic/src/anthropic_client.rs).

- Vendor page: [vendor-anthropic.md](vendor-anthropic.md) · GAISe surface: Messages
- Discovery: GET /v1/models (capabilities: image_input, pdf_input, thinking types, effort levels, structured_outputs, token limits)
- Official catalog: <https://platform.claude.com/docs/en/about-claude/models/overview> · lifecycle: <https://platform.claude.com/docs/en/about-claude/model-deprecations>
- Direct Claude API lifecycle. Bedrock-hosted Claude has a separate AWS lifecycle.

#### Current and preview

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `claude-fable-5` | — | `active` | not before 2027-06-09 | text, image, file | text | IS | ✓ | ✓ adaptive (low, medium, high, xhigh, max) | native — Adaptive thinking is always on and cannot be disabled; only the effort level is configurable. Non-de… |
| `claude-mythos-5` | — | `limited_availability` | — | text, image, file | text | IS | ✓ | ✓ adaptive (low, medium, high, xhigh, max) | native when account access exists — Adaptive thinking is always on and cannot be disabled. |
| `claude-opus-5` | — | `active` | not before 2027-07-24 | text, image, file | text | IS | ✓ | ✓ adaptive (low, medium, high, xhigh, max) | native — Released 2026-07-24; Anthropic's recommended default model. Adaptive-only thinking, on by default (d… |
| `claude-opus-4-8` | — | `active` | not before 2027-05-28 | text, image, file | text | IS | ✓ | ✓ adaptive (low, medium, high, xhigh, max) | native |
| `claude-opus-4-7` | — | `active` | not before 2027-04-16 | text, image, file | text | IS | ✓ | ✓ adaptive (low, medium, high, xhigh, max) | native |
| `claude-opus-4-6` | — | `active` | not before 2027-02-05 | text, image, file | text | IS | ✓ | ✓ adaptive (low, medium, high, max) | native |
| `claude-opus-4-5-20251101` | `claude-opus-4-5` | `active` | not before 2026-11-24 | text, image, file | text | IS | ✓ | ✓ manual (low, medium, high) | native |
| `claude-sonnet-5` | — | `active` | not before 2027-06-30 | text, image, file | text | IS | ✓ | ✓ adaptive (low, medium, high, xhigh, max) | native |
| `claude-sonnet-4-6` | — | `active` | not before 2027-02-17 | text, image, file | text | IS | ✓ | ✓ adaptive (low, medium, high, max) | native |
| `claude-sonnet-4-5-20250929` | `claude-sonnet-4-5` | `active` | not before 2026-09-29 | text, image, file | text | IS | ✓ | ✓ manual | native |
| `claude-haiku-4-5-20251001` | `claude-haiku-4-5` | `active` | not before 2026-10-15 | text, image, file | text | IS | ✓ | ✓ manual | native — Manual thinking budget; no adaptive thinking or effort parameter. |

#### Deprecated and legacy

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `claude-mythos-preview` | — | `deprecated` | — | text, image, file | text | IS | ✓ | ✓ adaptive | Deprecated in favour of claude-mythos-5; no retirement date is published. Invitation-only (Project Glasswing). |

#### Retired

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `claude-opus-4-1-20250805` | — | `retired` | shutdown 2026-08-05 | — | — | — | ? | ? | — |
| `claude-opus-4-20250514` | — | `retired` | shutdown 2026-06-15 | — | — | — | ? | ? | — |
| `claude-sonnet-4-20250514` | — | `retired` | shutdown 2026-06-15 | — | — | — | ? | ? | — |

## gemini

### Google Gemini API

Google AI Gemini API lifecycle only — see [Vertex AI](#vertexai) for Google Cloud. `models.list` reports `supportedGenerationMethods` (→ Ops), `thinking`, and token limits but no modalities ([`catalog.rs`](../gaise-provider-gemini/src/contracts/catalog.rs)). Gemini 2.5 uses `thinkingBudget`, 3.x uses `thinkingLevel`.

- Vendor page: [vendor-gemini.md](vendor-gemini.md) · GAISe surface: Gemini generateContent, streamGenerateContent, Embeddings, and Live
- Discovery: GET /v1beta/models (supportedGenerationMethods, token limits, thinking flag; no modalities)
- Official catalog: <https://ai.google.dev/gemini-api/docs/models> · lifecycle: <https://ai.google.dev/gemini-api/docs/deprecations>
- Google AI Gemini API lifecycle; do not reuse these dates for Vertex AI. Shutdown dates on the deprecations page are the earliest possible retirement dates.

#### Current and preview

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `gemini-3.7-flash` | — | `stable` | — | text, image, audio, video, file | text | IS | ✓ | ✓ (low, medium, high) | native — GA 2026-08-13. thinkingLevel minimal is not supported (default medium). Live API not supported. |
| `gemini-3.6-flash` | — | `stable` | — | text, image, audio, video, file | text | IS | ✓ | ✓ (minimal, low, medium, high) | native — Released 2026-07-21. Fixed sampling: temperature, top_p, and top_k are deprecated (changelog 2026-07… |
| `gemini-3.5-flash` | — | `stable` | — | text, image, audio, video, file | text | IS | ✓ | ✓ (minimal, low, medium, high) | native |
| `gemini-3.5-flash-lite` | — | `stable` | — | text, image, audio, video, file | text | IS | ✓ | ✓ (minimal, low, medium, high) | native |
| `gemini-3.1-flash-lite` | — | `stable` | shutdown 2027-05-07 | text, image, audio, video, file | text | IS | ✓ | ✓ | native — Thinking is supported; the exact thinkingLevel set is not tabulated in the thinking guide. |
| `gemini-3.1-pro-preview` | — | `preview` | — | text, image, audio, video, file | text | IS | ✓ | ✓ (low, medium, high) | native — No shutdown date announced. A gemini-3.1-pro-preview-customtools variant endpoint exists. |
| `gemini-3-flash-preview` | — | `preview` | — | text, image, audio, video, file | text | IS | ✓ | ✓ (minimal, low, medium, high) | native — No shutdown date announced. |
| `gemini-3.1-flash-live-preview` | — | `preview` | — | text, image, audio, video | text, audio | L | ✓ | ✓ (minimal, low, medium, high) | native Live API transport — Released 2026-03-11; no shutdown date announced. |
| `gemini-2.5-flash-native-audio-preview-12-2025` | — | `preview_legacy` | — | text, audio, video | text, audio | L | ✓ | ✓ | Live transport; migration recommended — Live guide still documents thinkingBudget (0 disables) for this model. |
| `gemini-3.1-flash-image` | — | `stable` | — | text, image, file | text, image | IS | ✗ | ✓ (minimal, high) | native image output through generateContent — Released 2026-05-28. Function calling and structured outputs ar… |
| `gemini-3.1-flash-lite-image` | — | `stable` | — | text, image | text, image | IS | ✓ | ✓ (minimal, high) | native image output through generateContent — Released June 2026; the recommended image model (1K output only… |
| `gemini-3-pro-image` | — | `stable` | — | text, image | text, image | IS | ✗ | ✓ | native image output through generateContent — Released 2026-05-28. Function calling is not supported. |
| `gemini-embedding-2` | `gemini-embedding-2-preview` | `stable` | — | text, image, audio, video | embedding | E | ✗ | ✗ | native — Released 2026-04-22. Text, image, video, audio, and PDF input. The catalog table still shows the -pr… |
| `gemini-3.1-flash-tts-preview` | — | `preview` | — | text | text, audio | — | ✗ | ✗ | text-to-speech is outside the instruct surface — Released 2026-04-13; replacement for the 2.5 TTS previews. |
| `gemini-2.5-pro` | — | `stable` | — | text, image, audio, video, file | text | IS | ✓ | ✓ | native (thinkingBudget mapper path) — No shutdown date announced on the Gemini API deprecations page; the 202… |
| `gemini-2.5-flash` | — | `stable` | — | text, image, audio, video, file | text | IS | ✓ | ✓ | native (thinkingBudget mapper path) — No shutdown date announced on the Gemini API deprecations page. |
| `gemini-2.5-flash-lite` | — | `stable` | — | text, image, audio, video, file | text | IS | ✓ | ✓ | native (thinkingBudget mapper path) — Released 2025-07-22; thinking is off by default. No shutdown date annou… |

#### Deprecated and legacy

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `gemini-2.5-flash-image` | — | `deprecated` | shutdown 2026-10-02 | — | — | — | ? | ? | The deprecations page names gemini-3.1-flash-image-preview (itself shut down 2026-06-25); the GA replacements… |
| `gemini-embedding-001` | — | `deprecated` | shutdown 2028-05-14 | text | embedding | E | ✗ | ✗ | native — Still listed as callable; the earlier registry date (2026-07-14) was the release date. |

#### Retired

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `embedding-2-preview` | — | `retired` | shutdown 2026-08-10 | — | — | — | ? | ? | — |
| `gemini-2.0-flash` | — | `retired` | shutdown 2026-06-01 | — | — | — | ? | ? | — |
| `gemini-2.0-flash-lite` | — | `retired` | shutdown 2026-06-01 | — | — | — | ? | ? | — |
| `gemini-2.0-flash-live-001` | — | `retired` | shutdown 2025-12-09 | — | — | — | ? | ? | — |
| `gemini-live-2.5-flash-preview` | — | `retired` | shutdown 2025-12-09 | — | — | — | ? | ? | — |

## vertexai

### Google Vertex AI

Google Cloud lifecycle only. Model Garden listing returns names, versions, and launch stages — no modalities or limits ([`catalog.rs`](../gaise-provider-vertexai/src/contracts/catalog.rs)). Short-term-availability models retire 45 days after a designated replacement ships.

- Vendor page: [vendor-vertexai.md](vendor-vertexai.md) · GAISe surface: Vertex AI generateContent, streamGenerateContent, and Embeddings
- Discovery: GET {region}-aiplatform.googleapis.com/v1beta1/publishers/{publisher}/models (names, versionId, launchStage; no modalities or limits)
- Official catalog: <https://docs.cloud.google.com/vertex-ai/generative-ai/docs/learn/models> · lifecycle: <https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/model-versions>
- Model availability and retirement dates are specific to Google Cloud. Short-term availability models retire 45 days after a replacement is released.

#### Current and preview

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `gemini-3.7-flash` | — | `short_term_active` | — | text, image, audio, video, file | text | IS | ✓ | ✓ | native — GA 2026-08-13; short-term availability table, no retirement date announced. |
| `gemini-3.6-flash` | — | `short_term_active` | — | text, image, audio, video, file | text | IS | ✓ | ✓ | native — Released 2026-07-21; short-term availability (retires 45 days after a designated replacement). gemin… |
| `gemini-3.5-flash` | — | `active` | not before 2027-05-19 | text, image, audio, video, file | text | IS | ✓ | ✓ | native |
| `gemini-3.5-flash-lite` | — | `active` | not before 2027-07-21 | text, image, audio, video, file | text | IS | ✓ | ✓ | native |
| `gemini-3.1-flash-lite` | — | `active` | not before 2027-05-07 | text, image, audio, video, file | text | IS | ✓ | ✓ | native — GA 2026-05-07. |
| `gemini-3-flash-preview` | — | `preview` | — | text, image, audio, video, file | text | IS | ✓ | ✓ | native — Public preview since 2025-12-17; no retirement row on the model-versions page. |
| `gemini-3.1-flash-image` | — | `active` | not before 2027-05-28 | text, image, video, file | text, image | IS | ✗ | ✓ | native image output through generateContent — Video input remains in preview. Function calling is not support… |
| `gemini-3.1-flash-lite-image` | — | `short_term_active` | — | text, image, video, file | text, image | IS | ✗ | ✓ | native image output through generateContent — GA 2026-06-30; no retirement date announced. Function calling i… |
| `gemini-3-pro-image` | — | `active` | not before 2027-05-28 | text, image, file | text, image | IS | ✗ | ✓ | native image output through generateContent — Function calling is not supported; video input is not supported… |
| `gemini-embedding-2` | `gemini-embedding-2-preview` | `active` | — | text, image, audio, video | embedding | — | ✗ | ✗ | not yet: Vertex serves it via :embedContent on the aiplatform.{location}.rep.googleapis.com host, which the a… |
| `gemini-embedding-001` | — | `active` | not before 2028-05-20 | text | embedding | E | ✗ | ✗ | native — Previous-generation text embedding model; gemini-embedding-2 is current. Accepts one input text per… |
| `text-embedding-005` | `text-embedding-004`, `text-multilingual-embedding-002` | `active` | not before 2027-04-01 | text | embedding | E | ✗ | ✗ | native — Legacy text embedding family; all retire 2027-04-01. 768 dimensions, 2,048 tokens, 250 texts per cal… |
| `multimodalembedding@001` | — | `active` | not before 2027-04-01 | text, image, audio, video | embedding | — | ✗ | ✗ | not yet: uses the image/video embedding request schema — 1408 dimensions (128/256/512 for text+image); 32 tex… |
| `gemini-live-2.5-flash-native-audio` | — | `active` | shutdown 2026-12-13 | text, image, audio, video | text, audio | — | ✓ | ✗ | not supported: the Vertex AI adapter has no Live transport — GA 2025-12-12. |

#### Deprecated and legacy

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `gemini-2.5-pro` | — | `deprecated` | shutdown 2026-10-20 | text, image, audio, video, file | text | IS | ✓ | ✓ | native (thinkingBudget mapper path) |
| `gemini-2.5-flash` | — | `deprecated` | shutdown 2026-10-20 | text, image, audio, video, file | text | IS | ✓ | ✓ | native (thinkingBudget mapper path) |
| `gemini-2.5-flash-lite` | — | `deprecated` | shutdown 2026-10-20 | text, image, audio, video, file | text | IS | ✓ | ✓ | native (thinkingBudget mapper path) |
| `gemini-2.5-flash-image` | — | `deprecated` | shutdown 2026-10-02 | text, image | text, image | IS | ✗ | ✗ | — |

#### Retired

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `gemini-2.0-flash` | `gemini-2.0-flash-lite` | `retired` | shutdown 2026-06-01 | — | — | — | ? | ? | — |

## bedrock

### Amazon Bedrock

Model IDs, inference profiles, and lifecycle are **region-specific**; entries are representative and `ListFoundationModels` / `ListInferenceProfiles` are authoritative ([`catalog.rs`](../gaise-provider-bedrock/src/catalog.rs)). The registry matcher strips `us.`/`eu.`/`apac.`/`ap.`/`jp.`/`au.`/`ca.`/`il.`/`global.`/`us-gov.` profile prefixes and `-vN:M` suffixes, and `*` entries are family globs.

- Vendor page: [vendor-bedrock.md](vendor-bedrock.md) · GAISe surface: Converse, ConverseStream, and InvokeModel
- Discovery: ListFoundationModels, GetFoundationModel, and ListInferenceProfiles
- Official catalog: <https://docs.aws.amazon.com/bedrock/latest/userguide/model-cards.html> · lifecycle: <https://docs.aws.amazon.com/bedrock/latest/userguide/model-lifecycle.html>
- Model IDs, inference profiles, lifecycle, and availability vary by AWS region. AWS replaced the Converse feature matrix with per-model cards (model-card-*.html) and models-api-compatibility.html in mid-2026.

#### Current and preview

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `anthropic.claude-opus-5` | — | `active` | — | text, image, file | text | IS | ✓ | ✓ adaptive (low, medium, high, xhigh, max) | native via Converse — Launched 2026-07-24. Profiles: us., eu., au., global. Adaptive thinking on by default;… |
| `anthropic.claude-fable-5` | — | `active` | — | text, image, file | text | IS | ✓ | ✓ adaptive (low, medium, high, xhigh, max) | native via Converse — Launched 2026-06-09; in-region us-east-1 only, geo profile us. only. Requires the provi… |
| `anthropic.claude-opus-4-8` | — | `active` | — | text, image, file | text | IS | ✓ | ✓ adaptive (low, medium, high, xhigh, max) | native via Converse — Launched 2026-05-28. Profiles: us., eu., jp., au. Prompt-cache minimum 4,096 tokens. |
| `anthropic.claude-sonnet-5` | — | `active` | — | text, image, file | text | IS | ✓ | ✓ adaptive (low, medium, high, xhigh, max) | native via Converse — Launched 2026-06-30. Fixed sampling as for Fable 5. |
| `anthropic.claude-opus-4-7` | — | `active` | — | text, image, file | text | IS | ✓ | ✓ adaptive (low, medium, high, xhigh, max) | native via Converse — Launched 2026-04-16. Profiles: us., eu., jp., au., global. thinking.type adaptive only;… |
| `anthropic.claude-sonnet-4-6` | — | `active` | — | text, image, file | text | IS | ✓ | ✓ adaptive (low, medium, high, max) | native via Converse — Launched 2026-02-17. Profiles: us., eu., au., jp., global. Structured outputs supported. |
| `anthropic.claude-opus-4-6-v1` | — | `active` | — | text, image, file | text | IS | ✓ | ✓ adaptive (low, medium, high, max) | native via Converse — Launched 2026-02-05. Profiles: us., eu., au., global. |
| `anthropic.claude-opus-4-5-20251101-v1:0` | — | `active` | — | text, image, file | text | IS | ✓ | ✓ manual (low, medium, high) | native via Converse — Profiles: us., eu., global. EOL floor (2026-03-25) has passed without a Legacy announce… |
| `anthropic.claude-sonnet-4-5-20250929-v1:0` | — | `active` | not before 2026-09-29 | text, image, file | text | IS | ✓ | ✓ manual | native via Converse — Profiles: us., eu., au., jp., global. |
| `anthropic.claude-haiku-4-5-20251001-v1:0` | — | `active` | not before 2026-10-01 | text, image, file | text | IS | ✓ | ✓ manual | native via Converse |
| `anthropic.claude-mythos-5` | — | `limited_availability` | — | text, image, file | text | — | ✓ | ✓ adaptive | not reachable: Messages API on bedrock-mantle only; Converse and InvokeModel are not supported — us-east-1 pr… |
| `amazon.nova-2-lite-v1:0` | — | `active` | — | text, image, video, file | text | IS | ✓ | ✗ | native via Converse — The only Converse-capable Nova 2 model. Client-side tool calling supported; structured… |
| `amazon.nova-2-sonic-v1:0` | — | `active` | — | text, audio | text, audio | — | ✓ | ✗ | not supported: InvokeModelWithBidirectionalStream only |
| `amazon.nova-2-multimodal-embeddings-v1:0` | — | `active` | — | text, image, audio, video | embedding | E | ✗ | ✗ | text embeddings via InvokeModel (SINGLE_EMBEDDING schema); image/audio/video input not mapped — us-east-1 and… |
| `amazon.nova-pro-v1:0` | — | `active` | — | text, image, video, file | text | IS | ✓ | ✗ | native via Converse — Bedrock model card: 300K context, 5K max output (the Nova user guide's spec table says 10K; the request schema… |
| `amazon.nova-lite-v1:0` | — | `active` | — | text, image, video, file | text | IS | ✓ | ✗ | native via Converse — Bedrock model card: 300K context, 5K max output (the Nova user guide's spec table says 10K; the request schema… |
| `amazon.nova-micro-v1:0` | — | `active` | — | text | text | IS | ✓ | ✗ | native via Converse — Text only. Bedrock model card: 128K context, 5K max output (the Nova user guide's spec table says 10K; the requ… |
| `amazon.nova-*` | — | `dynamic_active_family` | — | text, image, video, file | text | IS | ✓ | ✗ | Converse where the regional catalog lists the model — Nova Pro/Lite/Micro (v1) remain Active with regional us… |
| `amazon.titan-embed-text-v2:0` | — | `active` | — | text | embedding | E | ✗ | ✗ | native via InvokeModel — In-region only. 8,192 tokens / 50,000 characters per text; one text per call; `norma… |
| `amazon.titan-embed-text-v1` | — | `active` | — | text | embedding | E | ✗ | ✗ | native via InvokeModel — First-generation Titan text embeddings; fixed 1536 dimensions, one text per call. |
| `amazon.titan-embed-image-v1` | — | `active` | — | text, image, audio, video | embedding | E | ✗ | ✗ | text input via InvokeModel; no image path — Text + image embeddings in one space; 256 text tokens, 25 MB imag… |
| `amazon.titan-embed-*` | — | `dynamic_active_family` | — | text | embedding | E | ✗ | ✗ | native via InvokeModel — Catch-all for Titan embedding ids not listed above; verify with ListFoundationModels. |
| `cohere.embed-v4:0` | — | `active` | — | text, image, audio, video | embedding | E | ✗ | ✗ | native via InvokeModel — Launched 2025-04-15; text and image input; profiles us., eu., global. `input_type` i… |
| `cohere.embed-english-v3` | `cohere.embed-multilingual-v3` | `active` | — | text | embedding | E | ✗ | ✗ | native via InvokeModel — Fixed 1024 dimensions, 512 tokens per text, 96 texts per call; `input_type` is requi… |
| `cohere.embed-*` | — | `dynamic_active_family` | — | text | embedding | E | ✗ | ✗ | native via InvokeModel — Catch-all for other Cohere embed ids; `input_type` is always required. |

#### Deprecated and legacy

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `anthropic.claude-opus-4-1-20250805-v1:0` | — | `legacy` | shutdown 2027-01-08 | — | — | — | ? | ? | Legacy since 2026-07-08; public extended access (higher pricing) from 2026-10-08. us. profile only. |
| `anthropic.claude-sonnet-4-20250514-v1:0` | — | `legacy` | shutdown 2026-10-14 | — | — | — | ? | ? | Legacy since 2026-04-14; extended-access pricing applies since 2026-07-14. |
| `anthropic.claude-3-haiku-20240307-v1:0` | — | `legacy` | shutdown 2026-09-10 | — | — | — | ? | ? | — |
| `amazon.nova-premier-v1:0` | — | `legacy` | shutdown 2026-09-14 | — | — | — | ? | ? | — |
| `amazon.nova-sonic-v1:0` | — | `legacy` | shutdown 2026-09-14 | — | — | — | ? | ? | — |
| `amazon.nova-reel-v1:*` | — | `legacy` | shutdown 2026-09-30 | — | — | — | ? | ? | — |
| `amazon.nova-canvas-v1:0` | — | `legacy` | shutdown 2026-09-30 | — | — | — | ? | ? | — |

#### Retired

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `cohere.command-r-*` | — | `retired` | shutdown 2026-08-19 | — | — | — | ? | ? | cohere.command-r-v1:0 and cohere.command-r-plus-v1:0. |

## ollama

### Ollama

The installed catalog is dynamic (`GET /api/tags`); entries are family globs describing typical capabilities. `POST /api/show` (opt-in `include_details`) reports the real capabilities of each installed tag ([`catalog.rs`](../gaise-provider-ollama/src/contracts/catalog.rs)).

- Vendor page: [vendor-ollama.md](vendor-ollama.md) · GAISe surface: Chat, streaming chat, and Embeddings
- Discovery: GET /api/tags
- Official catalog: <https://ollama.com/search> · lifecycle: <dynamic local catalog>
- Tags are installed locally and can move; Ollama has no centralized retirement calendar.

#### Current and preview

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `qwen3:*` | — | `dynamic_local` | — | text | text | IS | ✓ | ✓ (true, false) | native |
| `gpt-oss:*` | — | `dynamic_local` | — | text | text | IS | ✓ | ✓ (low, medium, high) | native |
| `deepseek-r1:*` | `deepseek-v3.1:*` | `dynamic_local` | — | text | text | IS | ✗ | ✓ (true, false) | native |
| `gemma4:*` | — | `dynamic_local` | — | text, image | text | IS | ✗ | ✗ | native when the installed tag advertises vision |
| `embeddinggemma:*` | — | `dynamic_local` | — | text | embedding | E | ✗ | ✗ | native — EmbeddingGemma 300m; Matryoshka 768/512/256/128; 2,048-token context; Google prompt-instruction conv… |
| `nomic-embed-text:*` | — | `dynamic_local` | — | text | embedding | E | ✗ | ✗ | native — nomic-embed-text v1.5; Matryoshka 64-768; 8,192-token context (raise num_ctx); prefixes are required… |
| `nomic-embed-text-v2-moe:*` | — | `dynamic_local` | — | text | embedding | E | ✗ | ✗ | native — Multilingual MoE; Matryoshka 256-768; 512-token context. |
| `qwen3-embedding:*` | — | `dynamic_local` | — | text | embedding | E | ✗ | ✗ | native — 0.6b/4b/8b = 1024/2560/4096 dimensions (Matryoshka 32-4096); 32k context; queries take an instructio… |
| `mxbai-embed-large:*` | — | `dynamic_local` | — | text | embedding | E | ✗ | ✗ | native — mixedbread mxbai-embed-large-v1; fixed 1024; 512-token context; queries take an instruction. |
| `bge-m3:*` | — | `dynamic_local` | — | text | embedding | E | ✗ | ✗ | native — BAAI bge-m3; fixed 1024; 8,192-token context; multilingual; no prefix. |
| `bge-large:*` | — | `dynamic_local` | — | text | embedding | E | ✗ | ✗ | native — BAAI bge-large-en-v1.5; fixed 1024; 512-token context; optional query instruction. |
| `all-minilm:*` | — | `dynamic_local` | — | text | embedding | E | ✗ | ✗ | native — all-MiniLM-L6/L12; fixed 384; 256-token context; English. |
| `snowflake-arctic-embed:*` | — | `dynamic_local` | — | text | embedding | E | ✗ | ✗ | native — Arctic-embed v1 22m-335m; 384-1024 dimensions by tag; 512-token context; queries take an instruction. |
| `snowflake-arctic-embed2:*` | — | `dynamic_local` | — | text | embedding | E | ✗ | ✗ | native — Arctic-embed 2.0; 1024 dimensions (Matryoshka to 256); 8,192-token context; multilingual; queries ta… |
| `granite-embedding:*` | — | `dynamic_local` | — | text | embedding | E | ✗ | ✗ | native — IBM Granite 30m (384, English) / 278m (768, 12 languages); 512-token context; no prefix. |
| `paraphrase-multilingual:*` | — | `dynamic_local` | — | text | embedding | E | ✗ | ✗ | native — paraphrase-multilingual-MiniLM-L12-v2; fixed 768; 128-token context; 50+ languages. |

## elevenlabs

### ElevenLabs

Text-to-speech and realtime voice. `GET /v1/models` reports model ids, languages, `can_do_text_to_speech`, style/speaker-boost support, and per-request character limits ([`models.rs`](../gaise-provider-elevenlabs/src/contracts/models.rs)). Voices are account-specific (`GET /v2/voices`) and ElevenLabs default voices expire 2026-12-31, so no voice is hard-coded. `eleven_v3*` realtime goes through the text-to-dialogue WebSocket; other models use `stream-input`.

- Vendor page: [vendor-elevenlabs.md](vendor-elevenlabs.md) · GAISe surface: Text-to-speech, streaming speech, realtime voice WebSocket, and model listing
- Discovery: GET /v1/models (model_id, languages, can_do_text_to_speech, can_use_style, character limits); GET /v2/voices for voices
- Official catalog: <https://elevenlabs.io/docs/overview/models> · lifecycle: <https://elevenlabs.io/docs/overview/models>
- Voices are account-specific and default voices expire 2026-12-31; never hard-code a voice id. Character limits per request are model-specific; the character-cost header reports billing.

#### Current and preview

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `eleven_v3` | — | `active` | — | text | audio | VL | ✗ | ✗ | speech via /v1/text-to-speech; realtime via the text-to-dialogue WebSocket — Flagship, 70+ languages, 5,000 c… |
| `eleven_v3_conversational` | — | `active` | — | text | text, audio | L | ✗ | ✗ | realtime only via the text-to-dialogue WebSocket (one voice) — ~280 ms latency variant of v3 for realtime use. |
| `eleven_multilingual_v2` | — | `active` | — | text | audio | VL | ✗ | ✗ | native — Default model; 29 languages; 10,000 characters per request. Rejects language_code (the adapter omits… |
| `eleven_flash_v2_5` | — | `active` | — | text | audio | VL | ✗ | ✗ | native — ~75 ms latency, 32 languages, 40,000 characters per request, accepts language_code. Numbers are not… |
| `eleven_flash_v2` | — | `active` | — | text | audio | VL | ✗ | ✗ | native — English only; 30,000 characters per request. |
| `eleven_multilingual_sts_v2` | `eleven_english_sts_v2` | `active` | — | audio | audio | — | ✗ | ✗ | not supported: speech-to-speech has no GAISe surface |
| `scribe_v2` | `scribe_v2_realtime` | `active` | — | text, audio | text | — | ✗ | ✗ | not supported: speech-to-text has no GAISe surface yet — scribe_v1 is deprecated. |

#### Deprecated and legacy

| Model | Aliases | Status | Dates | Input | Output | Ops | Tools | Reasoning | GAISe support / notes |
|---|---|---|---|---|---|---|---|---|---|
| `eleven_turbo_v2_5` | — | `deprecated` | — | text | audio | VL | ✗ | ✗ | native while available — Functionally equivalent to eleven_flash_v2_5; no shutdown date published. |
| `eleven_turbo_v2` | — | `deprecated` | — | text | audio | VL | ✗ | ✗ | native while available |

## Lifecycle calendar

Published shutdown dates for entries that are not yet retired, soonest first. Treat them as the **earliest** possible date; providers may extend but not advance them.

| Date | Provider | Model | Replacement |
|---|---|---|---|
| 2026-09-10 | [bedrock](#bedrock) | `anthropic.claude-3-haiku-20240307-v1:0` | anthropic.claude-haiku-4-5-20251001-v1:0 |
| 2026-09-14 | [bedrock](#bedrock) | `amazon.nova-premier-v1:0` | amazon.nova-2-lite-v1:0 |
| 2026-09-14 | [bedrock](#bedrock) | `amazon.nova-sonic-v1:0` | amazon.nova-2-sonic-v1:0 |
| 2026-09-30 | [bedrock](#bedrock) | `amazon.nova-canvas-v1:0` | — |
| 2026-09-30 | [bedrock](#bedrock) | `amazon.nova-reel-v1:*` | — |
| 2026-10-02 | [gemini](#gemini) | `gemini-2.5-flash-image` | gemini-3.1-flash-image |
| 2026-10-02 | [vertexai](#vertexai) | `gemini-2.5-flash-image` | gemini-3.1-flash-lite-image |
| 2026-10-14 | [bedrock](#bedrock) | `anthropic.claude-sonnet-4-20250514-v1:0` | anthropic.claude-sonnet-5 |
| 2026-10-20 | [vertexai](#vertexai) | `gemini-2.5-flash` | gemini-3.5-flash-lite or gemini-3.1-flash-lite |
| 2026-10-20 | [vertexai](#vertexai) | `gemini-2.5-flash-lite` | gemini-3.1-flash-lite or Gemma 4 |
| 2026-10-20 | [vertexai](#vertexai) | `gemini-2.5-pro` | gemini-3.5-flash |
| 2026-10-23 | [openai](#openai) | `gpt-4.1-nano` | gpt-5.6-luna |
| 2026-10-23 | [openai](#openai) | `gpt-image-1` | gpt-image-2 |
| 2026-10-23 | [openai](#openai) | `o4-mini` | gpt-5.6-terra |
| 2026-12-01 | [openai](#openai) | `gpt-image-1.5` | gpt-image-2 |
| 2026-12-11 | [openai](#openai) | `gpt-5-2025-08-07` | gpt-5.6-sol |
| 2026-12-11 | [openai](#openai) | `gpt-5-mini-2025-08-07` | gpt-5.6-terra |
| 2026-12-11 | [openai](#openai) | `gpt-5-nano-2025-08-07` | gpt-5.6-luna |
| 2026-12-11 | [openai](#openai) | `gpt-5-pro-2025-10-06` | gpt-5.6-sol (reasoning.mode: pro) |
| 2026-12-11 | [openai](#openai) | `o3-2025-04-16` | gpt-5.6-sol |
| 2026-12-11 | [openai](#openai) | `o3-pro-2025-06-10` | gpt-5.6-sol (reasoning.mode: pro) |
| 2026-12-13 | [vertexai](#vertexai) | `gemini-live-2.5-flash-native-audio` | — |
| 2027-01-08 | [bedrock](#bedrock) | `anthropic.claude-opus-4-1-20250805-v1:0` | anthropic.claude-opus-4-8 |
| 2027-01-20 | [openai](#openai) | `gpt-audio` | gpt-audio-1.5 |
| 2027-01-20 | [openai](#openai) | `gpt-realtime` | gpt-realtime-2.1 or gpt-realtime-2.1-mini |
| 2027-05-07 | [gemini](#gemini) | `gemini-3.1-flash-lite` | gemini-3.5-flash-lite |
| 2028-05-14 | [gemini](#gemini) | `gemini-embedding-001` | gemini-embedding-2 |

## Maintaining the registry

1. Verify against the official catalog and lifecycle pages linked above — never aggregators, and never the *other* Google surface.
2. Edit [`gaise-core/model-registry.toml`](../gaise-core/model-registry.toml) using only the closed vocabulary in its header. `cargo test -p gaise --lib registry` fails on unknown terms, unmapped statuses, or broken lookups ([`registry.rs` tests](../gaise-core/src/registry.rs)).
3. Separate lifecycle (`status`, `shutdown_date`, `retirement_not_before`, `replacement`) from adapter support (`gaise_support`).
4. When a family needs a new control mapping (thinking type, fixed sampling, tools rule), add a hermetic request-shape test in the provider crate and update [capabilities.md](capabilities.md).
5. Bump `audited_on`, regenerate this page (`cargo run -p gaise --example registry_json`), refresh the [vendor pages](README.md#vendors), and append to the audit report.
6. Never validate lifecycle by making a billable inference request.

The `.claude/agents/model-audit.md` agent definition automates steps 1–2 for coding agents.
