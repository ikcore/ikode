# `/help`

> **Category:** General · **Usage:** `/help` · [All commands](README.md)
>
> ⚠️ Keep in sync with [`palette.rs`](../../ikode-cli/src/palette.rs) — this page documents behaviour, the registry defines it.

## What it does

Prints the full command reference in the terminal, grouped into the same categories as this
wiki (General, Agent, Codebase, Configuration, Sessions), plus the `!<command>` shell escape.

## How it works

`/help` is rendered directly from the `COMMANDS` registry in
[`palette.rs`](../../ikode-cli/src/palette.rs) — the same array that drives the live palette
suggestions. Because both views read one source of truth, the palette and `/help` can never
drift apart; each entry carries its `usage`, one-line `summary`, and `category`, and
`print_help()` simply iterates the categories in a fixed order.

## Use cases

- **Forgotten syntax** — you remember a command exists but not its arguments:

  ```text
  /help
  ```

- **Discovering what's available** after an upgrade — new commands appear automatically since
  the output is registry-driven.

## Related

- [/doctor](doctor.md) — environment diagnostics rather than command syntax
- The live palette (type `/`) — searchable, keyboard-driven version of the same registry
