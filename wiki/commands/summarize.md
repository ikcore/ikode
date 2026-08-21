# `/summarize`

> **Category:** Codebase · **Usage:** `/summarize [max]` · [All commands](README.md)
>
> ⚠️ Keep in sync with `run_summarize` in [`passes.rs`](../../ikode-cli/src/passes.rs).

## What it does

Generates one-line natural-language summaries for indexed chunks via the **summary model**
([/smodel](smodel.md)) and stores them on the graph. Summaries feed richer
[/ask](ask.md) retrieval, richer [/embed](embed.md) vectors, and the
[/dir-summaries](dir-summaries.md) roll-up.

## How it works

- **Hash-gated and incremental**: chunks whose content hasn't changed since their last summary
  are skipped ("already enriched"), so re-runs only pay for real changes.
- `max` caps inference calls (`0` = all). Ctrl+C cancels cleanly; per-chunk progress lines
  stream as it works.
- Policy knobs are read from resolved config **at run time** (so `update_settings` writes take
  effect without a restart):
  - `summarize_tests` (default `false`) — tests are always indexed and embedded; this only
    governs the opt-in LLM summary pass.
  - `summarize_min_lines` (default `4`) — non-callable chunks below this many non-blank body
    lines are skipped; functions/methods are summarised regardless of size.
  - `summarize_concurrency` (default `4`) — concurrent summary calls.
- Runs [/index](index.md) first if the index is empty and applies `.ikignore` before starting.

## Use cases

- **Warm up retrieval quality** without paying for embeddings yet:

  ```text
  /summarize
  ```

- **Budgeted pass on a monorepo**:

  ```text
  /summarize 300
  ```

- **Include test files** for a test-heavy audit — set `summarize_tests = true` in
  `.ikode/config.toml`, then re-run.

## Related

- [/enrich](enrich.md) — summaries then embeddings in dependency order
- [/smodel](smodel.md) — pick a cheap/fast model for this pass
- [/dir-summaries](dir-summaries.md) — the per-directory roll-up built on these
