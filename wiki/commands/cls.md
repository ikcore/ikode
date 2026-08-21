# `/cls`

> **Category:** General · **Usage:** `/cls` (also accepted: `/clear_screen`) · [All commands](README.md)

## What it does

Clears the terminal screen. Nothing else — conversation history, the saved session, and all
settings are untouched.

## How it works

A pure terminal operation (clear + cursor home). It is deliberately separate from
[/clear](clear.md), which resets the *conversation*; `/cls` only tidies the *scrollback*.

## Use cases

- Long enrichment or indexing output has filled the screen and you want a clean prompt:

  ```text
  /cls
  ```

## Related

- [/clear](clear.md) — reset history and start a new saved session (different command!)
