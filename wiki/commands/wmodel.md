# `/wmodel`

> **Category:** Configuration · **Usage:** `/wmodel [model|clear|save [global]]` · [All commands](README.md)
>
> ⚠️ Keep in sync with the `/wmodel` arm in [`command_loop.rs`](../../ikode-cli/src/repl/command_loop.rs) and [`web/mod.rs`](../../ikode-cli/src/web/mod.rs).

## What it does

Shows (bare) or switches the **web-research agent model** — the model that sandboxed web agents
run on when the lead calls the `web_research` tool.

| Form | Effect |
| --- | --- |
| `/wmodel` | Show the pinned model, or the inherited chat model if none is pinned |
| `/wmodel <provider::model>` | Pin for this session — **new** web agents use it; running ones keep theirs |
| `/wmodel clear` (alias `reset`) | Remove the pin; web agents inherit the chat model again |
| `/wmodel save [global]` | Persist the pin (`web_model` in config) — requires an explicit pin first |

## How it works

- Web research is architecturally isolated: the lead never touches the network. `web_research`
  spawns a dedicated, budget-bounded **web agent** whose *only* tools are `web_search` and
  `web_fetch`; raw pages live and die inside that worker, and only a short synthesised answer
  with source URLs returns. This contains prompt injection from untrusted web content (the
  worker has no write/shell/orchestration tools) and keeps page dumps out of the lead's
  context and cache prefix.
- Research is summarisation-shaped work, so a **cheap/fast model is a good pin** — that's the
  entire reason this slot exists separately from [/model](model.md).
- Budgets (config-tunable): 3 searches / 4 fetches per task by default (a depth-2 request
  doubles them), max 2 concurrent web agents (within the overall
  [`agent_max_threads`](agent.md) cap), 8 model turns per agent.
- Search backends: Brave → Tavily → SearXNG, first available wins (pin with
  `web_search_backend`); keys via `BRAVE_API_KEY` / `TAVILY_API_KEY` / `SEARXNG_URL`. See the
  [configuration guide](../configuration.md#web-research). JavaScript-heavy pages fall back to
  a headless render via your own Chrome/Edge/Chromium.
- `web_research` is permission-gated as a **network tool**: available even in plan mode (it's
  read-only) but always prompting unless an allow rule such as `web_research(*)` or yolo mode
  covers it.

## Use cases

- **Pin a cheap researcher** while keeping a strong chat model:

  ```text
  /wmodel gemini::gemini-2.5-flash
  /wmodel save
  ```

- **Check what web agents would use right now**:

  ```text
  /wmodel
  ```

- **Undo the pin**:

  ```text
  /wmodel clear
  ```

## Related

- [/vars](vars.md) — see which search-backend keys are set
- [/allow](allow.md) — e.g. `/allow web_research(*)` to stop the prompts
- [/agents](agents.md) — web agents appear in the thread list (`… · 1 web`)
