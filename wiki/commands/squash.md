# `/squash`

> **Category:** Codebase · **Usage:** `/squash` · [All commands](README.md)
>
> ⚠️ Keep in sync with `run_squash` in [`passes.rs`](../../ikode-cli/src/passes.rs) and the [WAL contract](../../docs/WAL.md).

## What it does

Checkpoints the **exact current graph state** into `.ikode/graph.log` and discards the
transaction history that produced it — a lossless compaction. Reports before/after sizes, the
percentage reclaimed, and the preserved node/edge counts and sequence number.

## How it works

- The WAL is a single-writer, checksummed log with explicit transaction/commit frames. Over
  time, edit history accumulates; `/squash` rewrites the log as one **exact-state snapshot**
  (LZ4-compressed where it saves space) that preserves internal IDs, indexes, and idempotency
  state — nothing derived is lost, unlike [/rebuild](rebuild.md).
- The same checkpoint happens **automatically** once 64 MiB of transaction delta accumulates;
  `/squash` merely forces it now. Snapshots are bounded to 1 GiB; transactions to 64 MiB
  uncompressed.
- Because history is irreversibly discarded, it **confirms first** (auto-confirmed in
  yolo/`--brave`). A no-op if there is no WAL yet.
- Startup nudges you towards `/squash` when the log has grown large; the nudge is
  informational only — automatic delta checkpoints remain enabled regardless.

## Use cases

- **Startup said the graph file is large**:

  ```text
  /squash
  ```

- **Before archiving / copying a project directory** — ship the minimal graph state.
- **After heavy churn** (mass renames, vendored-code import/removal) that bloated history.

## Related

- [/storage](storage.md) — see WAL size and since-checkpoint counters
- [/rebuild](rebuild.md) — the destructive alternative (loses enrichment)
- [WAL format and recovery contract](../../docs/WAL.md)
