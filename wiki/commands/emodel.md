# `/emodel`

> **Category:** Configuration · **Usage:** `/emodel [model|save [global]]` · [All commands](README.md)

## What it does

Shows (bare) or switches the **embedding model** used by [/embed](embed.md) and by the
embedding-first retrieval in [/ask](ask.md) and [/visualize](visualize.md) search.

| Form | Effect |
| --- | --- |
| `/emodel` | Show the current embedding model |
| `/emodel <provider::model>` | Switch for this session only |
| `/emodel save [global]` | Persist to project (`embedding_model` in `.ikode/config.toml`) or global config |

## How it works

The embedding model is a **separate config key** from the chat model for a hard reason:
vectors from different models are not comparable, so **changing it invalidates stored
embeddings** — iKode notes this on switch and re-derives vectors on the next
[/embed](embed.md)/[/enrich](enrich.md) pass (hash gates track the model identity). Query
embedding at `/ask` time always uses the same model as the stored vectors it is compared with.

## Use cases

- **Cheaper cloud embeddings**:

  ```text
  /emodel openai::text-embedding-3-small
  /emodel save global
  ```

- **Fully local semantic search** (no cloud key needed):

  ```text
  /emodel ollama::nomic-embed-text
  /embed
  ```

## Related

- [/embed](embed.md) — rebuild vectors after a switch
- [/ask](ask.md) — the semantic retrieval that consumes them
