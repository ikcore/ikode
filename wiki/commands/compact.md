# `/compact`

> **Category:** Sessions · **Usage:** `/compact` · `/compact auto [<percent>% | <tokens> | off | default]` · [All commands](README.md)
>
> ⚠️ Keep in sync with `run_compact`/`run_compact_auto`/`compact_messages` in [`passes.rs`](../../ikode-cli/src/passes.rs)
> and the threshold rules in [`settings.rs`](../../ikode-cli/src/settings.rs).

## What it does

Summarises older turns into a single briefing note, keeping recent turns verbatim, then
continues **in the same session** — context cost drops, the thread survives.

| Form | Effect |
| --- | --- |
| `/compact` | Compact now |
| `/compact auto` | Show the auto-compact threshold for the current chat model and how close the context is to it |
| `/compact auto 70%` | Auto-compact at 70% of the chat model's context window (persisted; clears any absolute override) |
| `/compact auto 200k` | Auto-compact at an absolute 200,000 prompt tokens, whatever the model (also `200000`, `200_000`) |
| `/compact auto off` | Disable auto-compaction (manual `/compact` only) |
| `/compact auto default` | Back to the built-in policy (**80%** of the window) |

## How it works

- The history is rebuilt as `[system, summary, …recent tail]`. The split point is the latest
  *user* turn before the recent window (the last **6** messages), so the kept tail never
  starts with an orphaned tool result.
- The summary is produced by the **summary model** ([/smodel](smodel.md)); the operation is
  Ctrl+C-escapable, and "not enough history" is a clean no-op.
- On success the on-disk transcript is **rewritten atomically** to match and the request cache
  lane rotates (the prompt prefix changed, so the provider cache must restart).
- **Auto-compaction** runs the same routine after any turn whose prompt size (the current
  context) reached the threshold — including inside [goal](goal.md) tasks, which compact
  their own isolated histories.

### The threshold

By default the threshold is **`auto_compact_percent` (80%) of the chat model's context
window**. The window comes from GAISe's bundled model registry (vendor-documented figures,
offline), falling back to a one-off live `list_models` call to the provider for models the
registry doesn't know (Anthropic, Gemini, and Ollama report windows; OpenAI does not). The
lookup happens once per model and is cached for the session.

Precedence, in `.ikode/settings.local.json`:

1. `auto_compact_tokens` set → that absolute number wins (older settings files keep their
   exact behaviour);
2. else `auto_compact_percent` × context window;
3. else, if the window is unknown, **256,000** tokens.

`0` in either field disables auto-compaction. `/history` shows the effective threshold
alongside the current context size.

## Use cases

- **Long session getting expensive** — keep working without losing the thread:

  ```text
  /compact
  ```

- **Before resuming a marathon session** you know has hundreds of turns.
- **Leave more headroom for big tool results** on a small-window local model:

  ```text
  /compact auto 60%
  ```

- **Your Ollama server runs a smaller `num_ctx` than the model's documented window** — pin an
  absolute limit instead:

  ```text
  /compact auto 24k
  ```

## Related

- [/clear](clear.md) — when nothing in the history is worth summarising
- [/history](history.md) — see the token pressure that motivates compaction
- [/prefix-keep](prefix-keep.md) — the cache-friendly stable prefix
