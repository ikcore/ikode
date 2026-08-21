# `/init`

> **Category:** Codebase · **Usage:** `/init [max]` · [All commands](README.md)

## What it does

One-shot project **warm-up**: (re)builds the index, then runs the full enrichment pipeline —
summaries, then embeddings. After it finishes, semantic [/ask](ask.md) is ready.

Note: this is different from the *shell* command `ikode init`, which scaffolds
`.ikode/config.toml` and `.ikode/ikode.md` for a new project.

## How it works

Literally [/index](index.md) followed by [/enrich](enrich.md)` [max]`:

- `max` caps the number of **summary** inference calls (`0` or omitted = no cap); embedding
  always covers every out-of-date chunk.
- Both enrichment passes are hash-gated (already-current chunks are skipped) and
  **Ctrl+C-cancellable** — a cancel in the summary pass aborts the pipeline rather than
  silently continuing.
- Cost scales with codebase size on first run, then drops to near-zero on re-runs thanks to
  the hash gates.

## Use cases

- **New clone, full send** — one command before starting work:

  ```text
  /init
  ```

- **Big repo, bounded first pass** — cap summaries, let embeddings complete, deepen later:

  ```text
  /init 500
  ```

## Related

- [/enrich](enrich.md) — the same pipeline without re-indexing
- [/index](index.md), [/summarize](summarize.md), [/embed](embed.md) — the individual stages
