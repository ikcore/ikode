# `/dir-summaries`

> **Category:** Codebase · **Usage:** `/dir-summaries [max]` · [All commands](README.md)

## What it does

Rolls chunk summaries up into **one summary per directory**, stored on each Directory node in
the graph. Gives retrieval (and you) a mid-altitude view between chunk detail and the
whole-project [/architecture](architecture.md) overview.

## How it works

- For each directory, the summary model condenses that directory's **chunk summaries** into a
  one-liner — so it works best *after* [/summarize](summarize.md) has run (or as part of a
  warmed-up index).
- `max` caps how many directories are processed; hash-gated so unchanged directories are
  skipped ("already current"); Ctrl+C-cancellable with per-directory progress.
- Runs [/index](index.md) first if the index is empty and applies `.ikignore`.

## Use cases

- **Complete the summary stack** after chunk summaries:

  ```text
  /summarize
  /dir-summaries
  ```

- **Budgeted roll-up** on a deep tree:

  ```text
  /dir-summaries 20
  ```

## Related

- [/summarize](summarize.md) — the per-chunk layer this builds on
- [/architecture](architecture.md) — the single top-level narrative
