# `/skills`

> **Category:** Agent · **Usage:** `/skills [name|add|edit|remove]` · [All commands](README.md)
>
> ⚠️ Keep in sync with [`skills_cmd.rs`](../../ikode-cli/src/skills_cmd.rs) and [`skills.rs`](../../ikode-cli/src/skills.rs).

## What it does

Lists, runs, or manages **project skills** — reusable instruction snippets stored as Markdown
under `.ikode/skills/<name>.md`, mirroring Claude's skill convention.

| Form | Effect |
| --- | --- |
| `/skills` (or `/skills list`) | List available skills with descriptions |
| `/skills <name>` | Run that skill immediately (its body is fed to the model) |
| `/skills add <name>` | Create a new skill file and open it in your editor |
| `/skills edit <name>` | Edit an existing skill file |
| `/skills remove <name>` | Delete a skill file |

## How it works

- Each skill file may carry optional YAML-style frontmatter between `---` lines:

  ```markdown
  ---
  name: review-pr
  description: Walk a pull request and flag risky changes
  ---
  <instruction body the model receives when the skill is invoked>
  ```

  A missing `name` falls back to the file stem; a missing `description` falls back to the
  first non-empty body line.
- Listing and editing are **pure file I/O — no inference**; only invoking a skill costs model
  calls (the body becomes the prompt for a normal turn).
- Editing opens your `$EDITOR`/`$VISUAL`.
- Skill files are ordinary project files: commit `.ikode/skills/` to share team workflows.

## Use cases

- **Codify a recurring review checklist** once, run it anywhere:

  ```text
  /skills add review-pr
  /skills review-pr
  ```

- **Team-shared release process** — a committed `release-notes.md` skill that anyone runs:

  ```text
  /skills release-notes
  ```

- **Prune stale snippets**:

  ```text
  /skills
  /skills remove old-migration-helper
  ```

## Related

- [Configuration guide](../configuration.md) — what lives in `.ikode/` and what is git-ignored
