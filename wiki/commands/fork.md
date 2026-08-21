# `/fork`

> **Category:** Sessions · **Usage:** `/fork` · [All commands](README.md)

## What it does

Clones the active transcript into a **fresh saved session** and switches the REPL to that
branch. The parent session file is left **byte-for-byte untouched** and remains resumable.

## How it works

The current history is written to a new session id (a new JSONL transcript under `.ikode/`)
and the REPL re-targets its appends there. Nothing is shared after the split — the two
branches diverge independently, both visible to [/resume](resume.md) and
[/sessions](sessions.md). Forking requires at least one exchanged message.

## Use cases

- **Explore a risky approach** while keeping the safe line:

  ```text
  /fork
  let's try rewriting this with async-trait instead
  ```

  If it sours, `/resume` back to the parent.

- **A/B two designs** from the same shared context — fork twice and pursue one in each branch.

## Related

- [/resume](resume.md) — jump between branches
- [/btw](btw.md) — for questions that don't need a persistent branch
- [/clear](clear.md) — a fresh start with *no* inherited context
