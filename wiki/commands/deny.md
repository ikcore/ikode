# `/deny`

> **Category:** Configuration · **Usage:** `/deny [list|remove <n|rule>|clear|<rule>]` · [All commands](README.md)

## What it does

Manages persistent **permission deny rules**. Deny is the strongest word in the permission
engine: a matching deny rule refuses the action **in every mode — including yolo** — and beats
any allow rule.

| Form | Effect |
| --- | --- |
| `/deny` or `/deny list` | Show current deny rules (numbered) |
| `/deny <rule>` | Add a rule |
| `/deny remove <n\|rule>` | Remove by number or exact rule text |
| `/deny clear` | Remove all deny rules |

## How it works

Same storage and grammar as [/allow](allow.md): rules in `permissions.deny` in
`.ikode/settings.local.json`; `tool` or `tool(glob)` with `*` as the only wildcard; matched
algorithmically against the tool name and its action detail (command string, file path, MCP
tool name, research question). Deny rules are checked **first** — before auto-allow for
read-only tools, before the mode, before allow rules.

## Use cases

- **Hard lines that survive yolo mode**:

  ```text
  /deny execute_command(git push*)
  /deny delete_file(*)
  ```

- **Fence off a sensitive path**:

  ```text
  /deny edit_file(.github/workflows/*)
  ```

- **Block an untrusted MCP server's writes wholesale**:

  ```text
  /deny mcp__somesever__*
  ```

## Related

- [/allow](allow.md) — the grammar and matching details
- [/mode](mode.md) — deny works underneath all three modes
