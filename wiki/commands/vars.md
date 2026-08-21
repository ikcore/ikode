# `/vars`

> **Category:** Configuration · **Usage:** `/vars` · [All commands](README.md)
>
> ⚠️ Keep in sync with `print_vars` in [`repl.rs`](../../ikode-cli/src/repl.rs) — adding a provider means adding rows there and here.

## What it does

Lists the environment variables iKode reads, grouped by area, with secret values **masked to
`***` plus the last four characters** — full secrets are never printed.

Groups and variables:

- **Providers**: `OPENAI_API_KEY`, `OPENAI_API_URL`, `OPENAI_API_TIER`, `ANTHROPIC_API_KEY`,
  `ANTHROPIC_API_URL`, `GEMINI_API_KEY`, `GEMINI_API_URL`, `OLLAMA_URL`, `AWS_REGION`,
  `BEDROCK_REGION`, `VERTEXAI_API_URL`, `VERTEXAI_API_TIER`, `VERTEXAI_SA_PATH`,
  `GOOGLE_ACCOUNT_ID`, `GOOGLE_PRIVATE_KEY`
- **Web research**: `BRAVE_API_KEY`, `TAVILY_API_KEY`, `SEARXNG_URL`

## How it works

Values reflect the **live process environment**, so anything loaded from `.env` / `.ikenv` /
`.ikode/.env` at startup is included — this is the quickest way to confirm which file actually
won for a given key (see the [precedence rules](../configuration.md#env-file-layering)). Unset
variables show as "not set".

## Use cases

- **"Which key is it actually using?"** — after juggling `.env` files:

  ```text
  /vars
  ```

- **Verify web research is configured** before expecting `web_research` to work.

## Related

- [Configuration guide](../configuration.md) — every variable explained, with layering rules
- [/doctor](doctor.md) — the same data interpreted as a health check
