# `/graph`

> **Category:** Configuration · **Usage:** `/graph [status|on|off|save [global]]` · [All commands](README.md)

## What it does

Shows, switches, or persists **graph/index mode** — whether iKode runs with its graph-backed
retrieval stack or as a traditional file/shell harness.

| Form | Effect |
| --- | --- |
| `/graph` or `/graph status` | Show the current mode |
| `/graph off` | Switch this session to the **traditional harness** |
| `/graph on` | Reload the project graph and restore graph tools |
| `/graph on save` / `/graph off save [global]` | Persist the state (`graph_enabled` in config) |

## How it works

`/graph off` is structural, not cosmetic:

- graph-backed tools are **removed from model requests** (the model cannot call them),
- startup indexing/sync is disabled,
- file changes stop writing to the graph WAL.

`/graph on` reloads `.ikode/graph.log` and restores the full tool surface. The equivalent
one-shot flag is `ikode --no-graph`; the config key is `graph_enabled = false`. Graph mode
defaults to **on**.

## Use cases

- **Tiny script or scratch directory** where an index is overkill:

  ```text
  /graph off
  ```

- **Generated-code monorepo** where indexing is noise — persist the choice for the project:

  ```text
  /graph off save
  ```

- **Coming back to full retrieval**:

  ```text
  /graph on
  /index
  ```

## Related

- [/index](index.md), [/ask](ask.md) — what graph mode enables
- [/doctor](doctor.md) — reports "disabled (traditional harness)" when off
