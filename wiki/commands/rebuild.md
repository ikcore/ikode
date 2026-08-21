# `/rebuild`

> **Category:** Codebase · **Usage:** `/rebuild` · [All commands](README.md)
>
> ⚠️ Keep in sync with `run_rebuild` in [`passes.rs`](../../ikode-cli/src/passes.rs).

## What it does

**Destructively rebuilds** the derived graph data: wipes the WAL (`.ikode/graph.log`) and all
in-memory graph state, then re-indexes the project from scratch.

## How it works

- Because derived data (summaries, embeddings, relationship descriptions — everything the
  enrichment passes paid for) is lost with the WAL, it **asks for confirmation first**;
  yolo/`--brave` sessions auto-confirm.
- After the wipe it runs the same indexing pipeline as [/index](index.md) and reports the
  fresh statistics. Enrichment must then be re-run ([/enrich](enrich.md)) to restore summaries
  and vectors — that is re-paid inference, so prefer [/index](index.md) (incremental) or
  [/squash](squash.md) (lossless compaction) unless the graph is genuinely wrong.

## Use cases

- **Corrupt or suspicious graph state** that recovery can't fix:

  ```text
  /rebuild
  ```

- **After changing indexing-relevant settings** in a way that invalidates the existing
  structure wholesale (e.g. sweeping `.ikignore` changes you want applied cleanly).

## Related

- [/index](index.md) — incremental, non-destructive refresh (use this first)
- [/squash](squash.md) — shrink the WAL **without** losing any state
- [WAL format and recovery contract](../../docs/WAL.md)
