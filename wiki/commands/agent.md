# `/agent`

> **Category:** Agent · **Usage:** `/agent [spawn|inspect|steer|wait|stop|close|collect] ...` · [All commands](README.md)
>
> ⚠️ Keep in sync with `handle_agent_command` in [`agent.rs`](../../ikode-cli/src/agent.rs).

## What it does

Spawns and controls **parallel subagent threads** — isolated, read-only workers that research,
trace, or analyse in the background while you keep working, then report back to the lead
session. It is the console twin of the model's own orchestration tools.

Supported forms:

| Form | Effect |
| --- | --- |
| `/agent spawn [--name NAME] [--effort LEVEL] <task>` | Start a worker on `<task>` |
| `/agent <id>` or `/agent inspect <id>` (alias `show`) | Show a worker's state and transcript |
| `/agent steer <id\|name> <message>` (alias `send`) | Send follow-up guidance to a running worker |
| `/agent wait [id\|all]` | Block until the worker(s) finish |
| `/agent stop <id\|all>` | Interrupt a running worker |
| `/agent close <id\|all>` | Discard a worker and its thread |
| `/agent collect <id\|all>` | Pull a finished worker's result into the conversation |
| `/agent` | List threads (same as [/agents](agents.md)) |

## How it works

- Workers run **read-only**: they get the bounded leaf toolset (retrieval, file reads, graph
  queries) with no write, shell-mutating, or orchestration tools — so they cannot recursively
  spawn more agents or change the workspace.
- Each worker has its own conversation and cache lane; results return to the lead as a
  summarised report, keeping raw exploration out of your context.
- Budgets come from `.ikode/config.toml`: `agent_max_threads` concurrent workers (default
  **4**) and `agent_max_turns` model turns per worker (default **12**, failing closed).
- Workers **inherit the lead's effort** by default; an `ultra` lead deliberately gives implicit
  workers `high` effort so fan-out doesn't multiply maximum-effort cost. Override per worker
  with `--effort`.
- Running-agent counts appear live in the prompt's status rule (e.g. `2 agents · 1 web`).
- At `/effort ultra` the *model* also spawns workers proactively; at other levels it delegates
  only when asked.

## Use cases

- **Parallel research while you code** — name it so you can steer it later:

  ```text
  /agent spawn --name wal-audit review the WAL recovery paths for torn-write handling
  ```

- **Add scope mid-flight**:

  ```text
  /agent steer wal-audit also check the checksum-failure branch
  ```

- **Fan out three independent questions, then gather**:

  ```text
  /agent spawn map every caller of Indexer::graph_query
  /agent spawn list the config keys read by passes.rs
  /agent spawn summarise how sessions are persisted
  /agent wait all
  /agent collect all
  ```

- **A worker went down a rabbit hole**:

  ```text
  /agent stop wal-audit
  /agent close wal-audit
  ```

## Related

- [/agents](agents.md) / [/tasks](tasks.md) — list threads
- [/effort](effort.md) — delegation policy and worker effort inheritance
- [Effort and subagent controls](../../docs/EFFORT_AND_AGENTS.md)
