# `!<command>` — shell escape

> **Category:** General · **Usage:** `!<command>` · [All commands](README.md)

## What it does

Runs a shell command directly from the prompt and **feeds its output back to the model** as
conversation context. It is the quickest way to put ground truth (test results, git state,
tool versions) in front of the model without asking it to run the command itself.

## How it works

The command runs in your shell with output captured into the transcript, so the model sees it
on the next turn. Unlike the model-invoked `execute_command` tool, a `!` command is *you*
acting — it is not permission-gated, runs in any mode (including plan), and is not subject to
allow/deny rules. Treat it accordingly.

## Use cases

- **Give the model real test output** to debug from:

  ```text
  !cargo test --workspace 2>&1 | tail -30
  why is wal::recovery::torn_tail failing?
  ```

- **Show current git state** before asking for a commit plan:

  ```text
  !git status
  !git diff --stat
  ```

- **Check a tool version** mid-conversation:

  ```text
  !rustc --version
  ```

## Related

- [/allow](allow.md) / [/deny](deny.md) — govern the *model's* shell access, not yours
- [/mode](mode.md) — modes never restrict `!` commands
