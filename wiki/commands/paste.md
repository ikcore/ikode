# `/paste`

> **Category:** Sessions · **Usage:** `/paste` · [All commands](README.md)
>
> ⚠️ Keep in sync with the clipboard handling in [`palette.rs`](../../ikode-cli/src/palette.rs) and [`attach.rs`](../../ikode-cli/src/attach.rs).

## What it does

Queues an **image from the clipboard** for your next message.

## How it works

- Reads the OS clipboard for image data and interns it as an attachment — the editor shows a
  single `[Image …]` placeholder (one cursor stop; one Backspace deletes it atomically).
- **Why a command exists at all**: many terminals intercept **Ctrl+V** for their own text
  paste, so the keystroke never reaches iKode. Where it *does* reach the editor, Ctrl+V
  attaches a clipboard image directly (falling back to text paste when the clipboard holds
  text); `/paste` is the path that works everywhere.
- Like all attachments it is ephemeral and per-turn — injected into the next request, not
  stored in the transcript.

## Use cases

- **Screenshot → question**, no file dance:

  ```text
  <take screenshot>
  /paste
  what's wrong with this error dialog?
  ```

## Related

- [/image](image.md) — the file-path variant
- [/attach](attach.md) — auto-detecting variant
