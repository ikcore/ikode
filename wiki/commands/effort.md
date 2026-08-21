# `/effort`

> **Category:** Agent · **Usage:** `/effort [auto|low|med|high|max|ultra]` · [All commands](README.md)
>
> ⚠️ Keep in sync with `Effort` in [`settings.rs`](../../ikode-cli/src/settings.rs).

## What it does

Shows (bare) or sets the session's **reasoning and delegation effort**. Setting a value
persists it immediately to `.ikode/settings.local.json`; the `--effort` CLI flag overrides it
for one process.

| Level | Meaning |
| --- | --- |
| `auto` | Preserve the selected provider/model default (also: `default`, `model`) |
| `low` | Fast, economical work for straightforward tasks (also: `lo`) |
| `med` | Balanced reasoning for normal coding work (also: `medium`, `mid`) |
| `high` | Deeper checking for complex logic and edge cases (also: `hi`) |
| `max` | Maximum model reasoning for especially difficult work (also: `xhigh`) |
| `ultra` | Maximum reasoning **plus proactive parallel subagents** |

## How it works

- Effort is **provider-neutral at the harness boundary** and translated per model in
  `Effort::api_value`: OpenAI's deepest common level is `xhigh` (supported GPT-5.x families),
  Claude's is `max` (with `high` clamped for Opus 4.5), Gemini tops out at `high`. Known
  non-reasoning families (e.g. `gpt-4*`, embeddings) get no effort parameter at all, Ollama has
  no such field, and unsupported Bedrock families omit it — so a level never causes an API
  rejection for a *known* model.
- Model-specific minimums are clamped (e.g. `gpt-5-pro` only accepts `high`; later Pro models
  clamp a requested `low` up to `medium`).
- `ultra` is more than an API scalar: it sends the deepest supported value **and** switches the
  harness to proactive delegation (`allows_proactive_agents`), so the model spawns bounded
  read-only [subagents](agent.md) when independent work would help. Workers under an ultra
  lead inherit `high` (not ultra) to keep fan-out affordable.

## Use cases

- **Check where you are**:

  ```text
  /effort
  ```

- **Crank up for a gnarly concurrency bug**:

  ```text
  /effort max
  ```

- **Big multi-file investigation where parallelism helps**:

  ```text
  /effort ultra
  ```

- **One cheap batch run** without touching the saved setting:

  ```bash
  ikode --effort low --prompt "reformat the changelog"
  ```

## Related

- [/agent](agent.md) — what proactive delegation spawns
- [/mode](mode.md) — permissions are a separate axis from effort
- [Effort and subagent controls](../../docs/EFFORT_AND_AGENTS.md)
