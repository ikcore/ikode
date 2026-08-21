# `/attach`

> **Category:** Sessions · **Usage:** `/attach [path|clear]` · [All commands](README.md)
>
> ⚠️ Keep in sync with the attachment handling in [`attach.rs`](../../ikode-cli/src/attach.rs) and [`repl/command_loop.rs`](../../ikode-cli/src/repl/command_loop.rs).

## What it does

Queues a local file — **image or document, auto-detected by type** — to accompany your next
message. `clear` empties the queue. It is the type-agnostic superset of
[/image](image.md) and [/document](document.md).

## How it works

- Attachments are **ephemeral and per-turn**: they are injected into the request at assembly
  time for your next message, not stored in the transcript — so they cost tokens once, not on
  every subsequent turn.
- Paths can also be **dragged into the prompt**, and a large text paste collapses to a tidy
  `[Pasted N lines]` placeholder (deletable as one unit; expanded back on submit).
- One-shot runs support repeatable flags: `ikode -a design.png -a spec.pdf --prompt "..."`.

## Use cases

- **Mixed evidence for one question**:

  ```text
  /attach ./design/screenshot.png
  /attach ./spec.pdf
  how does the implementation differ from this spec and mock?
  ```

- **Changed your mind**:

  ```text
  /attach clear
  ```

## Related

- [/image](image.md), [/document](document.md) — type-specific variants
- [/paste](paste.md) — clipboard images
