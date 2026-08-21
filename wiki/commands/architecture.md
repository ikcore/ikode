# `/architecture`

> **Category:** Codebase · **Usage:** `/architecture` (alias: `/arch`) · [All commands](README.md)

## What it does

Generates a **prose overview of how the codebase fits together** — modules, responsibilities,
and connections — derived from the structural graph and printed in the terminal.

## How it works

A **single model call** against the summary model ([/smodel](smodel.md)): the indexer projects
the structural graph (directories, files, key symbols, edge structure) into a compact prompt
and asks for a narrative overview. Runs [/index](index.md) first if the index is empty, applies
`.ikignore`, and is Ctrl+C-cancellable. Because it is one bounded call, it is cheap enough to
re-run whenever the shape of the project shifts.

## Use cases

- **Onboarding** — orient yourself (or a new teammate) in an unfamiliar repo:

  ```text
  /architecture
  ```

- **Seed for docs** — a starting draft for a README or design-doc "overview" section (review
  before publishing, as with all generated text).
- **Drift check** after a large refactor — does the described architecture still match your
  intent?

## Related

- [/dir-summaries](dir-summaries.md) — finer-grained, per-directory equivalents
- [/visualize](visualize.md) — the same structural picture, interactive
