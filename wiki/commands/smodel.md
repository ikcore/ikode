# `/smodel`

> **Category:** Configuration · **Usage:** `/smodel [model|save [global]]` · [All commands](README.md)

## What it does

Shows (bare) or switches the **summary model** — used by every summarisation pass:
[/summarize](summarize.md), [/architecture](architecture.md), [/dir-summaries](dir-summaries.md),
[/relationships](relationships.md), and [/compact](compact.md).

| Form | Effect |
| --- | --- |
| `/smodel` | Show the current summary model |
| `/smodel <provider::model>` | Switch for this session only |
| `/smodel save [global]` | Persist to project (`summary_model` in `.ikode/config.toml`) or global config |

## How it works

Summarisation is high-volume, low-difficulty work — one short output per chunk/edge/directory —
so a **cheap, fast model** is usually the right choice here, independent of the chat model.
Unset, it falls back to the chat model's default. Switching mid-session affects the next pass;
hash-gating means already-stored summaries are not regenerated just because the model changed.

## Use cases

- **Cheap bulk enrichment**:

  ```text
  /smodel openai::gpt-5.4-mini
  /enrich
  ```

- **Fully local summaries**:

  ```text
  /smodel ollama::qwen3:8b
  /smodel save
  ```

## Related

- [/model](model.md), [/emodel](emodel.md), [/wmodel](wmodel.md)
- [/summarize](summarize.md) — the main consumer
