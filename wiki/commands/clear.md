# `/clear`

> **Category:** Sessions · **Usage:** `/clear` · [All commands](README.md)

## What it does

Resets the conversation history and **begins a new saved session**. The old session stays on
disk, untouched and resumable — nothing is deleted.

## How it works

The in-memory history is reset to a fresh system context, a new session id/transcript is
created for subsequent appends, and the request cache lane rotates (the prompt prefix changed).
To merely clean the *screen*, use [/cls](cls.md); to *shrink* context while keeping the
conversation, use [/compact](compact.md).

## Use cases

- **Task switch** — the accumulated context is now irrelevant and would only cost tokens:

  ```text
  /clear
  ```

- **Runaway context** you don't need any of — `/clear` beats [/compact](compact.md) when
  nothing is worth keeping.

## Related

- [/compact](compact.md) — keep the conversation, shrink the cost
- [/resume](resume.md) — the old session is one command away
- [/cls](cls.md) — screen only, not history
