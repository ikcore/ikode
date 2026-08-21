# `/storage`

> **Category:** Sessions · **Usage:** `/storage` · [All commands](README.md)

## What it does

Shows combined disk usage for the project's iKode state:

- **Graph WAL** (`.ikode/graph.log`) — current size, bytes accumulated since the last
  checkpoint, and how much compression has saved
- **Saved sessions** — total bytes across all transcript files

It also warns when saved sessions exceed the configured budget (`session_max_bytes`),
suggesting a [/sessions](sessions.md) `prune` dry run — over-budget storage warns but is
**never deleted automatically**.

## How it works

Stats the WAL through the indexer's counters and walks the saved-session directory. Purely
local; also embedded at the end of [/doctor](doctor.md) output.

## Use cases

- **Before/after a [/squash](squash.md)** — see what the checkpoint reclaimed:

  ```text
  /storage
  /squash
  /storage
  ```

- **Disk pressure triage** — is it the WAL (→ `/squash`) or transcripts (→
  `/sessions prune`)?

## Related

- [/squash](squash.md) — shrink the WAL
- [/sessions](sessions.md) — shrink the transcripts
- [/history](history.md) — the token-centric view
