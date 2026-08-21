# `/ask-codebase`

> **Category:** Codebase · **Usage:** `/ask-codebase <question>` (also accepted: `/ask_codebase`) · [All commands](README.md)

## What it does

The **raw-retrieval twin of [/ask](ask.md)**: prints the relevant indexed chunks *and how they
are wired together* (connectivity via graph edges), with **no LLM synthesis**. What you see is
exactly the evidence `/ask` would reason over.

## How it works

- Runs the deterministic retrieval pipeline (`Indexer::ask` with a cap of 12 chunks): keyword
  scoring over indexed chunk names/content plus graph-neighbourhood expansion — **zero model
  calls, zero cost, instant**.
- It is the console twin of the `ask_codebase` *tool* the model itself uses for retrieval, so
  it's also the way to debug "why did the model get shown that?".
- If the index is empty, [/index](index.md) runs automatically first.

## Use cases

- **Zero-cost exploration** — find where something lives without spending tokens:

  ```text
  /ask-codebase graph query traversal
  ```

- **Verify retrieval quality** — check what evidence `/ask` sees before trusting its answer:

  ```text
  /ask-codebase session prune protections
  ```

- **Offline / no-key situations** — works with no provider configured at all.

## Related

- [/ask](ask.md) — the synthesising counterpart
- [/query](query.md) — structured traversal when keywords aren't precise enough
