# `/model`

> **Category:** Configuration · **Usage:** `/model [model|save [global]]` · [All commands](README.md)

## What it does

Shows (bare) or switches the **chat model** — the model that drives conversation, tool calls,
goals, and (by default) web agents. Model names are `provider::model`.

| Form | Effect |
| --- | --- |
| `/model` | Show the current chat model |
| `/model <provider::model>` | Switch **for this session only** |
| `/model save` | Persist the current model to `.ikode/config.toml` (`model` key) |
| `/model save global` | Persist to the global config (`%APPDATA%\ikode\config.toml` on Windows, `~/.config/ikode/config.toml` elsewhere) |

## How it works

- Providers: `openai`, `anthropic`, `gemini`, `ollama`, `bedrock`, `vertexai` — resolved
  through the GAISe provider layer, with credentials from the environment (see the
  [configuration guide](../configuration.md)).
- A session switch is instant and unlogged in config; `save` writes the key with
  comment-preserving TOML editing. Resolution order: CLI `--model` > local settings > project
  config > global config > built-in default.
- The chat model also serves as the fallback for [/wmodel](wmodel.md) and the default base for
  the summary model.
- [/effort](effort.md) translation adapts automatically to the selected model's vocabulary.

## Use cases

- **Try a stronger model for one hard session**:

  ```text
  /model anthropic::claude-sonnet-5
  ```

- **Make it the project default** (committed, shared with the team):

  ```text
  /model save
  ```

- **Local-only work**:

  ```text
  /model ollama::qwen3:14b
  ```

## Related

- [/smodel](smodel.md), [/emodel](emodel.md), [/wmodel](wmodel.md) — the other three model slots
- [/doctor](doctor.md) — verify the provider credential for the chosen prefix
- [Configuration guide](../configuration.md)
