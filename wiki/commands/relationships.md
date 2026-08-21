# `/relationships`

> **Category:** Codebase · **Usage:** `/relationships [kind] [max]` · [All commands](README.md)
>
> ⚠️ Keep in sync with the `/relationships` arm in [`command_loop.rs`](../../ikode-cli/src/repl/command_loop.rs) and `run_relationships` in [`passes.rs`](../../ikode-cli/src/passes.rs).

## What it does

Describes graph **relationship edges** in one line each via the summary model, storing the
description on the edge. Described edges surface in [/ask](ask.md) connectivity output, so this
pass makes "how are these wired?" answers much more readable.

## How it works

- Arguments are order-independent tokens: an **ALL-CAPS word** selects the edge kind, a
  **number** caps the inference calls.
- Edge kinds: **`CALLS`** (default) is the precise call graph; **`REFERENCES`** is the broader
  mention graph (identifier mentions, filtered by a stopword list). Together they answer "who
  depends on this?".
- Describing every edge is **O(edges) inference calls** and call graphs can be huge, so the
  default cap is **200**; an explicit count overrides it and an explicit **`0` means
  unlimited**.
- Hash-gated like the other passes — already-described, unchanged edges are skipped — and
  Ctrl+C-cancellable with per-edge progress.

## Use cases

- **Default pass** — describe the most important call edges:

  ```text
  /relationships
  ```

- **Mention graph, small budget**:

  ```text
  /relationships REFERENCES 50
  ```

- **Full call graph, uncapped** (deliberate spend on a small repo):

  ```text
  /relationships 0
  ```

## Related

- [/query](query.md) — traverse the same edges deterministically
- [/ask](ask.md) — where the descriptions surface
