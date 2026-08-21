# `/history`

> **Category:** Sessions · **Usage:** `/history` · [All commands](README.md)

## What it does

Prints the session's history and usage statistics:

- **Max messages per request** ([/max-history](max-history.md); "unlimited" when 0) and
  **prefix keep** ([/prefix-keep](prefix-keep.md))
- **Total messages stored** in the in-memory history
- **Session tokens** — input / output / cached
- **Current transcript** size on disk, and **all saved sessions** (total bytes across files)

## How it works

Reads live counters (token usage accumulates per request, including [/btw](btw.md) side turns)
and stats the transcript files under `.ikode/`. No model calls. The cached-token figure shows
how well the stable prefix ([/prefix-keep](prefix-keep.md)) is doing its job with providers
that support prompt caching.

## Use cases

- **Cost check mid-session** — is it time to [/compact](compact.md)?

  ```text
  /history
  ```

- **Cache tuning** — low cached-token counts on a long session suggest the prefix is being
  disturbed; see [/prefix-keep](prefix-keep.md).

## Related

- [/storage](storage.md) — disk-centric view (WAL + sessions)
- [/compact](compact.md), [/max-history](max-history.md), [/prefix-keep](prefix-keep.md)
