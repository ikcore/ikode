# `/document`

> **Category:** Sessions · **Usage:** `/document [path|clear]` (alias: `/doc`) · [All commands](README.md)

## What it does

Queues a local **document** (spec, log, data file …) to accompany your next message; `clear`
empties the queue.

## How it works

Same ephemeral, per-turn model as [/image](image.md): the document is injected at request
assembly for the next message only and never stored in the transcript. Paths can be dragged
into the prompt; one-shot runs use the repeatable `--attach`/`-a` flag.

## Use cases

- **Work against a spec**:

  ```text
  /document ./requirements.md
  which requirements does the current implementation miss?
  ```

- **Feed a long log once** (rather than pasting it into permanent history):

  ```text
  /doc ./crash.log
  what's the failure chain here?
  ```

## Related

- [/attach](attach.md) — the type-agnostic version
- [/image](image.md) — for pictures
