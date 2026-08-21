# `/query`

> **Category:** Codebase · **Usage:** `/query <json>` · [All commands](README.md)
>
> ⚠️ Keep in sync with `GraphQuerySpec` in [`index.rs`](../../ikode-cli/src/index.rs).

## What it does

Runs a **structured, deterministic query** over the code graph — resolve a start set, walk
labelled edges, filter, project — and prints one row per matching node:
`kind name — path:start-end [chunk_id]`. **No inference**; results are exact.

## How it works

The JSON deserialises into `GraphQuerySpec` (shared with the model's `graph_query` tool — the
model can run exactly the same queries):

| Field | Shape | Meaning |
| --- | --- | --- |
| `start` | `{"symbol": "...", "chunk_id": "...", "kind": "..."}` | Resolve the starting node set (any subset of fields) |
| `traverse` | `[{"edge": "CALLS", "dir": "out"\|"in", "depth": 2}, …]` | Hops to walk; each step's reached set becomes the next step's frontier. `dir` defaults to `out`, `depth` to 1 |
| `where` | `{"kind": ["fn"], "path_prefix": "src/", "visibility": "pub", "name": "..."}` | Post-traversal filter; every set field must hold. `kind` is case-insensitive |
| `select` | `["name", "path", …]` | Projection (optional) |
| `limit` | number | Row cap (optional) |

Edge kinds of interest: `CALLS` (precise call graph), `REFERENCES` (mention graph), plus the
containment edges the indexer maintains. Runs [/index](index.md) first if the index is empty.

## Use cases

- **Blast radius** — everything reachable from `parse` in two call hops:

  ```text
  /query {"start":{"symbol":"parse"},"traverse":[{"edge":"CALLS","dir":"out","depth":2}]}
  ```

- **Reverse dependencies** — who calls into the WAL module:

  ```text
  /query {"start":{"symbol":"append_commit"},"traverse":[{"edge":"CALLS","dir":"in"}],"where":{"path_prefix":"ikode-cli/src"}}
  ```

- **API surface audit** — public functions in one subtree:

  ```text
  /query {"where":{"kind":["fn"],"visibility":"pub","path_prefix":"modules/gaise"},"limit":50}
  ```

## Related

- [/ask-codebase](ask-codebase.md) — keyword retrieval when you don't know the symbol
- [/relationships](relationships.md) — attach LLM descriptions to these same edges
- [/visualize](visualize.md) — see the graph instead of querying it
