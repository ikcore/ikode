# `/image`

> **Category:** Sessions · **Usage:** `/image [path|clear]` (alias: `/img`) · [All commands](README.md)

## What it does

Queues a local **PNG / JPEG / GIF / WebP** image to accompany your next message; `clear`
empties the image queue.

## How it works

- Images are **ephemeral and per-turn**: injected into the next request at assembly time,
  never stored in the transcript — later turns don't re-pay their token cost, and resumed
  sessions don't replay them.
- In the editor an attached image occupies a single placeholder (one cursor stop, one
  Backspace to remove). Dragging a file into the prompt and Ctrl+V of a clipboard image (where
  the terminal permits) queue the same way; [/paste](paste.md) is the reliable clipboard path.
- Queued images are also passed into [/btw](btw.md) side turns.
- One-shot runs: repeatable `--image`/`-i` flags.

## Use cases

- **Debug from a screenshot**:

  ```text
  /image ./bug-repro.png
  why would the layout collapse like this?
  ```

- **Design-to-code**:

  ```text
  /img ./mock.png
  scaffold this settings panel
  ```

## Related

- [/attach](attach.md) — auto-detects images vs documents
- [/paste](paste.md) — straight from the clipboard
