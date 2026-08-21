# `/goal`

> **Category:** Agent · **Usage:** `/goal [objective|switch <id>|done|abandon]` · [All commands](README.md)
>
> ⚠️ Keep in sync with the dispatch in [`command_loop.rs`](../../ikode-cli/src/repl/command_loop.rs) and [`goal.rs`](../../ikode-cli/src/goal.rs).

## What it does

Starts, resumes, steers, switches, or closes an **autonomous goal** — a persisted objective the
harness pursues across many model turns until acceptance criteria are met, the work blocks on
your input, or the step budget runs out. Behaviour is state-dependent:

| Form | With no resumable goal | With an active/blocked goal |
| --- | --- | --- |
| `/goal <text>` | Starts a new goal | **Steers** the goal (and answers a blocked goal's question) |
| `/goal` | Shows status | Resumes the goal loop |
| `/goal switch <id>` | Switches to (a prefix of) that goal id and resumes it | same |
| `/goal done` | — | Closes the active goal as **Done** |
| `/goal abandon` | — | Closes the active goal as **Abandoned** |

## How it works

- Each goal is a `GoalTask` with its **own conversation history, mode, effort snapshot, cache
  lane, and saved session** — deliberately separated from the interactive chat so multiple
  goals can coexist (and, by design, later run concurrently).
- The goal loop drives `run_turn` repeatedly. The model manages its own plan through the
  `task_*` tools: `task_plan` authors the acceptance criteria (shown by bare `/goal`),
  `task_complete` ends the loop successfully, and `task_block` pauses with a question for you —
  the goal enters **Blocked** and your next `/goal <text>` answers it.
- A safety budget caps the loop at `goal_max_steps` model turns (default **25**, configurable
  in `.ikode/config.toml`) so a goal can never run unbounded; the loop is also
  Ctrl+C-escapable.
- Goal contexts auto-compact past the [/compact auto](compact.md) threshold (default 80% of the
  model's context window), rewriting the goal's own
  session and rotating its cache lane — long goals don't blow the context window.
- Statuses: `Active`, `Blocked`, `Done`, `Abandoned`. Ids are addressable by unique prefix.
- Step progress shown by bare `/goal` comes from the shared todo list (the in-task tracker).

## Use cases

- **Fire-and-forget refactor** with a definition of done:

  ```text
  /goal migrate the settings loader to serde, keep round-trip tests green
  ```

- **Answer a blocked goal** — the goal asked "TOML or JSON for the new format?":

  ```text
  /goal use TOML, and keep reading the legacy JSON as a fallback
  ```

- **Juggle two objectives** — park one, resume the other:

  ```text
  /goals
  /goal switch 3f2a
  ```

- **Wrap up**:

  ```text
  /goal done        # acceptance met
  /goal abandon     # not worth finishing
  ```

## Related

- [/goals](goals.md) — list every goal and status
- [/agent](agent.md) — session-scoped read-only workers (a goal may write; a subagent may not)
- [Goals & tasks design doc](../../docs/GOALS_AND_TASKS.md)
