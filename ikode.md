# iKode — Project Guidelines & Structure

iKode is an agentic coding CLI (Rust) that indexes code **and** Markdown into an
in-memory, WAL-backed property graph, then uses graph + RAG retrieval to answer
and edit with minimal inference. See `docs/` for the full design.

## Workspace structure

```
ikode/                         # cargo workspace root (Cargo.toml)
├── Cargo.toml                 # [workspace] members = ["ikode-cli"], excludes modules/
├── ikode.md                   # this file — project instructions (loaded at session start)
├── docs/                      # design docs (read these first)
│   ├── iKode (Agentic CLI Harness).md
│   └── IKODE_TOOLS.md
├── ikode-cli/                 # the `ikode` binary crate
│   ├── Cargo.toml             # bin name = "ikode"
│   └── src/
│       ├── main.rs            # CLI args, REPL/slash commands, agent loop; App impls ToolHost
│       ├── tools/             # one file per tool: each has its Args, spec(), and execute()
│       │   ├── mod.rs         #   get_tools(mode), dispatch(), shared schema builders
│       │   ├── host.rs        #   ToolHost trait (tool↔session seam) + Todo
│       │   ├── read_file.rs, edit_file.rs, create_file.rs, delete_file.rs, …
│       │   └── ask_codebase.rs, search_code.rs, find_references.rs, graph_overview.rs, …
│       ├── settings.rs        # .ikode/settings.local.json: mode + allow/deny permission engine
│       ├── prompts.rs         # authored system prompts for internal sub-calls (enrichment passes)
│       ├── harness.rs         # .ikode/ discovery, config, init, graph/ask/diff rendering
│       ├── index.rs           # Indexer: chunk store + livec graph mirror + search + impact/ask
│       ├── lang.rs            # language registry + pure-Rust chunker + modifier inference
│       └── sys-prompt.md      # system prompt (compiled in via include_str!)
└── modules/                   # vendored dependencies (consumed via path deps)
    ├── gaise/                 # GAISe inference gateway — its OWN nested cargo workspace
    │   ├── gaise-core/        #   package name `gaise`, lib `gaise_core` (GaiseClient trait, contracts)
    │   ├── gaise-client/      #   GaiseClientService — routes "provider::model" strings
    │   └── gaise-provider-*/  #   ollama, openai, anthropic, gemini, vertexai, bedrock
    └── livec_graph/           # LivingVector in-memory property graph (standalone crate)
                               #   GraphService = WAL-first txn store -> .ikode/graph.log
```

**Why `modules/` are not workspace members:** `modules/gaise` is itself a cargo
workspace and a workspace cannot nest another, so both `gaise` and `livec_graph`
are wired in as **path dependencies** in `ikode-cli/Cargo.toml`, and `modules/`
is listed under `[workspace] exclude`.

**Dependency keys (ikode-cli/Cargo.toml):**
- `gaise-core = { path = "../modules/gaise/gaise-core", package = "gaise" }` (package is `gaise`, crate is `gaise_core`)
- `gaise-client = { path = "../modules/gaise/gaise-client" }`
- `livec_graph = { path = "../modules/livec_graph" }`

## Build & run

```
cargo build -p ikode            # builds the `ikode` binary
ikode init                      # scaffold .ikode/ (ikode.md, config.toml, .gitignore)
ikode                           # interactive REPL (streams tokens live)
ikode --prompt "do X"           # one-shot / headless
ikode --model "provider::model" # e.g. anthropic::claude-opus-4-8, openai::gpt-5.4-mini, ollama::...
```

Notes for this machine:
- A global cargo `target-dir = "D:/Projects/Target"` is set in `~/.cargo/config.toml`,
  so build artifacts land in `D:/Projects/Target/debug`, not `./target`.
- The indexer is **pure Rust** (heuristic chunker, no tree-sitter/native C grammars).
- Per-project state lives in `.ikode/` (e.g. `.ikode/graph.log`, the livec WAL).

## Harness features (Claude-Code-like)

Implemented to match the `docs/` design and mirror the reference CLI:
- **Streaming output.** Responses stream token-by-token via gaise `instruct_stream`
  (`GaiseStreamAccumulator` assembles the final message + tool calls).
- **`.ikode/` project model.** On start iKode finds the **project root** — nearest
  ancestor with `.ikode/`, else nearest `.git/`, else cwd — and anchors indexing,
  path validation, and instruction loading there.
  - **Project instructions:** `.ikode/ikode.md` (preferred) or root `ikode.md`
    (fallback) is appended to the system prompt — the `CLAUDE.md` equivalent.
  - **Layered config.** Resolved highest-precedence first: **CLI flags** >
    **local** `.ikode/settings.local.json` (model overrides) > **project**
    `.ikode/config.toml` > **global** `<config_dir>/ikode/config.toml`
    (e.g. `%APPDATA%\ikode\config.toml`) > built-in defaults. Keys: `model` (chat,
    default `openai::gpt-5.4-mini`), `embedding_model` (separate — changing it
    invalidates stored embeddings), `brave`, `max_history`, `prefix_keep`, `auto_index`.
  - **Provider credentials & `.env`.** API keys / endpoints are read from environment
    variables (`OPENAI_API_KEY`, `OPENAI_API_URL`, `OLLAMA_URL`, `ANTHROPIC_API_KEY`,
    `GEMINI_API_KEY`, `AWS_REGION`, `VERTEXAI_*`, …) in `App::new`. At startup iKode
    loads (in precedence order) `.ikode/.env`, `.ikode/.ikenv`, root `.ikenv`, then root
    `.env` into the environment (`harness::load_dotenv`, parser `harness::parse_dotenv`).
    `.ikenv` is an iKode-specific alias for `.env`, parsed identically: `#` comments,
    `export ` prefix, and single/double quotes (double-quoted values honour
    `\n`/`\t`/`\r`/`\"`/`\\`). **A real environment variable always wins** — files never
    override what's already set, and the first file to define a key wins. `.env`, `.env.*`,
    `.ikenv`, `.ikenv.*` are git-ignored; values are never logged (only the filename loaded
    is announced).
  - **Modes & permissions (`.ikode/settings.local.json`, git-ignored).** Operating
    mode is `plan` (read-only — mutating tools are withheld from the tool list
    entirely), `agentic` (default — mutating tools prompt, honouring rules), or
    `yolo` (no prompts, deny rules still apply). `permissions.allow`/`deny` are
    rules of the form `"tool"` or `"tool(glob)"` (e.g. `execute_command(cargo *)`);
    `deny` beats `allow`. Commands: `/mode [plan|agentic|yolo]`, `/allow <rule>`,
    `/deny <rule>` (all persist). `--brave` forces `yolo` for the session. The
    permission engine lives in `settings.rs`; `App::ask_permission` consults it.
  - **Startup drift check.** When a prior graph is loaded, iKode diffs the working
    tree against it (added/changed/deleted files, by content hash — no inference)
    and offers to re-index. Skipped under `auto_index` (which already re-syncs).
  - **Model commands.** `/model` shows the chat model, `/model {m}` switches it,
    `/model save [global]` persists it (comment-preserving); `/emodel` / `--emodel`
    are the embedding-model equivalents. CLI: `--model` / `--emodel`.
  - **`ikode init`** scaffolds `.ikode/ikode.md` + `config.toml` and adds derived
    state (`graph.log`, `embeddings/`, `tesseract/`, `plan/`) to `.gitignore`.
- **Shell escape.** `!<cmd>` in the REPL runs a command in-session and feeds its
  output back into history as context.
- **Permission modes.** Mutating tools (`execute_command`, `edit_file`,
  `create_file`) prompt **allow once / always (this session) / deny**; `--brave`
  or `brave = true` skips prompts. Edits/creates print a **coloured diff** first.
- **Interrupts.** Ctrl-C cancels the in-flight turn (history stays consistent);
  the REPL keeps running — quit with `/exit`.

## Code graph & retrieval

Built **structurally, zero inference** (pure-Rust heuristics — no tree-sitter):
- **Node kinds** mirror the design doc: `Directory`, `File`, `Function`, `Method`
  (nested in a type/trait/impl), `Struct`, `Enum`, `Class`, `Interface`, `Trait`,
  `ImplBlock`, `Module`, `TypeAlias`, `Union`, `Const`, `Macro`, `TypeParam` (generic
  param), `Section` (markdown), `Document` (plain text / config). `Directory` and
  `File` carry a **project-relative** `path` (root directory is `"."`); chunk nodes
  carry `file_path` instead (`path` holds a unique index, so many chunks must not
  collide on it). Each chunk node carries inferred **visibility**
  (`public`/`private`/`protected`/`internal`/`crate`/`default`) and modifier flags
  (`is_static`/`is_const`/`is_async`/`is_abstract`/`is_virtual`/`is_final`). Members
  are chunked recursively, so methods are their own nodes. Most languages mark
  methods with a function keyword (Rust/Python/JS/TS/Go/Kotlin/Swift/PHP/Scala/
  Solidity/Zig); **Java/C#/C/C++ have no leading keyword, so their methods (and free
  functions) are detected from the signature shape** — `<modifiers/return-type> name(…)`
  followed by `{`/`;`, with calls, field initialisers, and control-flow lines rejected
  (constructors are recognised by name == enclosing type).
- **Edge kinds:** `CONTAINS` (`Directory`→`Directory`/`File`, the project tree),
  `DEFINES` (containment — File→top-level chunk, parent chunk→member),
  `REFERENCES` (caller→callee, token match), `TESTS` (test chunk→target),
  `IMPLEMENTS`/`FOR_TYPE`/`HAS_IMPL` (Rust `impl`), `IMPLEMENTS`/`EXTENDS`
  (class/interface inheritance across langs), `SATISFIES` (impl/class method →
  trait/interface method of the same name), and `HAS_TYPE_PARAM`/`BOUND_BY`
  (a definition's generic params and their resolved bounds).
- **Decorations.** A definition's leading attributes, annotations/decorators, and
  doc/line comments (`#[…]`, `@…`, `///`, `//`, `/* … */`, C# `[…]`, Python/Ruby `#`)
  are folded into its chunk by an upward backscan, so the chunk stays semantically
  whole and `line_start` points at the topmost decoration. Inner attributes (`#![…]`)
  and inner doc comments (`//!`) belong to the enclosing module and are excluded.
- **Languages:** Rust, Python, JavaScript, TypeScript, Go, Java, C#, C, C++,
  Kotlin, Swift, Zig, PHP, Ruby, Scala, Dart, Solidity, Markdown; a **Config**
  language indexes project/build manifests whole-file (`.toml`, `.json`, `.yaml`,
  `.xml`, `.csproj`/`.vbproj`/`.fsproj`, `.sln`, `.gradle`, `.props`/`.targets`,
  `.lock`, …) — a repo may hold many, and each is one `Document` chunk that
  participates in search, embeddings, and summaries; a **Markup** language does the
  same whole-file for HTML/CSS/single-file components (`.html`/`.htm`/`.xhtml`,
  `.css`/`.scss`/`.sass`/`.less`, `.vue`/`.svelte`/`.astro`) — no native HTML/CSS
  parser is used; plain-text indexing for shell/SQL/etc. Inheritance edges resolve
  `extends`/`implements` (Java/TS/Kotlin/PHP), `: Base` (C#/C++/Swift), and
  `Foo(Base)` (Python). Reference resolution is best-effort token matching (not
  LSP-precise).
- **Tools:** `ask_codebase` (flagship — relevant chunks **+** how they're wired;
  semantic when an embedding index exists, keyword otherwise — same `ask_auto` path
  as `/ask`), `find_references` (dependents/dependencies + guarding tests),
  `graph_overview`, `list_directory` (discover structure / un-indexed projects),
  `delete_file` (removes a file **and** prunes it from the graph), alongside
  `index_codebase`/`search_code`/`outline_file`/`read_file`/`edit_file`/`create_file`/
  `execute_command`. `execute_command` is the cross-cutting fallback (rename/move, run
  tools); it spawns through `cmd.exe` on Windows and `sh` on macOS/Linux, and the
  system prompt tells the model which shell + platform it is on so commands match.
- **Commands:** `/index` (= `/scan`), `/rebuild` (wipe WAL + re-index, confirms),
  `/ask {question}`, `/graph`, `/embed`, `/enrich [max]` (summarise then embed, in
  order), `/summarize [max]`, `/architecture`, `/relationships [kind] [max]`,
  `/dir-summaries [max]`.

### Inference / semantic layer (opt-in, on top of the zero-inference graph)

The structural graph is built with no inference; these passes enrich it on demand:
- **Embeddings / semantic search (`/embed`).** `Indexer::embed_index` embeds every
  chunk via the gaise `embeddings` API and stores the vector as an `embedding`
  (`ArrayFloat`) prop on the chunk node, plus `embed_hash` and `embed_model`. The
  embedded text is **not** raw code but a composed *embedding document*
  (`embedding_document`) so summary, comments, entities and relationships all steer a
  chunk's position in the vector space:
  ```text
  Context: <kind> `<name>` (<language>)[, within <parent>]
  File: <path>
  Summary: <one-line LLM summary, when summarised>
  Signature: <first source line — structural shape>
  Comments: <captured comment text — intent>
  Keywords: <entity names the chunk uses / is>
  Relationships: <EDGE target; …>   (REFERENCES/IMPLEMENTS/EXTENDS/FOR_TYPE/SATISFIES/TESTS)
  Content:
  <full body — only when small>
  ```
  `embed_hash` is the hash of this document — the **deterministic key**. It changes
  whenever the comments, signature, summary, entities or relationships change (drawn
  via `outgoing_relations`), which is exactly when the vector should be recomputed; an
  unchanged document with a current `embed_model` is skipped. Keywords/relationships
  come from the graph, and the summary from the stored `summary` prop, so embedding
  *after* summarising (i.e. `/enrich`) yields the richest vectors. `semantic_search` /
  `ask_semantic` rank chunks by **cosine similarity** (livec_graph's built-in
  `cosine_similarity`) to an embedded query. `/ask` automatically uses semantic
  retrieval when an embedding index exists, falling back to keyword search otherwise.
- **Full enrich pass (`/enrich [max]`).** Runs `/summarize` then `/embed` in
  dependency order (so embedding documents fold in fresh summaries), each pass still
  hash-gated and incremental. The one-shot path to "semantic `/ask` is ready".
- **Index exclusions (`.ikignore`).** Root and nested `.ikignore` files remove
  matching files/directories from the structural graph, ordinary code search,
  summaries, and embeddings. The next `/index` (including startup/automatic sync) or
  model-backed enrichment command prunes anything that was indexed before it became
  ignored, including orphaned directory nodes; indexing reports every `.ikignore`
  file it discovered. Use one pattern per line; blank lines and lines beginning with
  `#` are ignored. A bare name such as `Cargo.lock` matches that file or directory
  name anywhere below the `.ikignore` containing it, while a path such as
  `generated/schema.rs` is relative to that `.ikignore`'s folder. Absolute paths are
  also accepted; prefix a bare name with `./` to anchor it directly to the
  `.ikignore` folder. `/` and `\` path separators are interchangeable; `*` and `?`
  match within one path component, while `**` matches recursively across directories.
  Directory patterns such as `generated/` exclude everything below them. Rules are
  rediscovered for every index/enrichment pass, so edits take effect without
  restarting.
- **Chunk summaries (`/summarize [max]`).** `Indexer::summarize_index` calls the chat
  model (`instruct`) for a one-line summary per chunk, stored as a `summary` prop
  (+ `summary_hash`); `max` caps calls per pass. Summaries surface on `AskChunk`.
- **Architecture overview (`/architecture`).** `summarize_architecture` turns the
  graph's label/edge counts into a prose overview in a single inference call — the
  aggregate relationship-level companion to per-chunk summaries.
- **Per-edge relationship summaries (`/relationships [kind] [max]`).**
  `summarize_relationships` describes each edge of a given kind (default `REFERENCES`,
  the call graph; also useful for `SATISFIES`) in one line via `instruct`, storing it
  as a `summary` prop **on the edge** (+ `rel_hash` over both endpoint bodies, for
  incremental skips). These descriptions surface in `/ask` connectivity. Because a
  re-`Merge` would *replace* (clear) an edge's props, `link_references` deliberately
  skips re-emitting already-existing `REFERENCES`/`SATISFIES` edges — so a stored
  summary survives re-indexing, and only changed/new edges are recreated.
- **Directory rollups (`/dir-summaries [max]`).** `summarize_directories` composes a
  one-line summary for each `Directory` node from its descendant top-level chunks
  (using their `summary` when present, else `kind name`), stored as a `summary` prop
  (+ `summary_hash` over the bounded child listing). Best run after `/summarize` so the
  rollup draws on real chunk summaries; the per-directory child list is capped so a
  large directory can't blow up the prompt.
- `TypeParam` nodes are never embedded or summarised. `Directory` nodes are not
  embedded, but *are* given a rolled-up `summary` by `/dir-summaries` (composed from
  their children, not from their own bytes). Changing a file deletes + re-mirrors its
  chunk nodes (dropping their embedding / summary), so the next `/embed` / `/summarize`
  re-derives only what changed.
- **Persistence.** All enrichment is stored as graph props and journalled to the WAL
  under `.ikode/`, so embeddings, chunk summaries, edge summaries, and directory
  rollups survive across sessions — a fresh `Indexer` replays them, and the hash-gated
  passes (`embed_hash`/`summary_hash`/`rel_hash`) then re-derive only what changed.
- **Incremental staleness pruning.** `/scan` keeps the graph honest without a full
  rebuild: a file is reconciled only when its `contentHash` differs from the File
  node's (so unchanged files — even across sessions — are skipped). When it differs,
  the file's prior chunk nodes are **deleted** (cascading every incident edge) before
  re-mirroring, so removed/renamed functions, methods, and classes — and their stale
  reference edges — don't linger; nested members are pruned with their parent.
  Deleted files (gone from disk) have their File node + all chunks pruned and are
  reported as `removed` in the `/index` summary. Relationship edges are then rebuilt
  globally, so a surviving edge can only exist between two unchanged endpoints.

## Note: token reduction via terse instruction ("cave speak")

A deliberate cost lever is "bad language" / **cave speak** — stripping articles,
pronouns, and grammar from *internal* / tool-facing text (sub-task descriptions,
tool args, scratch notes), e.g. `"read foo.rs, find fn bar, fix off-by-one"`
instead of a polished sentence. LLMs parse the keywords fine and fewer tokens
means lower cost and latency. Apply it to internal/agent-facing text **only** —
never to user-facing replies, which stay clear and readable. The larger savings
still come from graph/index retrieval (returning only the relevant chunks + line
ranges rather than whole files); cave speak trims what's left.

## Conventions
- **Chunk bodies are stored dedented.** A chunk's shared leading-whitespace prefix
  is factored into a single `indent` field rather than repeated on every line of
  `code` (so a method body indented two tabs stores those tabs once). The transform
  is lossless: `ChunkRecord::indented_code()` (= `lang::reindent`) reconstructs the
  on-disk text. All consumers (search, modifier/header parsing, reference tokens)
  are indentation-insensitive, so they read `code` directly. Single-line and
  top-level chunks have an empty `indent`.
- **Chunks include their decorations.** A definition's leading attributes,
  annotations/decorators, and doc/line comments are folded into its chunk (so
  `line_start` is the topmost decoration); the dedent above still applies to the
  combined block. Inner attributes (`#![…]`) / inner doc (`//!`) are excluded.
- **Enrichment props are additive and snake_case:** `embedding` (`ArrayFloat`),
  `embed_hash`, `embed_model`, `summary`, `summary_hash` on chunk nodes; `summary` /
  `summary_hash` on `Directory` nodes; `summary` / `rel_hash` on `REFERENCES`/
  `SATISFIES` edges. All are written by the inference passes onto existing nodes/edges
  — no `/rebuild` needed to add them, and old graphs simply lack them until the
  relevant pass runs.
- Match surrounding code style; do not add comments unless asked.
- Prefer `search_code` / `outline_file` over reading whole files.
- Never commit unless explicitly asked; never log or commit secrets.
- **Graph property keys are `snake_case`** (e.g. `chunk_id`, `file_path`,
  `line_start`, `line_end`, `content_hash`, `is_static`). Node and edge **labels**
  stay as-is — node kinds PascalCase (`File`, `ImplBlock`, `TypeParam`), edge kinds
  UPPER_SNAKE (`DEFINES`, `HAS_TYPE_PARAM`). Renaming a prop key is a schema change:
  existing `.ikode/graph.log` holds the old keys, so run `/rebuild` after changing one.
