# MCP server setup

> **⚠️ Keep this page up to date.** It mirrors [`docs/MCP.md`](../docs/MCP.md) and the MCP
> module. Protocol/transport changes, new registration fields, or permission changes must be
> reflected here in the same change.
>
> Last reviewed: 01/08/2026

iKode connects to third-party **MCP (Model Context Protocol) servers** and exposes their tools
to the model as `mcp__<server>__<tool>`. It implements the stable MCP `2025-11-25` lifecycle
via the official Rust SDK, over both current transports:

- **stdio** — iKode launches a local server process and speaks over stdin/stdout.
- **Streamable HTTP** — iKode connects to a remote endpoint, including JSON and SSE responses,
  protocol negotiation, pagination, and session IDs.

Not supported: the deprecated `2024-11-05` HTTP+SSE transport, and OAuth browser flows (use
headers with environment references instead).

## Registering a server

### Local (stdio)

```text
/mcp add <name> [--env KEY=VALUE]... -- <command> [args...]
```

| Part | Meaning |
| --- | --- |
| `<name>` | Your label for the server — becomes the `mcp__<name>__…` tool prefix |
| `--env KEY=VALUE` | Environment variables for the launched process (repeatable). Values may reference `${VARS}` |
| `--` | Separates iKode's options from the server command line |
| `<command> [args...]` | The process to launch; quoted arguments are supported |

```text
/mcp add filesystem -- npx -y @modelcontextprotocol/server-filesystem .
/mcp add database --env DATABASE_URL=${DATABASE_URL} -- uvx my-database-mcp
```

### Remote (Streamable HTTP)

```text
/mcp add <name> --url <url> [--header NAME=VALUE]...
```

| Part | Meaning |
| --- | --- |
| `--url <url>` | The MCP endpoint |
| `--header NAME=VALUE` | HTTP headers (repeatable) — typically `Authorization`. Values may reference `${VARS}` |

```text
/mcp add hosted --url https://mcp.example.com/mcp --header "Authorization=Bearer ${MCP_TOKEN}"
```

### `${NAME}` expansion

Environment references in `env` values and header values expand **only when the server
connects** — never at registration and never into the settings file. A missing variable fails
*that server* without exposing the value or preventing other servers from loading. This is the
supported way to keep tokens out of config files.

## Where registrations live

Under `mcp_servers` in the git-ignored `.ikode/settings.local.json` — machine-specific by
design (commands, endpoints, headers, env references). Servers reconnect automatically on
startup. Edit the JSON directly for advanced setups, then run `/mcp refresh`:

```json
{
  "mcp_servers": {
    "filesystem": {
      "transport": "stdio",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "."],
      "env": {},
      "enabled": true
    },
    "hosted": {
      "transport": "http",
      "url": "https://mcp.example.com/mcp",
      "headers": { "Authorization": "Bearer ${MCP_TOKEN}" },
      "enabled": true
    }
  }
}
```

| Field | Transport | Meaning |
| --- | --- | --- |
| `transport` | both | `"stdio"` or `"http"` |
| `command` / `args` | stdio | Process and argument list |
| `env` | stdio | Extra environment for the process (`${VAR}` allowed) |
| `url` | http | Endpoint URL |
| `headers` | http | Header map (`${VAR}` allowed in values) |
| `enabled` | both | `false` keeps the registration but doesn't connect |

## Managing servers

```text
/mcp                  # connection status
/mcp list             # registrations
/mcp tools            # exposed name -> server tool mapping
/mcp refresh          # reload settings, reconnect, rediscover tools
/mcp enable <name>
/mcp disable <name>
/mcp remove <name>
```

Tool-list change notifications are **not** applied live yet — run `/mcp refresh` after a
server's tools change.

## Tool exposure and the security boundary

- Tool names are sanitised and namespaced `mcp__<server>__<tool>` to avoid collisions with
  built-in tools and provider naming limits; JSON Schemas are adapted to GAISe's
  provider-neutral tool schema.
- **Third-party descriptions and annotations are untrusted**, so every MCP invocation is
  treated as potentially mutating:
  - **plan** mode does not send MCP tools to the model at all;
  - **agentic** mode asks before each invocation unless an [/allow](commands/allow.md) rule
    matches (e.g. `/allow mcp__github__list_issues` to trust one tool);
  - **yolo** mode runs them without prompting, but explicit [/deny](commands/deny.md) rules
    still win (e.g. `/deny mcp__someserver__*`).
- A tool call may have changed external or workspace state even when the server reports an
  error, so graph auto-sync treats any *attempted* MCP call as a possible workspace change.
- Results: text, structured JSON, embedded text resources, and resource links are returned to
  the model; image and audio blocks are identified but omitted (text-only transcript). Text
  results are capped at **512 KiB**.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Server shows disconnected | `/mcp` for status; does the command exist / is the URL reachable? Try `/mcp refresh` |
| "Missing variable" on connect | The `${VAR}` referenced in env/headers isn't set — see [/vars](commands/vars.md) and the [env layering rules](configuration.md#env-file-layering) |
| Tools missing after a server update | `/mcp refresh` (tool-list notifications aren't live) |
| Model never uses the tools | Plan mode withholds MCP tools — check [/mode](commands/mode.md) |
| Constant permission prompts | Add targeted allow rules per tool; avoid blanket-allowing a whole server unless you trust it like your own shell |
| Remote server wants OAuth | Not yet supported — use a token header with `${VAR}` expansion |

## Related

- [/mcp command page](commands/mcp.md)
- [docs/MCP.md](../docs/MCP.md) — the full integration contract
- [Configuration guide](configuration.md)
