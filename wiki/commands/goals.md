# `/goals`

> **Category:** Agent · **Usage:** `/goals` · [All commands](README.md)

## What it does

Lists every goal task — id, status glyph (`Active`, `Blocked`, `Done`, `Abandoned`), and
objective — so you can pick one to resume with [/goal](goal.md) `switch <id>`.

## How it works

Reads the persisted `GoalStore` (goals survive restarts; each goal owns a saved session that
[/resume](resume.md) can also open). Purely local; no model calls. Goal ids can be addressed by
any unique prefix, and prefixes are validated for uniqueness.

## Use cases

- **Morning triage** — see what's still open and what's blocked on you:

  ```text
  /goals
  /goal switch 3f2a
  ```

- **Cleanup** — spot abandoned experiments worth closing out with `/goal done` / `/goal abandon`.

## Related

- [/goal](goal.md) — start, resume, steer, switch, close
- [Goals & tasks design doc](../../docs/GOALS_AND_TASKS.md)
