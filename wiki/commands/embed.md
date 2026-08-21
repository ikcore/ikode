# `/embed`

> **Category:** Codebase · **Usage:** `/embed` · [All commands](README.md)
>
> ⚠️ Keep in sync with `run_embed` in [`passes.rs`](../../ikode-cli/src/passes.rs).

## What it does

Builds (or refreshes) the **semantic embedding index** over all chunks using the embedding
model ([/emodel](emodel.md)). This is what upgrades [/ask](ask.md) from keyword matching to
semantic retrieval.

## How it works

- Embeddings are stored **as node properties on the graph** (classic graph-RAG: vector on the
  node, cosine similarity computed at retrieval time — no separate vector store to manage).
- **Hash-gated**: a chunk is re-embedded only when its content (or folded-in summary) changed;
  requests are batched (8 chunks per request) for throughput. Re-runs are cheap.
- The embedding *document* for a chunk folds in its stored summary when one exists — run
  [/summarize](summarize.md) first (or just use [/enrich](enrich.md)) for richer vectors.
- Changing the embedding model with [/emodel](emodel.md) invalidates stored vectors; they are
  re-derived on the next pass.
- Ctrl+C-cancellable with per-chunk progress; failure is reported honestly so
  [/enrich](enrich.md) won't claim semantic `/ask` is ready when no vectors were stored.
- Runs [/index](index.md) first if the index is empty; `.ikignore` exclusions are honoured.

## Use cases

- **Enable semantic search** on an already-indexed repo:

  ```text
  /embed
  ```

- **After switching embedding models**:

  ```text
  /emodel ollama::nomic-embed-text
  /embed
  ```

- **Fully local semantic search** — point [/emodel](emodel.md) at an Ollama model and `/embed`
  without any cloud key.

## Related

- [/enrich](enrich.md) — summarise + embed in the right order
- [/emodel](emodel.md) — which model computes the vectors
- [/visualize](visualize.md) — the code map's semantic search uses these same vectors
