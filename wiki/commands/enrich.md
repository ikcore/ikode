# `/enrich`

> **Category:** Codebase · **Usage:** `/enrich [max]` · [All commands](README.md)
>
> ⚠️ Keep in sync with `run_enrich` in [`passes.rs`](../../ikode-cli/src/passes.rs).

## What it does

Runs the full **semantic-index pipeline** in dependency order: summarise chunks first
([/summarize](summarize.md)), then embed them ([/embed](embed.md)). Ends with
"semantic /ask is ready".

## How it works

- **Order matters**: an embedding document folds in the chunk's stored summary, so summarising
  *before* embedding yields richer vectors in one pass.
- `max` caps the **summary** call count (`0` = all); embedding always covers every chunk whose
  vector is out of date.
- Both passes are **incremental and hash-gated** — re-running `/enrich` only touches what
  changed since last time, so it's cheap to run after every meaningful edit burst.
- **Ctrl+C aborts the whole pipeline** (a cancel in the summary pass does not fall through to
  embedding), and every item prints a per-chunk progress line.
- Summary behaviour is tunable in `.ikode/config.toml`: `summarize_tests` (default `false`),
  `summarize_min_lines` (default `4`), `summarize_concurrency` (default `4`).
- With `auto_enrich = true` in config, this pass runs automatically after any agent turn that
  changed files.

## Use cases

- **After a refactor**, refresh what changed (hash gates skip the rest):

  ```text
  /enrich
  ```

- **Budgeted enrichment on a huge repo**:

  ```text
  /enrich 200
  ```

- **Hands-off freshness** — set `auto_enrich = true` in `.ikode/config.toml` and stop thinking
  about it.

## Related

- [/init](init.md) — index + enrich in one command
- [/summarize](summarize.md), [/embed](embed.md) — run either half alone
- [Configuration guide](../configuration.md) — the `summarize_*` and `auto_enrich` knobs
