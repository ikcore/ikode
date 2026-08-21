# iKode CLI

iKode is an agentic coding CLI for browsing, editing, and reasoning over code and
Markdown with a graph-backed index. It supports interactive and one-shot work,
persisted sessions and goals, provider-independent chat/embeddings, and a local
code-map visualizer.

## Requirements

- Rust 1.91.1 (pinned in `rust-toolchain.toml`)
- Credentials for at least one configured provider, or a local Ollama instance

Provider settings are read from the environment or from `.env` / `.ikode/.env`.
Common variables include `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`,
`OLLAMA_URL`, `AWS_REGION`, `VERTEXAI_API_URL`, and `VERTEXAI_SA_PATH`.

## Installation

```bash
cargo install --path ikode-cli
```

## Quick start

```bash
ikode init
ikode
```

`ikode init` creates the shareable `.ikode/config.toml` and `.ikode/ikode.md`
project files. Generated indexes, sessions, goals, embeddings, and local permission
settings remain git-ignored.

## Run modes

- `ikode` — interactive REPL
- `ikode --prompt "do X"` — one-shot run
- `ikode --model "provider::model"` — select the chat model
- `ikode --effort low|medium|high|max|ultra` — set session reasoning/delegation effort
- `ikode --emodel "provider::model"` — select the embedding model
- `ikode --smodel "provider::model"` — select the summarization model
- `ikode --continue` or `ikode --resume <id>` — resume a saved session
- `ikode --no-index` — skip configured startup indexing and sync prompts
- `ikode --no-graph` — disable graph/index tools for a traditional harness session
- `ikode --brave` — use yolo mode for this session

## Interactive commands

The prompt offers a searchable slash-command palette. The main command groups are:

- Retrieval/indexing: `/ask`, `/ask-codebase`, `/index`, `/init`, `/enrich`,
  `/embed`, `/summarize`, `/architecture`, `/relationships`, `/dir-summaries`,
  `/graph`, `/rebuild`, and `/squash`.
- Agents, goals, and sessions: `/effort`, `/agent`, `/agents` (`/tasks` alias),
  `/goal`, `/goals`, `/btw` (`/side` alias), `/fork`, `/resume`, `/history`,
  `/storage`, `/sessions`, `/compact`, `/clear`, `/max-history`, and `/prefix-keep`.
- Models, integrations, and permissions: `/model`, `/emodel`, `/smodel`, `/mode`,
  `/graph on|off`, `/mcp`, `/allow`, and `/deny`. Permission rules support `list`,
  `remove`, and `clear`; history limits
  support `save [global]`. Shift+Tab cycles plan, agentic, and yolo modes.
- Project features: `/skills`, `/visualize [port]`, `/image <path>`, `/document <path>`,
  generic `/attach <path>`, and `/paste` for clipboard images. Image and document
  paths can also be dragged into the prompt; one-shot use supports repeatable
  `--image/-i` and `--attach/-a` flags.
- Diagnostics/terminal: `/doctor`, `!<command>`, `/cls`, `/help`, and `/exit`.

`/history` reports message, token, transcript, and aggregate session byte usage.
`/storage` combines session totals with WAL/checkpoint/compression counters.
`/sessions prune --keep 20` or `--max-bytes 512MiB` produces a dry-run; add
`--apply` to remove eligible oldest sessions. The current session and active or
blocked goal sessions are always protected. `session_keep` and
`session_max_bytes` in `.ikode/config.toml` provide default retention targets;
over-budget storage warns but is never deleted automatically.

`/fork` clones the active transcript into a fresh saved session and switches to
that branch; the parent remains untouched and resumable. `/btw <question>` (or
`/side <question>`) answers from the current conversation in a temporary,
read-only side chat, then returns automatically. Its messages, tool trace, and
answer are not appended to the main transcript; bare `/btw` prompts for a question.

`/effort` accepts `auto`, `low`, `med`/`medium`, `high`, `max`, and `ultra`.
Ultra combines the deepest provider-supported reasoning with proactive delegation;
other levels delegate only when requested. `/agent spawn <task>` starts an isolated
read-only worker, `/agents` shows its state, and `/agent steer|wait|stop|close|collect`
controls the thread. See [effort and subagent controls](docs/EFFORT_AND_AGENTS.md).

## Graph and traditional harness modes

Graph mode remains enabled by default. `/graph off` switches the current session
to a traditional file/shell harness: graph-backed tools are removed from model
requests, startup/index sync is disabled, and file changes do not write to the graph
WAL. `/graph on` reloads the project graph. Add `save` (and optionally `global`) to
persist either state, or set `graph_enabled = false` in `.ikode/config.toml`.

## Third-party MCP servers

iKode connects to MCP tool servers over both standard transports: local stdio and
remote Streamable HTTP. Registrations are stored in the git-ignored
`.ikode/settings.local.json` file and reconnect automatically on startup.

```text
/mcp add filesystem -- npx -y @modelcontextprotocol/server-filesystem .
/mcp add remote --url https://example.com/mcp --header "Authorization=Bearer ${MCP_TOKEN}"
/mcp list
/mcp tools
/mcp disable filesystem
/mcp refresh
/mcp remove filesystem
```

Discovered tools are namespaced as `mcp__server__tool`. Every MCP tool is treated
as externally mutating and permission-gated in agentic mode, withheld in plan mode,
and still subject to explicit deny rules in yolo mode. See the [MCP integration
guide](docs/MCP.md) for configuration, environment references, compatibility, and
the security boundary.

## Agent tools

iKode exposes normal coding tools, six subagent-orchestration tools, and three
additional goal-control tools. They cover deterministic retrieval and graph
traversal, exact file/chunk editing, bounded shell execution, todos, settings,
project skills, and parallel read-only delegation. See the factual
[agent tool catalog](docs/IKODE_TOOLS.md) for the complete list and permission model.

File operations are restricted to the project root. Shell commands default to a
120-second timeout (maximum 600 seconds) and retain at most 512 KiB each from stdout
and stderr. Mutating tools are unavailable in plan mode and permission-gated in
agentic mode.

Harness metadata (`config.toml`, local settings, goals, and compacted session
transcripts) is written through same-directory temporary files and atomically
replaced. Session appends are synced per JSONL record; an incomplete trailing
record is recoverable, while malformed complete records are reported instead of
silently disappearing.

## Graph durability

The code graph is backed by a single-writer, checksummed WAL at
`.ikode/graph.log`. Transactions use explicit transaction/commit frames and are
synced before success is returned. Torn trailing records are repaired to the last
commit boundary; checksum damage inside the log stops recovery rather than silently
dropping data. Existing JSONL logs are read as a legacy prefix and can be extended
with the framed format.

Payloads of 4 KiB or more use LZ4 when compression saves space. Transactions are
bounded to 64 MiB uncompressed, snapshots to 1 GiB, and 64 MiB of transaction delta
triggers an exact-state checkpoint before the next write. `/squash` forces the same
checkpoint manually. See the [WAL format and recovery contract](docs/WAL.md).

## Development

The repository contains three Rust build roots because GAISe is a nested workspace
and LivingVector is a standalone crate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets

cd modules/gaise
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets

cd ../livec_graph
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

CI runs these checks on Windows and Linux.

## License

Licensed under the GNU Affero General Public License v3.0 only. See [LICENSE](LICENSE).
