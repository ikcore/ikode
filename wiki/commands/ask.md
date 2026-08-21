# `/ask`

> **Category:** Codebase · **Usage:** `/ask <question>` · [All commands](README.md)
>
> ⚠️ Keep in sync with the `/ask` arm in [`command_loop.rs`](../../ikode-cli/src/repl/command_loop.rs) and the retrieval code in [`index.rs`](../../ikode-cli/src/index.rs).

## What it does

Answers a natural-language question **about this codebase** by retrieving relevant indexed
chunks and synthesising an answer with the chat model. It is the conversational front door to
the code graph — cheaper and more focused than pasting files into chat.

## How it works

Embedding-first, with algorithmic fallback at every step:

1. If the index is empty, [/index](index.md) runs automatically first.
2. **With an embedding index** (`/embed` or `/enrich` has run): the question is embedded once,
   cheap **cosine similarity** over embeddings stored as graph-node properties ranks candidate
   chunks, and a small, **token-bounded synthesis pass** feeds the chat model one chunk-set at
   a time, escalating only if the answer needs more context.
3. **Without embeddings** — or if the query-embed or the answer call fails — it degrades to the
   traditional **keyword + graph lookup** (the same deterministic retrieval as
   [/ask-codebase](ask-codebase.md)) and prints the matches instead of failing.
4. The whole operation is Ctrl+C-cancellable.

Stored chunk summaries and relationship descriptions (from [/summarize](summarize.md) /
[/relationships](relationships.md)) enrich what the model sees, so a warmed-up index gives
noticeably better answers.

## Use cases

- **Orientation in an unfamiliar repo**:

  ```text
  /ask where is the WAL checkpoint triggered and what forces it?
  ```

- **Behavioural question spanning files**:

  ```text
  /ask how does session pruning decide which transcripts are protected?
  ```

- **Pre-change impact check** (pair with [/query](query.md) for exact edges):

  ```text
  /ask what breaks if ChunkRecord gains a new required field?
  ```

## Related

- [/ask-codebase](ask-codebase.md) — the raw retrieval, no LLM synthesis
- [/enrich](enrich.md) — one command to make `/ask` semantic
- [/query](query.md) — deterministic graph traversal when you need exact answers
