# `/agents`

> **Category:** Agent · **Usage:** `/agents` (alias: [/tasks](tasks.md)) · [All commands](README.md)

## What it does

Lists every subagent thread — id, name, status (running / finished / stopped), and task — plus
a hint line for the [/agent](agent.md) control verbs.

## How it works

Reads the in-memory agent registry of the current session (subagent threads are
session-scoped, unlike persisted [goals](goals.md)). The same listing renders when you run
bare `/agent`. A compact live count also appears right-aligned in the prompt's frame rule
while workers run.

## Use cases

- **Check what's still running** before you [/exit](exit.md) or wait:

  ```text
  /agents
  /agent wait all
  ```

- **Find the id/name to steer or collect**:

  ```text
  /agents
  /agent collect wal-audit
  ```

## Related

- [/agent](agent.md) — spawn and control workers
- [/goals](goals.md) — persisted goals, a different concept
