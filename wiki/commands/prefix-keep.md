# `/prefix-keep`

> **Category:** Sessions · **Usage:** `/prefix-keep <n> [save [global]] | save [global]` · [All commands](README.md)

## What it does

Sets (or persists) the **stable history prefix** — the number of leading messages kept verbatim
at the front of every request.

| Form | Effect |
| --- | --- |
| `/prefix-keep 8` | Set for this session |
| `/prefix-keep 8 save [global]` | Set and persist (`prefix_keep` in config) |

## How it works

Providers with **prompt caching** bill cached prefix tokens at a fraction of the normal rate —
but only if the prefix is byte-stable between requests. When [/max-history](max-history.md)
trimming would otherwise shift the whole window, the first `prefix_keep` messages are pinned so
the request prefix (system context + early turns) stays identical and the provider cache keeps
hitting. The cached-token counter in [/history](history.md) shows the effect. Cache lanes
rotate deliberately when the prefix genuinely changes ([/compact](compact.md), [/clear](clear.md)).

## Use cases

- **Long sessions on cache-supporting providers** — keep the early project briefing pinned:

  ```text
  /prefix-keep 8 save
  ```

- **Cache misses despite a stable session** — check the interplay of `max_history` and
  `prefix_keep` values with `/history`.

## Related

- [/max-history](max-history.md) — the trimming this protects against
- [/history](history.md) — cached-token evidence it's working
