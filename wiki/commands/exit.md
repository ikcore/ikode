# `/exit`

> **Category:** General · **Usage:** `/exit` · [All commands](README.md)

## What it does

Quits the interactive session cleanly.

## How it works

The session transcript is already durable before exit: appends are synced per JSONL record as
the conversation happens, so quitting loses nothing. The same quit path is triggered by
**Ctrl+D** (EOF) or **Ctrl+C on an empty prompt** — Ctrl+C with text typed only clears the
current line, like a shell.

## Use cases

- End of a working session:

  ```text
  /exit
  ```

- Come back later with `ikode --continue` (latest session) or [/resume](resume.md) from a new
  REPL.

## Related

- [/resume](resume.md), [/sessions](sessions.md)
