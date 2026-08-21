# `/visualize`

> **Category:** Codebase · **Usage:** `/visualize [port]` (alias: `/viz`) · [All commands](README.md)
>
> ⚠️ Keep in sync with [`visualize/mod.rs`](../../ikode-cli/src/visualize/mod.rs).

## What it does

Serves the **interactive code map** — a local web app over the indexed codebase — and blocks
until Ctrl+C. Open the printed URL in a browser to explore:

- a **left-to-right collapsible containment tree** (directories → files → symbols, rendered
  with D3),
- a **3D relationship graph** (force layout in higher dimensions, projected via PCA, rendered
  with three.js / 3d-force-graph),
- a **code box** showing any chunk's full source and stored summary,
- **search** — semantic (cosine over stored embeddings) or keyword.

## How it works

- An [axum] server rides the tokio runtime already in use; the page is a **single static HTML
  file embedded at compile time** — no npm, no build step, no CDN.
- It serves JSON APIs over an owned **snapshot** of the index (`/api/tree`, `/api/graph`,
  `/api/chunk`, `/api/search`, `/api/stats`), so the REPL keeps ownership of the live indexer
  while the server runs.
- The only model access is a shared client used to **embed search queries**; everything else
  is served from the snapshot. Semantic search therefore needs [/embed](embed.md) to have run —
  keyword search works regardless.
- `port` is optional; omit it for the default.

## Use cases

- **Explore structure visually** during onboarding:

  ```text
  /visualize
  ```

- **Fixed port** (e.g. to bookmark or tunnel):

  ```text
  /viz 8931
  ```

- **Semantic hunting** — after [/enrich](enrich.md), search "retry with backoff" and jump
  straight to the implementing chunk.

## Related

- [/embed](embed.md) — powers the semantic search box
- [/architecture](architecture.md) — the prose version of the same picture
