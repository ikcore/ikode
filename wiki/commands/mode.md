# `/mode`

> **Category:** Configuration · **Usage:** `/mode [plan|agentic|yolo]` · [All commands](README.md)
>
> ⚠️ Keep in sync with `Mode` in [`settings.rs`](../../ikode-cli/src/settings.rs).

## What it does

Shows (bare) or sets the **operating mode** — the permission posture that governs everything
the model may do. Setting a value persists it immediately to `.ikode/settings.local.json`.
**Shift+Tab** cycles the mode live at the prompt (plan → agentic → yolo).

| Mode | Behaviour |
| --- | --- |
| `plan` | Read-only. Mutating tools are **withheld from the model entirely** — not just denied. Explore and plan only. Synonyms: `planning`, `read-only`, `ro` |
| `agentic` | Default. Mutating tools **prompt for confirmation**, honouring [/allow](allow.md)/[/deny](deny.md) rules. Synonyms: `agent`, `default`, `normal`, `ask` |
| `yolo` | No prompts — every action auto-allowed **except explicit deny rules**, which always win. Synonyms: `brave`, `auto`, `unsafe` |

## How it works

- The permission engine is purely algorithmic (string/glob matching, no inference) and is
  consulted before any mutating tool runs. Precedence: explicit **deny** > non-mutating
  auto-allow > mode.
- Plan mode is *structural*: mutating tools are removed from the request, so the model can't
  even attempt them. MCP tools (always treated as mutating) are withheld too.
- Network tools (`web_research`) are read-only, so plan mode does **not** withhold them — but
  every call leaves the machine, so they prompt in plan/agentic unless an allow rule covers
  them and run freely only in yolo.
- Even in agentic mode with a broad allow rule, a shell command containing an unquoted control
  operator (`&&`, `;`, `|`, redirection, backticks, `$(...)`) still asks — a prefix rule can't
  be smuggled into a second command.
- The mode is folded into the request cache key, and each [goal](goal.md) task snapshots its
  own mode. `ikode --brave` selects yolo for one session without persisting it.

## Use cases

- **Safe exploration of an unknown repo**:

  ```text
  /mode plan
  ```

- **Normal supervised work**:

  ```text
  /mode agentic
  ```

- **Trusted batch run in a scratch clone**:

  ```text
  /mode yolo
  /deny execute_command(git push*)
  ```

## Related

- [/allow](allow.md), [/deny](deny.md) — fine-grained rules under the mode
- [/effort](effort.md) — reasoning depth is a separate axis
