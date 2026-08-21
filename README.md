# iKode CLI

iKode is an agentic coding CLI for browsing, editing, and reasoning over code and
Markdown with a graph-backed index. It supports interactive and one-shot work,
persisted sessions and goals, parallel subagents, provider-independent chat and
embeddings, third-party MCP servers, web research, and a local code-map visualizer.

Full documentation lives in the [wiki](wiki/README.md): a
[slash-command reference](wiki/commands/README.md) with one page per command, a
[configuration and environment guide](wiki/configuration.md), and an
[MCP server guide](wiki/mcp.md).

## Requirements

- Rust toolchain `1.97.1` (pinned in [`rust-toolchain.toml`](rust-toolchain.toml);
  `rustup` installs it automatically). The crate's minimum supported version is
  `1.91.1`.
- Credentials for at least one provider, or a local Ollama instance.

Chat, summary, embedding, and web-research models are independently switchable
across **OpenAI, Anthropic, Google Gemini, Vertex AI, Amazon Bedrock, and Ollama**,
using `provider::model` names such as `anthropic::claude-opus-5` or
`ollama::llama3`. Credentials are read from the environment or from `.env` /
`.ikode/.env` (for example `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`,
`OLLAMA_URL`, `AWS_REGION`, `VERTEXAI_SA_PATH`). See
[provider credentials](wiki/configuration.md#provider-credentials) for the complete
table and [env-file layering](wiki/configuration.md#env-file-layering) for precedence.
Inspect what is set with [`/vars`](wiki/commands/vars.md) and verify the setup with
[`/doctor`](wiki/commands/doctor.md).

## Installation

```bash
cargo install --path ikode-cli
```

## Quick start

```bash
ikode init   # create .ikode/config.toml and .ikode/ikode.md
ikode        # start the interactive REPL
```

Then, inside the REPL:

```text
/init                                   # index + summarize + embed in one pass
/ask how does the WAL recover from a torn write?
/visualize                              # open the interactive code map
```

`ikode init` creates the shareable `.ikode/config.toml` and `.ikode/ikode.md`
project files. Generated indexes, sessions, goals, embeddings, and local permission
settings remain git-ignored. Every config key is documented in the
[`config.toml` reference](wiki/configuration.md#ikodeconfigtoml--full-key-reference).

## Run modes

| Flag | Effect |
| --- | --- |
| `ikode` | Interactive REPL |
| `ikode init` | Scaffold the `.ikode/` project folder |
| `-p, --prompt "do X"` | One-shot run |
| `-m, --model provider::model` | Chat model (default `openai::gpt-5.6-luna`) |
| `-s, --smodel provider::model` | Summarization model (default `openai::chat-5.6-luna`) |
| `-e, --emodel provider::model` | Embedding model (default `openai::text-embedding-3-small`) |
| `--effort auto\|low\|medium\|high\|max\|ultra` | Reasoning and delegation effort |
| `-i, --image PATH` / `-a, --attach PATH` | Attach an image or document to the first prompt (repeatable) |
| `-g, --guide PATH` | Extra guide file for the system prompt |
| `--continue` / `--resume <id>` | Resume the latest or a specific saved session |
| `--max-history N` / `--prefix-keep N` | Request history limit (default 80) and stable prefix (default 4) |
| `--no-index` | Skip configured startup indexing and sync prompts |
| `--no-graph` | Disable graph/index tools for a traditional file/shell harness |
| `-b, --brave` | Use yolo mode (no command confirmation) for this session |

## Interactive commands

The prompt offers a searchable slash-command palette (Tab completes, Shift+Tab
cycles plan → agentic → yolo). Every command has a wiki page with usage, behaviour,
and worked examples; the [full index](wiki/commands/README.md) mirrors `/help`.

**Codebase** —
[`/ask`](wiki/commands/ask.md),
[`/ask-codebase`](wiki/commands/ask-codebase.md),
[`/index`](wiki/commands/index.md) ([`/scan`](wiki/commands/scan.md)),
[`/init`](wiki/commands/init.md),
[`/enrich`](wiki/commands/enrich.md),
[`/summarize`](wiki/commands/summarize.md),
[`/embed`](wiki/commands/embed.md),
[`/architecture`](wiki/commands/architecture.md),
[`/relationships`](wiki/commands/relationships.md),
[`/dir-summaries`](wiki/commands/dir-summaries.md),
[`/query`](wiki/commands/query.md),
[`/visualize`](wiki/commands/visualize.md),
[`/rebuild`](wiki/commands/rebuild.md),
[`/squash`](wiki/commands/squash.md)

**Agent** —
[`/goal`](wiki/commands/goal.md),
[`/goals`](wiki/commands/goals.md),
[`/agent`](wiki/commands/agent.md),
[`/agents`](wiki/commands/agents.md) ([`/tasks`](wiki/commands/tasks.md)),
[`/effort`](wiki/commands/effort.md),
[`/skills`](wiki/commands/skills.md)

**Configuration** —
[`/mode`](wiki/commands/mode.md),
[`/model`](wiki/commands/model.md),
[`/smodel`](wiki/commands/smodel.md),
[`/emodel`](wiki/commands/emodel.md),
[`/wmodel`](wiki/commands/wmodel.md),
[`/graph`](wiki/commands/graph.md),
[`/mcp`](wiki/commands/mcp.md),
[`/allow`](wiki/commands/allow.md),
[`/deny`](wiki/commands/deny.md),
[`/vars`](wiki/commands/vars.md)

**Sessions** —
[`/btw`](wiki/commands/btw.md),
[`/fork`](wiki/commands/fork.md),
[`/resume`](wiki/commands/resume.md),
[`/clear`](wiki/commands/clear.md),
[`/compact`](wiki/commands/compact.md),
[`/history`](wiki/commands/history.md),
[`/storage`](wiki/commands/storage.md),
[`/sessions`](wiki/commands/sessions.md),
[`/max-history`](wiki/commands/max-history.md),
[`/prefix-keep`](wiki/commands/prefix-keep.md),
[`/attach`](wiki/commands/attach.md),
[`/image`](wiki/commands/image.md),
[`/document`](wiki/commands/document.md),
[`/paste`](wiki/commands/paste.md)

**General** —
[`/help`](wiki/commands/help.md),
[`/doctor`](wiki/commands/doctor.md),
[`!<command>`](wiki/commands/shell.md),
[`/cls`](wiki/commands/cls.md),
[`/exit`](wiki/commands/exit.md)

## How it works

### Modes and permissions

Three operating modes control what the model may do: **plan** (read-only, mutating
tools withheld), **agentic** (mutating tools permission-gated), and **yolo** (no
confirmation). Persistent [`/allow`](wiki/commands/allow.md) and
[`/deny`](wiki/commands/deny.md) rules sit on top; deny rules apply even in yolo
mode. File operations are restricted to the project root, and shell commands run
with a bounded timeout and capped output. See [`/mode`](wiki/commands/mode.md) and
[machine-local settings](wiki/configuration.md#ikodesettingslocaljson--machine-local-settings).

### Graph-backed retrieval

Indexing, graph traversal, and retrieval are deterministic and local; LLM calls for
summaries and embeddings are opt-in, capped, and hash-gated. The code graph is
stored in a single-writer, checksummed WAL at `.ikode/graph.log`, with torn trailing
records repaired to the last commit boundary and periodic exact-state checkpoints
([`/squash`](wiki/commands/squash.md) forces one). Graph mode is on by default;
[`/graph off`](wiki/commands/graph.md) or `--no-graph` switches a session to a
traditional file/shell harness.

### Sessions, goals, and subagents

Every conversation is a saved, resumable session. [`/fork`](wiki/commands/fork.md)
branches it, [`/btw`](wiki/commands/btw.md) asks a side question without touching
the transcript, and [`/compact`](wiki/commands/compact.md) summarises older turns
(auto-compaction kicks in near the model's context window).
[`/goal`](wiki/commands/goal.md) runs a persisted objective autonomously across many
turns; [`/agent`](wiki/commands/agent.md) spawns isolated read-only workers that
report back to the lead, and [`/effort`](wiki/commands/effort.md) controls how
deeply the model reasons and how readily it delegates. Retention is managed with
[`/sessions prune`](wiki/commands/sessions.md) and the `session_keep` /
`session_max_bytes` config keys.

### MCP servers

iKode connects to MCP tool servers over local stdio and remote Streamable HTTP:

```text
/mcp add filesystem -- npx -y @modelcontextprotocol/server-filesystem .
/mcp add remote --url https://example.com/mcp --header "Authorization=Bearer ${MCP_TOKEN}"
/mcp tools
```

Discovered tools are namespaced `mcp__<server>__<tool>` and treated as externally
mutating (permission-gated in agentic mode, withheld in plan mode, subject to deny
rules in yolo mode). See the [MCP guide](wiki/mcp.md) for registration fields,
`${NAME}` expansion, the security boundary, and troubleshooting.

### Web research

The `web_search`, `web_fetch`, and `web_research` tools use the first available of
Brave → Tavily → SearXNG (`BRAVE_API_KEY`, `TAVILY_API_KEY`, `SEARXNG_URL`), or a
pinned `web_search_backend`. Research agents run on the
[`/wmodel`](wiki/commands/wmodel.md) model, which defaults to the chat model. See
[web research configuration](wiki/configuration.md#web-research).

## Repository layout

| Path | What it is |
| --- | --- |
| [`ikode-cli/`](ikode-cli) | The CLI binary: REPL, command palette, turn loop, tools, sessions, goals, visualizer |
| [`modules/gaise/`](modules/gaise) | GAISe — provider-abstraction layer (chat, embeddings, streaming, tools); nested workspace with its own [wiki](modules/gaise/wiki/README.md) |
| [`modules/livec_graph/`](modules/livec_graph) | LivingVector — the WAL-backed property graph behind the code index; standalone crate |
| [`wiki/`](wiki/README.md) | User documentation: command reference, configuration, MCP |
| [`ikode.md`](ikode.md) | Project guidelines loaded into every session |

## Development

There are three Rust build roots because GAISe is a nested workspace and
LivingVector is a standalone crate:

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

When adding or changing a slash command, update its page under
[`wiki/commands/`](wiki/commands/README.md) and the index tables in the same change —
the `COMMANDS` registry in [`ikode-cli/src/palette.rs`](ikode-cli/src/palette.rs)
drives the palette and `/help`, and the wiki mirrors it by hand.

## License

Licensed under the GNU Affero General Public License v3.0 only. See [LICENSE](LICENSE).
