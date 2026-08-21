# `/index`

> **Category:** Codebase · **Usage:** `/index` (alias: [/scan](scan.md)) · [All commands](README.md)
>
> ⚠️ Keep in sync with `run_index` in [`passes.rs`](../../ikode-cli/src/passes.rs) and [`index.rs`](../../ikode-cli/src/index.rs).

## What it does

Builds or refreshes the **code and Markdown index**: parses project files into chunks
(functions, types, sections), symbols, and graph edges, and reports per-chunk progress plus a
final summary (files / chunks / references, skipped / ignored / removed, and a per-language
breakdown).

## How it works

- Parsing is **pure Rust and algorithmic — no model calls** (a deliberate design rule: no
  native C parsers either). Indexing is synchronous and fast; each chunk streams a progress
  line as it lands.
- **Incremental**: unchanged files are skipped, deleted files are removed from the graph, and
  `.ikignore` rules are applied (newly ignored files are pruned).
- Skips well-known junk directories (`.git`, `.ikode`, `target`, `node_modules`, `dist`,
  `build`, virtualenvs, IDE folders, `vendor`, …) and any file over **2 MiB**.
- Produces two edge families: **CALLS** (the precise call graph) and **REFERENCES** (the
  broader mention graph, with a stopword list so generic identifiers like `new`/`get` don't
  link everything). Together they answer "who depends on this?".
- All writes go through the checksummed **graph WAL** at `.ikode/graph.log` (see
  [/squash](squash.md) and the [WAL contract](../../docs/WAL.md)).
- Startup can auto-index (config `auto_index`), and `--no-index` skips it for one run.

## Use cases

- **First contact with a repo** (or after a big `git pull`):

  ```text
  /index
  ```

- **Keep the graph current while working** — cheap to re-run; only changes are processed.
- **CI-fresh clone, no provider keys yet** — indexing needs no credentials; only
  [/enrich](enrich.md) does.

## Related

- [/init](init.md) — index **and** enrich in one shot
- [/rebuild](rebuild.md) — destructive from-scratch rebuild
- [/embed](embed.md), [/summarize](summarize.md) — the inference passes layered on top
