# Reasoning effort

> Part of the [GAISe wiki](README.md) · [Capabilities](capabilities.md) · [Models](models.md) · [HTTP API](api.md) · [Rust SDK](sdk.md) · Vendors: [OpenAI](vendor-openai.md) · [Anthropic](vendor-anthropic.md) · [Gemini](vendor-gemini.md) · [Vertex AI](vendor-vertexai.md) · [Bedrock](vendor-bedrock.md) · [Ollama](vendor-ollama.md)

Every vendor names its thinking controls differently (`reasoning_effort`, `output_config.effort` + `thinking.type`, `thinkingLevel`, `thinkingBudget`, `think`, `reasoningConfig`), accepts a different set of levels, and changes that set per model family. GAISe exposes **one vocabulary** — `GaiseGenerationConfig.thinking_effort` — and every adapter resolves it against the target family with the same rules, implemented once in [`GaiseReasoningEffort`](../gaise-core/src/contracts/gaise_reasoning.rs).

## Contents

- [Canonical levels and aliases](#canonical-levels-and-aliases)
- [Resolution rules](#resolution-rules)
- [Vendor mapping](#vendor-mapping)
- [Model × level matrix](#model--level-matrix)
- [Token budgets](#token-budgets)
- [Keeping this page current](#keeping-this-page-current)

## Canonical levels and aliases

The ladder, lowest to highest. Names are case-insensitive; aliases are accepted anywhere `thinking_effort` is.

| Canonical | Aliases | Meaning |
|---|---|---|
| `none` | `off`, `disabled`, `disable`, `false`, `0`, `no` | Turn reasoning off where the family allows it |
| `auto` | `default`, `adaptive`, `on`, `true`, `enabled`, `yes`, `dynamic` | Reasoning on; the provider chooses the depth (not on the ladder) |
| `minimal` | `min`, `lowest`, `very_low` | Smallest non-zero effort |
| `low` | — | |
| `medium` | `med`, `mid`, `moderate`, `standard`, `balanced` | |
| `high` | — | |
| `xhigh` | `x-high`, `x_high`, `extra_high`, `extra-high`, `very_high`, `very-high`, `xl` | |
| `max` | `maximum`, `xxhigh` | The vendor level literally called `max` where one exists |
| `ultra` | `ultracode`, `ultrathink`, `highest`, `extreme`, `unlimited` | **The highest level the model supports, whatever it is called.** Never sent verbatim |

Anything else is a **custom** value and is forwarded to the provider unchanged (upper-cased for Gemini/Vertex), so a brand-new vendor level can be used before GAISe learns its name. Source: [`GaiseReasoningEffort::parse`](../gaise-core/src/contracts/gaise_reasoning.rs).

## Resolution rules

Applied by every adapter, in this order ([`clamp_to`](../gaise-core/src/contracts/gaise_reasoning.rs)):

1. **Exact match** — if the family accepts the level, it is sent as-is (in the vendor's spelling).
2. **`ultra`** — resolves to the family's highest accepted level; on an unknown family it becomes `max`.
3. **Nearest level** — an unsupported level snaps to the nearest accepted rank; ties resolve upward (`minimal` on a family with `none, low, …` becomes `low`; `xhigh` on a family with `…, high, max` becomes `max`).
4. **`none` where thinking cannot be disabled** — becomes the lowest accepted level (Gemini 3.x) or the thinking block is omitted entirely (always-on Claude Fable/Mythos).
5. **`auto`** — enables thinking and lets the provider choose: `reasoning_effort` omitted (OpenAI), `thinking: adaptive` without effort (Claude adaptive families), `thinking: enabled` with the default 4,096 budget (Claude manual families), a thinking block without a level (Gemini 3.x), `thinkingBudget: -1` (Gemini 2.5), `think: true` (Ollama).
6. **Budget-only families** — a level on a family whose only control is a token budget is approximated: `minimal` 1,024 · `low` 2,048 · `medium` 4,096 · `high` 8,192 · `xhigh` 16,384 · `max`/`ultra` family maximum (Claude 4.5 and older; Gemini 2.5 uses its own ranges — see [Token budgets](#token-budgets)). An explicit `thinking_tokens` always wins over an approximation.
7. **Custom values** pass through unchanged; families without any effort control drop them.
8. **Non-reasoning families** never receive an effort field at all (OpenAI GPT-4.1/4o/chat-latest/audio).

These rules are pinned by each provider's `parameter_matrix_tests` and by the matrix below, which is generated from the real request builders.

## Vendor mapping

What each canonical level becomes on the wire, per control type. Family-specific clamping (rule 3) is shown in the [matrix](#model--level-matrix).

| Canonical | OpenAI Chat `reasoning_effort` | OpenAI Realtime `reasoning.effort` | Anthropic / Bedrock Claude (`thinking` + `output_config.effort`) | Gemini / Vertex 3.x `thinkingLevel` | Gemini / Vertex 2.5 `thinkingBudget` | Bedrock Nova `reasoningConfig.maxReasoningEffort` | Ollama `think` |
|---|---|---|---|---|---|---|---|
| `none` | `none` (or lowest level where `none` is absent) | `minimal` | `thinking: disabled` (omitted on always-on families) | lowest level (cannot disable) | `0` (Pro: `128`, cannot disable) | `none` → sent as-is | `false` |
| `auto` | omitted | omitted | `adaptive` / `enabled (4096)` without effort | thinking block, no level | `-1` (dynamic) | `auto` → sent as-is | `true` |
| `minimal` | `minimal` where present, else `low` | `minimal` | `low` (+ budget 1,024 on manual families) | `MINIMAL` where present, else `LOW` | `512` | `minimal` → sent as-is | GPT-OSS `low`; others `true` |
| `low` | `low` | `low` | `low` | `LOW` | `2048` | `low` | GPT-OSS `low` |
| `medium` | `medium` | `medium` | `medium` | `MEDIUM` (image models: `HIGH`) | `8192` | `medium` | GPT-OSS `medium` |
| `high` | `high` | `high` | `high` | `HIGH` | `24576` (Pro cap 32,768) | `high` | GPT-OSS `high` |
| `xhigh` | `xhigh` where present, else `high` | `xhigh` | `xhigh` where present, else `max`/`high` | `HIGH` | family maximum | `high`-like → sent as-is | GPT-OSS `high` |
| `max` | `max` where present, else `xhigh`/`high` | `xhigh` | `max` where present, else `high` | `HIGH` | family maximum | sent as-is | GPT-OSS `high` |
| `ultra` | family top (`max`, `xhigh`, or `high`) | `xhigh` | family top (`max` or `high`) | family top (`HIGH`) | family maximum | sent as-is | GPT-OSS `high`; others `true` |
| custom | forwarded | forwarded | forwarded (families with effort control) | forwarded upper-cased | ignored | forwarded | GPT-OSS forwarded; others `true` |

Bedrock Nova forwards the canonical name because AWS documents `low`/`medium`/`high` only; other names surface as provider validation errors rather than silent substitutions.

## Model × level matrix

Generated from the bundled registry and the real adapters — each cell is what GAISe sends for `thinking_effort: <level>` with no explicit `thinking_tokens` and `max_tokens: 8192`. Models without reasoning support and retired models are omitted; `*` rows are Ollama family globs. Responses-only OpenAI models cannot be reached through Chat Completions.

<!-- Generated by `cargo run -p gaise-client --example reasoning_matrix --all-features`; registry audited 2026-08-20 -->

### OpenAI (Chat Completions)

| Model | `none` | `auto` | `minimal` | `low` | `medium` | `high` | `xhigh` | `max` | `ultra` |
|---|---|---|---|---|---|---|---|---|---|
| `gpt-5.6` | `reasoning_effort: none` | omitted (model default) | `reasoning_effort: low` | `reasoning_effort: low` | `reasoning_effort: medium` | `reasoning_effort: high` | `reasoning_effort: xhigh` | `reasoning_effort: max` | `reasoning_effort: max` |
| `gpt-5.6-terra` | `reasoning_effort: none` | omitted (model default) | `reasoning_effort: low` | `reasoning_effort: low` | `reasoning_effort: medium` | `reasoning_effort: high` | `reasoning_effort: xhigh` | `reasoning_effort: max` | `reasoning_effort: max` |
| `gpt-5.6-luna` | `reasoning_effort: none` | omitted (model default) | `reasoning_effort: low` | `reasoning_effort: low` | `reasoning_effort: medium` | `reasoning_effort: high` | `reasoning_effort: xhigh` | `reasoning_effort: max` | `reasoning_effort: max` |
| `gpt-5.5` | `reasoning_effort: none` | omitted (model default) | `reasoning_effort: low` | `reasoning_effort: low` | `reasoning_effort: medium` | `reasoning_effort: high` | `reasoning_effort: xhigh` | `reasoning_effort: xhigh` | `reasoning_effort: xhigh` |
| `gpt-5.5-pro` | — (Responses-only) | — (Responses-only) | — (Responses-only) | — (Responses-only) | — (Responses-only) | — (Responses-only) | — (Responses-only) | — (Responses-only) | — (Responses-only) |
| `gpt-5.4` | `reasoning_effort: none` | omitted (model default) | `reasoning_effort: low` | `reasoning_effort: low` | `reasoning_effort: medium` | `reasoning_effort: high` | `reasoning_effort: xhigh` | `reasoning_effort: xhigh` | `reasoning_effort: xhigh` |
| `gpt-5.4-mini` | `reasoning_effort: none` | omitted (model default) | `reasoning_effort: low` | `reasoning_effort: low` | `reasoning_effort: medium` | `reasoning_effort: high` | `reasoning_effort: xhigh` | `reasoning_effort: xhigh` | `reasoning_effort: xhigh` |
| `gpt-5.4-nano` | `reasoning_effort: none` | omitted (model default) | `reasoning_effort: low` | `reasoning_effort: low` | `reasoning_effort: medium` | `reasoning_effort: high` | `reasoning_effort: xhigh` | `reasoning_effort: xhigh` | `reasoning_effort: xhigh` |
| `gpt-realtime-2.1` | `reasoning_effort: none` | omitted (model default) | `reasoning_effort: minimal` | `reasoning_effort: low` | `reasoning_effort: medium` | `reasoning_effort: high` | `reasoning_effort: xhigh` | `reasoning_effort: max` | `reasoning_effort: max` |
| `gpt-realtime-2.1-mini` | `reasoning_effort: none` | omitted (model default) | `reasoning_effort: minimal` | `reasoning_effort: low` | `reasoning_effort: medium` | `reasoning_effort: high` | `reasoning_effort: xhigh` | `reasoning_effort: max` | `reasoning_effort: max` |
| `gpt-realtime-2` | `reasoning_effort: none` | omitted (model default) | `reasoning_effort: minimal` | `reasoning_effort: low` | `reasoning_effort: medium` | `reasoning_effort: high` | `reasoning_effort: xhigh` | `reasoning_effort: max` | `reasoning_effort: max` |

### Anthropic (Messages)

| Model | `none` | `auto` | `minimal` | `low` | `medium` | `high` | `xhigh` | `max` | `ultra` |
|---|---|---|---|---|---|---|---|---|---|
| `claude-fable-5` | omitted | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: xhigh` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |
| `claude-mythos-5` | omitted | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: xhigh` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |
| `claude-opus-5` | `thinking: disabled` | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: xhigh` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |
| `claude-opus-4-8` | `thinking: disabled` | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: xhigh` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |
| `claude-opus-4-7` | `thinking: disabled` | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: xhigh` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |
| `claude-opus-4-6` | `thinking: disabled` | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |
| `claude-opus-4-5-20251101` | `thinking: disabled` | `thinking: enabled (4096)` | `thinking: enabled (1024)` `effort: low` | `thinking: enabled (2048)` `effort: low` | `thinking: enabled (4096)` `effort: medium` | `thinking: enabled (8192)` `effort: high` | `thinking: enabled (16384)` `effort: high` | `thinking: enabled (32000)` `effort: high` | `thinking: enabled (32000)` `effort: high` |
| `claude-sonnet-5` | `thinking: disabled` | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: xhigh` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |
| `claude-sonnet-4-6` | `thinking: disabled` | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |
| `claude-sonnet-4-5-20250929` | `thinking: disabled` | `thinking: enabled (4096)` | `thinking: enabled (1024)` | `thinking: enabled (2048)` | `thinking: enabled (4096)` | `thinking: enabled (8192)` | `thinking: enabled (16384)` | `thinking: enabled (32000)` | `thinking: enabled (32000)` |
| `claude-haiku-4-5-20251001` | `thinking: disabled` | `thinking: enabled (4096)` | `thinking: enabled (1024)` | `thinking: enabled (2048)` | `thinking: enabled (4096)` | `thinking: enabled (8192)` | `thinking: enabled (16384)` | `thinking: enabled (32000)` | `thinking: enabled (32000)` |
| `claude-mythos-preview` | omitted | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |

### Amazon Bedrock (Converse `additionalModelRequestFields`)

| Model | `none` | `auto` | `minimal` | `low` | `medium` | `high` | `xhigh` | `max` | `ultra` |
|---|---|---|---|---|---|---|---|---|---|
| `anthropic.claude-opus-5` | `thinking: disabled` | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: xhigh` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |
| `anthropic.claude-fable-5` | omitted | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: xhigh` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |
| `anthropic.claude-opus-4-8` | `thinking: disabled` | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: xhigh` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |
| `anthropic.claude-sonnet-5` | `thinking: disabled` | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: xhigh` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |
| `anthropic.claude-opus-4-7` | `thinking: disabled` | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: xhigh` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |
| `anthropic.claude-sonnet-4-6` | `thinking: disabled` | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |
| `anthropic.claude-opus-4-6-v1` | `thinking: disabled` | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |
| `anthropic.claude-opus-4-5-20251101-v1:0` | `thinking: disabled` | `thinking: enabled (4096)` | `thinking: enabled (1024)` `effort: low` | `thinking: enabled (2048)` `effort: low` | `thinking: enabled (4096)` `effort: medium` | `thinking: enabled (8192)` `effort: high` | `thinking: enabled (16384)` `effort: high` | `thinking: enabled (32000)` `effort: high` | `thinking: enabled (32000)` `effort: high` |
| `anthropic.claude-sonnet-4-5-20250929-v1:0` | `thinking: disabled` | `thinking: enabled (4096)` | `thinking: enabled (1024)` | `thinking: enabled (2048)` | `thinking: enabled (4096)` | `thinking: enabled (8192)` | `thinking: enabled (16384)` | `thinking: enabled (32000)` | `thinking: enabled (32000)` |
| `anthropic.claude-haiku-4-5-20251001-v1:0` | `thinking: disabled` | `thinking: enabled (4096)` | `thinking: enabled (1024)` | `thinking: enabled (2048)` | `thinking: enabled (4096)` | `thinking: enabled (8192)` | `thinking: enabled (16384)` | `thinking: enabled (32000)` | `thinking: enabled (32000)` |
| `anthropic.claude-mythos-5` | omitted | `thinking: adaptive` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: low` | `thinking: adaptive` `effort: medium` | `thinking: adaptive` `effort: high` | `thinking: adaptive` `effort: xhigh` | `thinking: adaptive` `effort: max` | `thinking: adaptive` `effort: max` |

### Google Gemini API (`generationConfig.thinkingConfig`)

| Model | `none` | `auto` | `minimal` | `low` | `medium` | `high` | `xhigh` | `max` | `ultra` |
|---|---|---|---|---|---|---|---|---|---|
| `gemini-3.7-flash` | `thinkingLevel: LOW` | thinking config without level (model default) | `thinkingLevel: LOW` | `thinkingLevel: LOW` | `thinkingLevel: MEDIUM` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3.6-flash` | `thinkingLevel: MINIMAL` | thinking config without level (model default) | `thinkingLevel: MINIMAL` | `thinkingLevel: LOW` | `thinkingLevel: MEDIUM` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3.5-flash` | `thinkingLevel: MINIMAL` | thinking config without level (model default) | `thinkingLevel: MINIMAL` | `thinkingLevel: LOW` | `thinkingLevel: MEDIUM` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3.5-flash-lite` | `thinkingLevel: MINIMAL` | thinking config without level (model default) | `thinkingLevel: MINIMAL` | `thinkingLevel: LOW` | `thinkingLevel: MEDIUM` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3.1-flash-lite` | `thinkingLevel: MINIMAL` | thinking config without level (model default) | `thinkingLevel: MINIMAL` | `thinkingLevel: LOW` | `thinkingLevel: MEDIUM` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3.1-pro-preview` | `thinkingLevel: LOW` | thinking config without level (model default) | `thinkingLevel: LOW` | `thinkingLevel: LOW` | `thinkingLevel: MEDIUM` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3-flash-preview` | `thinkingLevel: MINIMAL` | thinking config without level (model default) | `thinkingLevel: MINIMAL` | `thinkingLevel: LOW` | `thinkingLevel: MEDIUM` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3.1-flash-live-preview` | `thinkingLevel: MINIMAL` | thinking config without level (model default) | `thinkingLevel: MINIMAL` | `thinkingLevel: LOW` | `thinkingLevel: MEDIUM` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-2.5-flash-native-audio-preview-12-2025` | `thinkingBudget: 0` | `thinkingBudget: -1` (dynamic) | `thinkingBudget: 512` | `thinkingBudget: 2048` | `thinkingBudget: 8192` | `thinkingBudget: 24576` | `thinkingBudget: 24576` | `thinkingBudget: 24576` | `thinkingBudget: 24576` |
| `gemini-3.1-flash-image` | `thinkingLevel: MINIMAL` | thinking config without level (model default) | `thinkingLevel: MINIMAL` | `thinkingLevel: MINIMAL` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3.1-flash-lite-image` | `thinkingLevel: MINIMAL` | thinking config without level (model default) | `thinkingLevel: MINIMAL` | `thinkingLevel: MINIMAL` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3-pro-image` | `thinkingLevel: HIGH` | thinking config without level (model default) | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-2.5-pro` | `thinkingBudget: 128` | `thinkingBudget: -1` (dynamic) | `thinkingBudget: 512` | `thinkingBudget: 2048` | `thinkingBudget: 8192` | `thinkingBudget: 24576` | `thinkingBudget: 32768` | `thinkingBudget: 32768` | `thinkingBudget: 32768` |
| `gemini-2.5-flash` | `thinkingBudget: 0` | `thinkingBudget: -1` (dynamic) | `thinkingBudget: 512` | `thinkingBudget: 2048` | `thinkingBudget: 8192` | `thinkingBudget: 24576` | `thinkingBudget: 24576` | `thinkingBudget: 24576` | `thinkingBudget: 24576` |
| `gemini-2.5-flash-lite` | `thinkingBudget: 0` | `thinkingBudget: -1` (dynamic) | `thinkingBudget: 512` | `thinkingBudget: 2048` | `thinkingBudget: 8192` | `thinkingBudget: 24576` | `thinkingBudget: 24576` | `thinkingBudget: 24576` | `thinkingBudget: 24576` |

### Google Vertex AI (`generationConfig.thinkingConfig`)

| Model | `none` | `auto` | `minimal` | `low` | `medium` | `high` | `xhigh` | `max` | `ultra` |
|---|---|---|---|---|---|---|---|---|---|
| `gemini-3.7-flash` | `thinkingLevel: LOW` | thinking config without level (model default) | `thinkingLevel: LOW` | `thinkingLevel: LOW` | `thinkingLevel: MEDIUM` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3.6-flash` | `thinkingLevel: MINIMAL` | thinking config without level (model default) | `thinkingLevel: MINIMAL` | `thinkingLevel: LOW` | `thinkingLevel: MEDIUM` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3.5-flash` | `thinkingLevel: MINIMAL` | thinking config without level (model default) | `thinkingLevel: MINIMAL` | `thinkingLevel: LOW` | `thinkingLevel: MEDIUM` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3.5-flash-lite` | `thinkingLevel: MINIMAL` | thinking config without level (model default) | `thinkingLevel: MINIMAL` | `thinkingLevel: LOW` | `thinkingLevel: MEDIUM` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3.1-flash-lite` | `thinkingLevel: MINIMAL` | thinking config without level (model default) | `thinkingLevel: MINIMAL` | `thinkingLevel: LOW` | `thinkingLevel: MEDIUM` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3-flash-preview` | `thinkingLevel: MINIMAL` | thinking config without level (model default) | `thinkingLevel: MINIMAL` | `thinkingLevel: LOW` | `thinkingLevel: MEDIUM` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3.1-flash-image` | `thinkingLevel: MINIMAL` | thinking config without level (model default) | `thinkingLevel: MINIMAL` | `thinkingLevel: MINIMAL` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3.1-flash-lite-image` | `thinkingLevel: MINIMAL` | thinking config without level (model default) | `thinkingLevel: MINIMAL` | `thinkingLevel: MINIMAL` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-3-pro-image` | `thinkingLevel: HIGH` | thinking config without level (model default) | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` | `thinkingLevel: HIGH` |
| `gemini-2.5-pro` | `thinkingBudget: 128` | `thinkingBudget: -1` (dynamic) | `thinkingBudget: 512` | `thinkingBudget: 2048` | `thinkingBudget: 8192` | `thinkingBudget: 24576` | `thinkingBudget: 32768` | `thinkingBudget: 32768` | `thinkingBudget: 32768` |
| `gemini-2.5-flash` | `thinkingBudget: 0` | `thinkingBudget: -1` (dynamic) | `thinkingBudget: 512` | `thinkingBudget: 2048` | `thinkingBudget: 8192` | `thinkingBudget: 24576` | `thinkingBudget: 24576` | `thinkingBudget: 24576` | `thinkingBudget: 24576` |
| `gemini-2.5-flash-lite` | `thinkingBudget: 0` | `thinkingBudget: -1` (dynamic) | `thinkingBudget: 512` | `thinkingBudget: 2048` | `thinkingBudget: 8192` | `thinkingBudget: 24576` | `thinkingBudget: 24576` | `thinkingBudget: 24576` | `thinkingBudget: 24576` |

### Ollama (`think`)

| Model | `none` | `auto` | `minimal` | `low` | `medium` | `high` | `xhigh` | `max` | `ultra` |
|---|---|---|---|---|---|---|---|---|---|
| `qwen3:*` | `think: false` | `think: true` | `think: true` | `think: true` | `think: true` | `think: true` | `think: true` | `think: true` | `think: true` |
| `gpt-oss:*` | `think: false` | `think: true` | `think: "low"` | `think: "low"` | `think: "medium"` | `think: "high"` | `think: "high"` | `think: "high"` | `think: "high"` |
| `deepseek-r1:*` | `think: false` | `think: true` | `think: true` | `think: true` | `think: true` | `think: true` | `think: true` | `think: true` | `think: true` |

## Token budgets

`thinking_tokens` is the explicit manual budget. Where a family has levels instead of budgets it is converted (Gemini 3.x: ≤ 2,000 → `LOW`, ≤ 12,000 → `MEDIUM`, else `HIGH`; OpenAI Realtime: ≤ 1,000 `minimal` … > 24,000 `xhigh`). Where a family has budgets it is clamped to the documented range and kept below `max_tokens`:

| Family | Minimum | Maximum | `0` |
|---|---|---|---|
| Claude 4.5 and older (manual) | 1,024 | `max_tokens` − 1,024 (64k ceiling) | not allowed → `thinking: disabled` via `none` |
| Gemini 2.5 Pro | 128 | 32,768 | → 128 (cannot disable) |
| Gemini 2.5 Flash | 1 | 24,576 | off |
| Gemini 2.5 Flash-Lite | 512 | 24,576 | off |
| Ollama | — | — | `think: false` |

When both `thinking_effort` and `thinking_tokens` are given, the tokens decide the budget and the effort decides any separate effort field (Claude Opus 4.5 accepts both).

## Keeping this page current

- Vocabulary and rules: [`gaise-core/src/contracts/gaise_reasoning.rs`](../gaise-core/src/contracts/gaise_reasoning.rs).
- Family tables: `openai_chat_rules` ([OpenAI](vendor-openai.md#parameter-compatibility-audited-2026-08-20)), `claude_family_rules` ([Anthropic](vendor-anthropic.md#parameter-compatibility-audited-2026-08-20)), `claude_rules` ([Bedrock](vendor-bedrock.md#parameter-compatibility-audited-2026-08-20)), `thinking_levels_for` / `thinking_budget_for` ([Gemini](vendor-gemini.md#parameter-compatibility-audited-2026-08-20), [Vertex AI](vendor-vertexai.md#parameter-compatibility-audited-2026-08-20)), `ollama_think` ([Ollama](vendor-ollama.md)).
- Regenerate the matrix after changing any of them: `cargo run -p gaise-client --example reasoning_matrix --all-features` ([`reasoning_matrix.rs`](../gaise-client/examples/reasoning_matrix.rs)) and paste the output into this page.
