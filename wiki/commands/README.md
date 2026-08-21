# Slash-command reference

> **⚠️ Keep these pages up to date.** Each command has its own page in this folder, mirroring the
> `COMMANDS` registry in [`ikode-cli/src/palette.rs`](../../ikode-cli/src/palette.rs) (which also
> renders `/help`) and the dispatch in
> [`ikode-cli/src/repl/command_loop.rs`](../../ikode-cli/src/repl/command_loop.rs). **Any change
> to a command's name, usage, aliases, or behaviour must update its page here in the same
> change** — and a new command needs a new page plus an index row below and in the
> [wiki home](../README.md).
>
> Last reviewed: 01/08/2026

## How commands work

- Type `/` at the prompt to open the **live command palette**: typing narrows the list
  (prefix > substring > fuzzy subsequence), ↑/↓ scroll, **Tab** or **Enter** accepts the
  highlighted suggestion, and a second **Enter** runs it.
- **Shift+Tab** cycles the operating mode (plan → agentic → yolo) at any time — see
  [/mode](mode.md).
- **`!<command>`** runs a shell command and feeds its output back to the model — see
  [Shell escape](shell.md).
- Arguments shown as `[optional]` may be omitted; `<required>` may not.
- A `/`-prefixed line that matches no command is reported as a typo (with close matches)
  rather than being sent to the model.

## General

| Command | Summary |
| --- | --- |
| [/help](help.md) | Display the command reference |
| [/doctor](doctor.md) | Check providers, configuration, graph and session storage |
| [/cls](cls.md) | Clear the terminal |
| [/exit](exit.md) | Quit the interactive session |
| [`!<command>`](shell.md) | Run a shell command and feed its output to the model |

## Agent

| Command | Summary |
| --- | --- |
| [/goal](goal.md) | Start, resume, steer, switch, or close an autonomous goal |
| [/goals](goals.md) | List all goal tasks and statuses |
| [/agent](agent.md) | Spawn and control parallel subagent threads |
| [/agents](agents.md) | List subagent threads and statuses |
| [/tasks](tasks.md) | Alias for `/agents` |
| [/effort](effort.md) | Show or set reasoning and delegation effort |
| [/skills](skills.md) | List, run, or manage project skills |

## Codebase

| Command | Summary |
| --- | --- |
| [/ask](ask.md) | Answer from retrieved code with LLM synthesis |
| [/ask-codebase](ask-codebase.md) | Show raw relevant chunks and connectivity |
| [/index](index.md) | Build or refresh the code and Markdown index |
| [/scan](scan.md) | Alias for `/index` |
| [/init](init.md) | One-shot index, summary and embedding warm-up |
| [/enrich](enrich.md) | Run summary then embedding passes |
| [/summarize](summarize.md) | Summarise chunks through the summary model |
| [/embed](embed.md) | Build the semantic embedding index |
| [/architecture](architecture.md) | Generate a prose codebase overview |
| [/relationships](relationships.md) | Describe graph relationships |
| [/dir-summaries](dir-summaries.md) | Roll up one summary per directory |
| [/query](query.md) | Run a structured start/traverse/where graph query |
| [/visualize](visualize.md) | Serve the interactive code map |
| [/rebuild](rebuild.md) | Wipe and rebuild derived graph data |
| [/squash](squash.md) | Checkpoint exact graph state and discard WAL history |

## Configuration

| Command | Summary |
| --- | --- |
| [/mode](mode.md) | Show or persist the operating mode (plan/agentic/yolo) |
| [/model](model.md) | Show or switch the chat model |
| [/smodel](smodel.md) | Show or switch the summary model |
| [/emodel](emodel.md) | Show or switch the embedding model |
| [/wmodel](wmodel.md) | Show or switch the web-research agent model |
| [/graph](graph.md) | Show, switch, or persist graph/index mode |
| [/mcp](mcp.md) | Register and manage third-party MCP servers |
| [/allow](allow.md) | Manage persistent permission allow rules |
| [/deny](deny.md) | Manage persistent permission deny rules |
| [/vars](vars.md) | List iKode-related environment variables (keys masked) |

## Sessions

| Command | Summary |
| --- | --- |
| [/btw](btw.md) | Ask in an ephemeral read-only side chat |
| [/fork](fork.md) | Clone this chat into a fresh saved branch |
| [/resume](resume.md) | Pick and resume a saved session |
| [/clear](clear.md) | Reset history and begin a new saved session |
| [/compact](compact.md) | Summarise older turns to shrink context; `auto` shows/sets the auto-compact threshold |
| [/history](history.md) | Show history, token, transcript and byte statistics |
| [/storage](storage.md) | Show graph WAL and saved-session storage usage |
| [/sessions](sessions.md) | Inspect or safely prune saved transcripts |
| [/max-history](max-history.md) | Set or persist the request history limit |
| [/prefix-keep](prefix-keep.md) | Set or persist the stable history prefix |
| [/attach](attach.md) | Queue an image or document for the next message |
| [/image](image.md) | Queue a local PNG/JPEG/GIF/WebP |
| [/document](document.md) | Queue a local document |
| [/paste](paste.md) | Queue an image from the clipboard |

## Related guides

- [Configuration & environment variables](../configuration.md)
- [MCP server setup](../mcp.md)
- [Wiki home](../README.md)
