# `/resume`

> **Category:** Sessions · **Usage:** `/resume` · [All commands](README.md)

## What it does

Opens an interactive picker of saved sessions and resumes the selected one — full history,
ready to continue where it left off.

## How it works

- Sessions are JSONL transcripts under `.ikode/`, appended and synced per record as you work,
  so everything listed is complete up to the last exchange. An incomplete trailing record
  (e.g. a crash mid-write) is recovered; malformed complete records are reported, never
  silently dropped.
- Resuming re-targets the REPL at that transcript; picking the current session is a no-op.
- Shell equivalents: `ikode --continue` (most recent session) and `ikode --resume <id>`
  (specific id, prefix-addressable).
- Goal tasks own sessions too — resuming one restores that goal's isolated context.

## Use cases

- **Pick up yesterday's thread**:

  ```text
  /resume
  ```

- **Scripted resume** of a known session:

  ```bash
  ikode --resume 3f2a
  ```

- **Switch between [forked](fork.md) branches** of the same investigation.

## Related

- [/sessions](sessions.md) — inspect and prune what's on disk
- [/fork](fork.md), [/clear](clear.md) — how new sessions come to exist
