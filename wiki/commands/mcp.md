# `/mcp`

> **Category:** Configuration · **Usage:** `/mcp [list|tools|refresh|add|remove|enable|disable]` · [All commands](README.md)
>
> ⚠️ Keep in sync with the MCP module and [`docs/MCP.md`](../../docs/MCP.md). Full setup walkthrough: [MCP server guide](../mcp.md).

## What it does

Registers and manages **third-party MCP servers**, whose tools become callable by the model as
`mcp__<server>__<tool>`.

| Form | Effect |
| --- | --- |
| `/mcp` | Connection status per registered server |
| `/mcp add <name> [--env KEY=VALUE] -- <command> [args...]` | Register a local **stdio** server |
| `/mcp add <name> --url <url> [--header NAME=VALUE]` | Register a remote **Streamable HTTP** server |
| `/mcp list` | List registrations |
| `/mcp tools` | Exposed tool name → server tool mapping |
| `/mcp refresh` | Reload settings, reconnect, rediscover tools |
| `/mcp enable <name>` / `/mcp disable <name>` | Toggle a server without removing it |
| `/mcp remove <name>` | Delete the registration |

## How it works

- Implements the stable MCP `2025-11-25` lifecycle over both current transports: **stdio**
  (iKode launches the process) and **Streamable HTTP** (JSON and SSE responses, protocol
  negotiation, pagination, session IDs). The deprecated 2024 HTTP+SSE transport is not
  supported; OAuth browser flows are not yet implemented — use headers with env references.
- Registrations are stored under `mcp_servers` in the git-ignored
  `.ikode/settings.local.json` and reconnect automatically on startup. `${NAME}` references in
  env/header values expand **only at connect time**; a missing variable fails that one server
  without leaking the value or blocking others.
- **Security model**: third-party tool descriptions are untrusted, so every MCP invocation is
  treated as potentially mutating — plan mode withholds MCP tools entirely, agentic mode asks
  each time unless an [/allow](allow.md) rule matches (e.g.
  `/allow mcp__github__list_issues`), and yolo still honours deny rules. An attempted MCP call
  is treated as a possible workspace change for graph auto-sync purposes.
- Results: text, structured JSON, embedded text resources, and resource links are returned
  (capped at 512 KiB); image/audio blocks are identified but omitted. Tool-list change
  notifications are not applied live — run `/mcp refresh`.

## Use cases

- **Local filesystem server**:

  ```text
  /mcp add filesystem -- npx -y @modelcontextprotocol/server-filesystem .
  ```

- **Server that needs a secret** (expanded at connect, never stored expanded):

  ```text
  /mcp add database --env DATABASE_URL=${DATABASE_URL} -- uvx my-database-mcp
  ```

- **Hosted server with auth header**:

  ```text
  /mcp add hosted --url https://mcp.example.com/mcp --header "Authorization=Bearer ${MCP_TOKEN}"
  ```

- **Day-to-day**:

  ```text
  /mcp tools
  /mcp disable filesystem
  /mcp refresh
  ```

## Related

- [MCP server setup guide](../mcp.md) — fields, JSON layout, troubleshooting
- [/allow](allow.md) / [/deny](deny.md) — trusting individual MCP tools
- [docs/MCP.md](../../docs/MCP.md) — the full integration contract
