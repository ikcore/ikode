# `/sessions`

> **Category:** Sessions · **Usage:** `/sessions [prune [--keep N] [--max-bytes SIZE] [--apply]]` · [All commands](README.md)
>
> ⚠️ Keep in sync with `handle_sessions` in [`repl.rs`](../../ikode-cli/src/repl.rs).

## What it does

Inspects or **safely prunes** saved session transcripts.

| Form | Effect |
| --- | --- |
| `/sessions` | Storage overview plus a hint about `prune` |
| `/sessions prune [--keep N] [--max-bytes SIZE]` | **Dry run**: show which oldest sessions would be removed |
| `/sessions prune … --apply` | Actually delete the eligible sessions |

## How it works

- Pruning removes **oldest first** until the retention targets are met: keep the newest `N`
  (`--keep`) and/or fit within a byte budget (`--max-bytes`, human-friendly sizes like
  `512MiB`).
- Defaults come from `.ikode/config.toml`: `session_keep` and `session_max_bytes`. Over-budget
  storage produces warnings ([/storage](storage.md)) but is **never deleted automatically** —
  deletion always requires an explicit `--apply`.
- **Protections**: the current session and any session owned by an active or blocked
  [goal](goal.md) are always exempt, whatever the flags say.

## Use cases

- **See the plan first** (always a dry run without `--apply`):

  ```text
  /sessions prune --keep 20
  ```

- **Enforce a byte budget**:

  ```text
  /sessions prune --max-bytes 512MiB --apply
  ```

- **Set-and-forget defaults** in `.ikode/config.toml`, then occasionally:

  ```toml
  session_keep = 30
  session_max_bytes = 536870912
  ```

  ```text
  /sessions prune --apply
  ```

## Related

- [/storage](storage.md) — the budget warning that points here
- [/resume](resume.md) — what those transcripts are for
