# Configuration & environment variables

> **⚠️ Keep this page up to date.** It mirrors `ProjectConfig` in
> [`ikode-cli/src/harness.rs`](../ikode-cli/src/harness.rs), `LocalSettings` in
> [`ikode-cli/src/settings.rs`](../ikode-cli/src/settings.rs), `load_dotenv` in
> [`harness.rs`](../ikode-cli/src/harness.rs), and the `/vars` table in
> [`repl.rs`](../ikode-cli/src/repl.rs). **A new config key or environment variable must be
> added here in the same change.**
>
> Last reviewed: 01/08/2026

## The layers, highest priority first

1. **CLI flags** — `--model`, `--effort`, `--brave`, `--no-index`, `--no-graph`, … (one process only)
2. **`.ikode/settings.local.json`** — machine-local runtime posture (git-ignored)
3. **`.ikode/config.toml`** — committed, shareable project configuration
4. **Global config** — `%APPDATA%\ikode\config.toml` (Windows) / `~/.config/ikode/config.toml`
5. **Built-in defaults**

Project config overlays global **field by field** — any key set in the project file wins; the
rest fall through. `ikode init` scaffolds `.ikode/config.toml` and `.ikode/ikode.md` (project
briefing); generated state (indexes, sessions, goals, embeddings, local settings) stays
git-ignored.

## Environment variables

### Provider credentials

| Variable | Purpose |
| --- | --- |
| `OPENAI_API_KEY` / `OPENAI_API_URL` / `OPENAI_API_TIER` | OpenAI key, optional endpoint override, service tier |
| `ANTHROPIC_API_KEY` / `ANTHROPIC_API_URL` | Anthropic key, optional endpoint override |
| `GEMINI_API_KEY` / `GEMINI_API_URL` | Google Gemini key, optional endpoint override |
| `OLLAMA_URL` | Local Ollama endpoint (no key needed) |
| `AWS_REGION` / `BEDROCK_REGION` | AWS Bedrock region selection |
| `VERTEXAI_API_URL` / `VERTEXAI_API_TIER` / `VERTEXAI_SA_PATH` | Vertex AI endpoint, tier, service-account JSON path |
| `GOOGLE_ACCOUNT_ID` / `GOOGLE_PRIVATE_KEY` | Vertex AI service-account credentials (alternative to `VERTEXAI_SA_PATH`) |

### Web research

| Variable | Purpose |
| --- | --- |
| `BRAVE_API_KEY` | Brave Search backend |
| `TAVILY_API_KEY` | Tavily backend |
| `SEARXNG_URL` | Self-hosted SearXNG endpoint (also settable as `web_search_endpoint` in config) |

Backend selection: first available of **Brave → Tavily → SearXNG**, or pin one with
`web_search_backend`. With none configured, web research reports itself disabled
([/doctor](commands/doctor.md) shows this).

### Editor

`$VISUAL` / `$EDITOR` — used by [/skills](commands/skills.md) `add`/`edit`.

Inspect everything live with [/vars](commands/vars.md) (secrets masked to `***` + last four);
verify interpretation with [/doctor](commands/doctor.md).

## Env-file layering

At startup iKode loads `.env`-style files **in this precedence order** (first file to define a
key wins):

| Order | File | Overrides inherited environment? |
| --- | --- | --- |
| 1 | `.ikode/.env` | **Yes** — explicit project-scoped iKode config |
| 2 | `.ikode/.ikenv` | **Yes** |
| 3 | `.ikenv` (project root) | No — never clobbers a real env var |
| 4 | `.env` (project root) | No |

`.ikenv` is an iKode-specific alias for `.env`, parsed identically. The asymmetry is
deliberate: root-level `.env` files are often shared with other tooling, so the real
environment wins there; files under `.ikode/` are explicit iKode configuration, so they win
over the environment.

Parser rules: `#` comment lines, blank lines, optional `export ` prefix; keys are
`[A-Za-z0-9_]` (must not start with a digit); double-quoted values honour `\n` `\t` `\r` `\"`
`\\` escapes, single-quoted are literal; **unquoted values keep trailing `# …` text** (secrets
containing `#` survive — quote a value if it needs trimming). Values are never logged; startup
prints only which files were applied.

## `.ikode/config.toml` — full key reference

Committed and shareable. Written by `save`-style commands with comment-preserving TOML
editing, so hand-written comments survive.

### Models

| Key | Default | Meaning |
| --- | --- | --- |
| `model` | built-in default | Chat model, `provider::model` ([/model](commands/model.md)) |
| `embedding_model` | built-in default | Embedding model — separate key because changing it invalidates stored embeddings ([/emodel](commands/emodel.md)) |
| `summary_model` | chat default | Summary passes: chunks, architecture, directories, relationships, `/compact` ([/smodel](commands/smodel.md)) |
| `web_model` | chat model | Model web-research agents run on ([/wmodel](commands/wmodel.md)) |

### Behaviour

| Key | Default | Meaning |
| --- | --- | --- |
| `brave` | `false` | Start sessions in yolo mode |
| `max_history` | — | Request history limit; `0` = unlimited ([/max-history](commands/max-history.md)) |
| `prefix_keep` | — | Stable history prefix for prompt caching ([/prefix-keep](commands/prefix-keep.md)) |
| `auto_index` | — | Index automatically at startup |
| `graph_enabled` | `true` | `false` = traditional file harness ([/graph](commands/graph.md)) |
| `auto_enrich` | `false` | After an agent turn that changed files, re-index then summarise + embed affected chunks (hash-gated, Ctrl+C-escapable) |

### Enrichment

| Key | Default | Meaning |
| --- | --- | --- |
| `summarize_tests` | `false` | Summarise test chunks (tests are always indexed and embedded regardless) |
| `summarize_min_lines` | `4` | Skip summarising non-callable chunks below this many non-blank lines (functions always summarised) |
| `summarize_concurrency` | `4` | Concurrent summary calls |

### Sessions

| Key | Default | Meaning |
| --- | --- | --- |
| `session_keep` | — | Newest transcripts retained by [/sessions](commands/sessions.md) `prune` |
| `session_max_bytes` | — | Byte budget: warn when exceeded (never auto-deletes), default for `prune --max-bytes` |

### Agents & goals

| Key | Default | Meaning |
| --- | --- | --- |
| `goal_max_steps` | `25` | Model-turn budget per [/goal](commands/goal.md) loop before pausing |
| `agent_max_threads` | `4` | Max concurrent subagent threads ([/agent](commands/agent.md)) |
| `agent_max_turns` | `12` | Model turns per subagent before it fails closed |

### Web research

| Key | Default | Meaning |
| --- | --- | --- |
| `web_search_backend` | first available | Pin `"brave"`, `"tavily"`, or `"searxng"` |
| `web_search_endpoint` | — | Self-hosted SearXNG base URL (or `SEARXNG_URL`) |
| `web_max_searches` | `3` | Searches per research task (depth 2 doubles) |
| `web_max_fetches` | `4` | Page fetches per research task (depth 2 doubles) |
| `web_max_agents` | `2` | Concurrent web agents (within `agent_max_threads`) |
| `web_agent_max_turns` | `8` | Model turns per web agent |

## `.ikode/settings.local.json` — machine-local settings

Git-ignored; the highest local layer (only CLI flags beat it). Written atomically by the
commands that manage it — direct edits work too, but prefer the commands.

```json
{
  "mode": "agentic",
  "effort": "high",
  "permissions": {
    "allow": ["execute_command(cargo *)", "web_research(*)"],
    "deny": ["execute_command(git push*)"]
  },
  "models": { "chat": null, "embedding": null, "summary": null, "web": null },
  "auto_compact_percent": 80,
  "mcp_servers": { }
}
```

| Field | Managed by | Notes |
| --- | --- | --- |
| `mode` | [/mode](commands/mode.md), Shift+Tab | `plan` / `agentic` / `yolo` |
| `effort` | [/effort](commands/effort.md) | `auto`/`low`/`medium`/`high`/`max`/`ultra` (aliases: `med`, `xhigh`) |
| `permissions.allow` / `.deny` | [/allow](commands/allow.md), [/deny](commands/deny.md) | `tool` or `tool(glob)`; deny always wins |
| `models` | model commands | Local overrides above config.toml |
| `auto_compact_percent` | [/compact auto](commands/compact.md) | Auto-compact at this share of the chat model’s context window; default 80; `0` disables |
| `auto_compact_tokens` | [/compact auto](commands/compact.md) | Absolute override in prompt tokens (wins over the percentage); also the 256,000 fallback when the window is unknown; `0` disables |
| `mcp_servers` | [/mcp](commands/mcp.md) | See the [MCP guide](mcp.md) |

A missing file yields defaults silently; a malformed one yields defaults **with a warning** —
startup never fails over local settings.

## Best practices

- **Commit** `.ikode/config.toml` and `.ikode/skills/`; **never commit** secrets,
  `settings.local.json`, or generated state (the default `.gitignore` from `ikode init`
  handles this).
- **Secrets go in `.ikode/.env`** when they are iKode-specific (they must win over a stale
  shell export), or the root `.env` when shared with other tooling (the environment wins
  there). Confirm the winner with [/vars](commands/vars.md).
- **Model keys are split for a reason** — set a cheap `summary_model` and `web_model`, keep
  the chat model strong, and treat `embedding_model` changes as a re-embed event.
- **Guard-rails over ceremony**: prefer a few precise [/allow](commands/allow.md) rules plus
  hard [/deny](commands/deny.md) lines to running in yolo; deny rules survive every mode.
- **Set retention early** (`session_keep`, `session_max_bytes`) so [/storage](commands/storage.md)
  warnings mean something before the directory is huge.
- After any credential change, run [/doctor](commands/doctor.md) once — cheaper than a failed
  provider call mid-turn.

## Related

- [Command reference](commands/README.md)
- [MCP server setup](mcp.md)
- [Wiki home](README.md)
