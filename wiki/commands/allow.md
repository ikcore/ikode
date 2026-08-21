# `/allow`

> **Category:** Configuration · **Usage:** `/allow [list|remove <n|rule>|clear|<rule>]` · [All commands](README.md)
>
> ⚠️ Keep in sync with `Permissions`/`rule_matches` in [`settings.rs`](../../ikode-cli/src/settings.rs).

## What it does

Manages persistent **permission allow rules** so approved classes of action stop prompting in
agentic mode.

| Form | Effect |
| --- | --- |
| `/allow` or `/allow list` | Show current allow rules (numbered) |
| `/allow <rule>` | Add a rule |
| `/allow remove <n\|rule>` | Remove by number or exact rule text |
| `/allow clear` | Remove all allow rules |

## How it works

- Rules live in `permissions.allow` in `.ikode/settings.local.json` (git-ignored — permissions
  are personal, not project policy). Writes are atomic; a failed save rolls back the in-memory
  change.
- **Rule grammar**: a bare tool name (`edit_file`) or a tool name with a glob over the action
  detail — `execute_command(cargo *)`, `delete_file(*.lock)`. `*` as the tool name matches any
  tool; `*` is the only wildcard. Matching is purely algorithmic — no inference decides
  permissions.
- **Precedence**: [/deny](deny.md) always beats allow; non-mutating tools are auto-allowed
  anyway; then the mode applies.
- **Shell-safety backstop**: even with a matching allow rule, an `execute_command` whose
  command string contains an unquoted control operator (`&&`, `;`, `|`, `<`, `>`, backticks,
  `$(...)`, newlines) still prompts — a broad prefix rule can't silently authorise a second
  command segment or a redirection.
- Network tools are never auto-allowed: `web_research(*)` is how you stop those prompts
  outside yolo.

## Use cases

- **Stop confirming harmless cargo commands**:

  ```text
  /allow execute_command(cargo check*)
  /allow execute_command(cargo test*)
  ```

- **Trust one MCP tool** (they always prompt otherwise):

  ```text
  /allow mcp__github__list_issues
  ```

- **Let web research run without prompts**:

  ```text
  /allow web_research(*)
  ```

- **Audit and prune**:

  ```text
  /allow list
  /allow remove 2
  ```

## Related

- [/deny](deny.md) — the overriding blocklist
- [/mode](mode.md) — the coarse posture these rules refine
