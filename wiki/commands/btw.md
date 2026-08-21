# `/btw`

> **Category:** Sessions · **Usage:** `/btw [question]` (alias: `/side`) · [All commands](README.md)
>
> ⚠️ Keep in sync with `run_btw` in [`turn.rs`](../../ikode-cli/src/turn.rs).

## What it does

Asks a question in an **ephemeral, read-only side chat** that sees the current conversation but
leaves no trace in it. The answer prints, then you're back in the main thread exactly as it
was. Bare `/btw` prompts for the question.

## How it works

- The side turn runs over a **clone** of the current context; its messages and tool trace
  never pass through the recorder, so neither the in-memory history nor the JSONL transcript
  changes.
- The model receives only the **bounded read-only leaf toolset** used by subagents — it can
  retrieve and read but not write, run mutating commands, or spawn agents (which also prevents
  recursive orchestration).
- The main context-size gauge is preserved across the side turn so a large side prompt cannot
  trigger auto-compaction after your next ordinary turn; token *usage totals* still count it,
  because it consumed real usage.
- Queued [/image](image.md)/[/attach](attach.md) items are included in the side turn (they stay
  local to it).

## Use cases

- **Quick recall without polluting context**:

  ```text
  /btw what did we decide about the retry policy earlier?
  ```

- **A tangent you don't want in the transcript** (the main thread's prompt cache prefix also
  stays intact):

  ```text
  /side how does tokio's watch channel differ from broadcast?
  ```

- **Sanity-check before steering a long-running piece of work**:

  ```text
  /btw list the files we have already changed this session
  ```

## Related

- [/fork](fork.md) — when the tangent deserves its own *persistent* branch
- [/compact](compact.md) — the other way to protect context budget
