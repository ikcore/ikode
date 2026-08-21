# `/max-history`

> **Category:** Sessions · **Usage:** `/max-history <n> [save [global]] | save [global]` · [All commands](README.md)

## What it does

Sets (or persists) the **request history limit** — the maximum number of past messages sent
with each model request. `0` means unlimited.

| Form | Effect |
| --- | --- |
| `/max-history 40` | Set for this session |
| `/max-history 40 save` | Set and persist to `.ikode/config.toml` (`max_history`) |
| `/max-history save global` | Persist the current value to the global config |

## How it works

Before each request, history beyond the limit is trimmed from the middle-distance past (the
full transcript on disk is never touched — this only shapes what the *model* sees). It trades
context completeness for token cost. Contrast with [/compact](compact.md), which *summarises*
older turns instead of dropping them, and [/prefix-keep](prefix-keep.md), which pins the
*start* of history for cache stability.

## Use cases

- **Cap costs on a long-running session** where deep history rarely matters:

  ```text
  /max-history 30 save
  ```

- **Diagnose "model forgot X"** — check whether the limit is trimming the relevant turn:

  ```text
  /history
  ```

## Related

- [/prefix-keep](prefix-keep.md), [/compact](compact.md), [/history](history.md)
