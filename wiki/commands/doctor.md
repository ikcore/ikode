# `/doctor`

> **Category:** General · **Usage:** `/doctor` · [All commands](README.md)
>
> ⚠️ Keep in sync with `run_doctor` in [`repl.rs`](../../ikode-cli/src/repl.rs).

## What it does

Runs a one-shot diagnostic of the session's environment and prints a report covering:

- **Project root** and the three resolved models (chat, embedding, summary).
- **Provider credentials** — for every provider referenced by those models it reports the
  relevant credential status: `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`,
  `AWS_REGION` (Bedrock), the Vertex AI service-account setup, or "local provider; no API key
  required" for Ollama.
- **Config files** — whether `.ikode/config.toml` and `.ikode/settings.local.json` exist.
- **Graph health** — node/edge counts, or "disabled (traditional harness)" when
  [/graph](graph.md) is off, or "unavailable/in-memory only".
- **Web research** — enabled (with backend description) or disabled with the hint to set
  `BRAVE_API_KEY`/`TAVILY_API_KEY` or `web_search_endpoint`.
- **MCP** — connected server count out of registered, plus total exposed tools.
- **Storage** — the same WAL and session figures as [/storage](storage.md).

## How it works

Purely local and read-only: it inspects the live process environment (so anything loaded from
`.env`/`.ikenv` files at startup counts), stats files on disk, and queries the in-memory
indexer and MCP manager. It makes **no model calls** and never prints full secrets.

## Use cases

- **First-run setup** — verify credentials landed before spending tokens:

  ```text
  /doctor
  ```

- **"Provider call failed"** — check whether the key is missing, or the wrong provider prefix
  is selected on the model.
- **Graph looks stale or empty** — confirm graph mode is on and the WAL is healthy before
  reaching for [/rebuild](rebuild.md).

## Related

- [/vars](vars.md) — the raw environment-variable view
- [Configuration guide](../configuration.md) — where each value comes from
- [/storage](storage.md), [/graph](graph.md), [/mcp](mcp.md)
