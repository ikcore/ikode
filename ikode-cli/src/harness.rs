//! Project-model helpers: `.ikode/` discovery, per-project config, `ikode init`
//! scaffolding, and graph-overview rendering. Kept free of the agent loop so it
//! is straightforward to unit-test.

use anyhow::Result;
use colored::*;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::index::{AskAnswer, AskResult, ConnEdge, GraphStats};

/// iKode configuration. Resolved by layering, highest precedence first:
/// CLI flags > project `.ikode/config.toml` > global `<config_dir>/ikode/config.toml`
/// and finally built-in defaults. The `model` is the chat model; `embedding_model` is a
/// *separate* setting because changing it invalidates stored embeddings.
#[derive(Debug, Default, Deserialize)]
pub struct ProjectConfig {
    pub model: Option<String>,
    pub embedding_model: Option<String>,
    /// Model used for summarisation passes (chunks, architecture, directories,
    /// relationships, `/compact`). A `None` here falls back to the chat default.
    pub summary_model: Option<String>,
    pub brave: Option<bool>,
    pub max_history: Option<usize>,
    pub prefix_keep: Option<usize>,
    pub auto_index: Option<bool>,
    /// Enable the LivingVector graph/index tool surface. Defaults to true for
    /// backwards compatibility; false runs iKode as a traditional file harness.
    pub graph_enabled: Option<bool>,
    /// Warn once saved transcripts exceed this many bytes. `/sessions prune`
    /// uses it as the default budget when no `--max-bytes` is supplied.
    pub session_max_bytes: Option<u64>,
    /// Default number of newest transcripts retained by `/sessions prune`.
    pub session_keep: Option<usize>,
    /// After an agent turn that changed files, automatically re-index then summarise +
    /// embed the affected chunks (default `false`). Both passes are hash-gated, so only
    /// new/changed chunks cost model calls; the pass is Ctrl+C-escapable.
    pub auto_enrich: Option<bool>,
    /// Summarise test chunks during enrichment (default `false`). Tests are always
    /// indexed and embedded; this only governs the opt-in LLM summary pass.
    pub summarize_tests: Option<bool>,
    /// Skip summarising non-callable chunks below this many non-blank body lines
    /// (default `4`). Functions/methods are summarised regardless of size.
    pub summarize_min_lines: Option<usize>,
    /// Number of concurrent summary calls during enrichment (default `4`).
    pub summarize_concurrency: Option<usize>,
    /// Maximum model turns the autonomous `/goal` loop takes before pausing for the
    /// user (default `25`). A safety budget so a goal can never run unbounded; the
    /// loop is also Ctrl+C-escapable and stops early on `task_complete`/`task_block`.
    pub goal_max_steps: Option<usize>,
    /// Maximum concurrently running read-only subagent threads (default `4`).
    pub agent_max_threads: Option<usize>,
    /// Maximum model turns per spawned subagent before it fails closed (default `12`).
    pub agent_max_turns: Option<usize>,
    /// Model web-research agents run on, as `provider::model`. Research is
    /// summarisation work, so a cheap/fast model fits; falls back to the chat
    /// model. Set during a session with `/wmodel`.
    pub web_model: Option<String>,
    /// Pin the search backend (`"brave"`, `"tavily"`, `"searxng"`). Unset:
    /// first available of Brave → Tavily → SearXNG wins.
    pub web_search_backend: Option<String>,
    /// Self-hosted SearXNG base URL (`SEARXNG_URL` env also works).
    pub web_search_endpoint: Option<String>,
    /// Per-research-task budgets (defaults `3` searches / `4` fetches; a
    /// depth-2 research doubles them).
    pub web_max_searches: Option<usize>,
    pub web_max_fetches: Option<usize>,
    /// Maximum concurrently running web agents, within `agent_max_threads`
    /// (default `2`).
    pub web_max_agents: Option<usize>,
    /// Maximum model turns per web agent (default `8`).
    pub web_agent_max_turns: Option<usize>,
}

/// Which config file a `save` targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigScope {
    Project,
    Global,
}

/// Path to the per-project config file (`<root>/.ikode/config.toml`).
pub fn project_config_path(root: &Path) -> PathBuf {
    root.join(".ikode").join("config.toml")
}

/// Path to the global user config (`<config_dir>/ikode/config.toml`), e.g.
/// `%APPDATA%\ikode\config.toml` on Windows or `~/.config/ikode/config.toml`.
/// `None` if the platform config directory can't be determined.
pub fn global_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("ikode").join("config.toml"))
}

impl ProjectConfig {
    /// Read a single config file, tolerating a missing file (returns default) but
    /// warning on a present-but-invalid one.
    fn read(path: &Path, label: &str) -> Self {
        match fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                eprintln!("{} Invalid {}: {}", "⚠️ ".yellow(), label, e);
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Load just the project config (`.ikode/config.toml`).
    pub fn load(root: &Path) -> Self {
        Self::read(&project_config_path(root), ".ikode/config.toml")
    }

    /// Load just the global config, if any.
    pub fn load_global() -> Self {
        match global_config_path() {
            Some(p) => Self::read(&p, "global config.toml"),
            None => Self::default(),
        }
    }

    /// Resolve global + project into one config, project winning field-by-field.
    pub fn resolve(root: &Path) -> Self {
        let global = Self::load_global();
        let project = Self::load(root);
        project.over(global)
    }

    /// Overlay `self` on top of `base`: each `Some` field in `self` wins.
    fn over(self, base: Self) -> Self {
        Self {
            model: self.model.or(base.model),
            embedding_model: self.embedding_model.or(base.embedding_model),
            summary_model: self.summary_model.or(base.summary_model),
            brave: self.brave.or(base.brave),
            max_history: self.max_history.or(base.max_history),
            prefix_keep: self.prefix_keep.or(base.prefix_keep),
            auto_index: self.auto_index.or(base.auto_index),
            graph_enabled: self.graph_enabled.or(base.graph_enabled),
            session_max_bytes: self.session_max_bytes.or(base.session_max_bytes),
            session_keep: self.session_keep.or(base.session_keep),
            auto_enrich: self.auto_enrich.or(base.auto_enrich),
            summarize_tests: self.summarize_tests.or(base.summarize_tests),
            summarize_min_lines: self.summarize_min_lines.or(base.summarize_min_lines),
            summarize_concurrency: self.summarize_concurrency.or(base.summarize_concurrency),
            goal_max_steps: self.goal_max_steps.or(base.goal_max_steps),
            agent_max_threads: self.agent_max_threads.or(base.agent_max_threads),
            agent_max_turns: self.agent_max_turns.or(base.agent_max_turns),
            web_model: self.web_model.or(base.web_model),
            web_search_backend: self.web_search_backend.or(base.web_search_backend),
            web_search_endpoint: self.web_search_endpoint.or(base.web_search_endpoint),
            web_max_searches: self.web_max_searches.or(base.web_max_searches),
            web_max_fetches: self.web_max_fetches.or(base.web_max_fetches),
            web_max_agents: self.web_max_agents.or(base.web_max_agents),
            web_agent_max_turns: self.web_agent_max_turns.or(base.web_agent_max_turns),
        }
    }
}

/// Persist a single string key to the chosen config file, preserving existing
/// comments and formatting (creates the file/dirs if absent). Returns the path
/// written. Used by `/model save` and `/emodel save`.
pub fn save_config_value(
    scope: ConfigScope,
    root: &Path,
    key: &str,
    value: &str,
) -> Result<PathBuf> {
    save_config_typed(scope, root, key, value)
}

/// As [`save_config_value`] but for any TOML scalar (`&str`, `bool`, `i64`, …),
/// preserving comments/formatting. Used by the `update_settings` tool, which writes
/// typed knobs like `summarize_tests` (bool) and `summarize_concurrency` (integer).
pub fn save_config_typed<V: Into<toml_edit::Value>>(
    scope: ConfigScope,
    root: &Path,
    key: &str,
    value: V,
) -> Result<PathBuf> {
    let path = match scope {
        ConfigScope::Project => project_config_path(root),
        ConfigScope::Global => {
            global_config_path().ok_or_else(|| anyhow::anyhow!("no platform config directory"))?
        }
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let mut doc = existing
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| anyhow::anyhow!("invalid TOML in {}: {}", path.display(), e))?;
    doc[key] = toml_edit::value(value);
    crate::util::atomic_write(&path, doc.to_string().as_bytes())?;
    Ok(path)
}

/// Locate the project root: nearest ancestor with `.ikode/`, else nearest with
/// `.git/`, else the starting directory.
pub fn find_project_root(start: &Path) -> PathBuf {
    let mut git_root: Option<PathBuf> = None;
    let mut dir = Some(start);
    while let Some(d) = dir {
        if d.join(".ikode").is_dir() {
            return d.to_path_buf();
        }
        if git_root.is_none() && d.join(".git").exists() {
            git_root = Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    git_root.unwrap_or_else(|| start.to_path_buf())
}

/// Load `.env`-style files into the process environment before any provider keys are
/// read. Looks (in precedence order) at `.ikode/.env`, `.ikode/.ikenv`, root `.ikenv`,
/// then root `.env` — `.ikenv` is an iKode-specific alias for `.env`, parsed identically.
///
/// Precedence against the real environment differs by scope: files under `.ikode/`
/// are explicit, project-scoped iKode configuration, so they **override** inherited
/// environment variables; root-level `.ikenv`/`.env` are frequently shared with other
/// tooling, so they never clobber what's already set. Among files, the first to
/// define a key wins. Returns the relative paths actually applied (for a quiet
/// startup confirmation). Values are never logged.
pub fn load_dotenv(root: &Path) -> Vec<String> {
    let mut loaded = Vec::new();
    let mut applied_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (rel, overrides_env) in [
        (".ikode/.env", true),
        (".ikode/.ikenv", true),
        (".ikenv", false),
        (".env", false),
    ] {
        let path = root.join(rel);
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut applied = 0usize;
        for (key, value) in parse_dotenv(&content) {
            // A key set by an earlier (higher-precedence) file stays set.
            if applied_keys.contains(&key) {
                continue;
            }
            // Root-level files don't clobber the real environment; .ikode/ files do.
            if !overrides_env && std::env::var_os(&key).is_some() {
                continue;
            }
            std::env::set_var(&key, &value);
            applied_keys.insert(key);
            applied += 1;
        }
        if applied > 0 {
            loaded.push(rel.to_string());
        }
    }
    loaded
}

/// Parse `.env`-style content into `(key, value)` pairs. **Pure** — does not touch the
/// environment, so it is unit-testable without global state. Supports `#` comment lines,
/// blank lines, an optional `export ` prefix, and single/double-quoted values (double
/// quotes honour `\n` `\t` `\r` `\"` `\\` escapes; single quotes are literal). Keys must
/// be `[A-Za-z0-9_]`. Unquoted values are taken verbatim (trailing `# comments` are *not*
/// stripped, so secrets containing `#` survive — quote a value if it needs trimming).
/// Malformed lines are skipped.
pub fn parse_dotenv(content: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line
            .strip_prefix("export ")
            .map(str::trim_start)
            .unwrap_or(line);
        let (key, value) = match line.split_once('=') {
            Some(kv) => kv,
            None => continue,
        };
        let key = key.trim();
        // A valid env key starts with a letter/underscore, then letters/digits/underscore.
        let mut kc = key.chars();
        let valid = matches!(kc.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
            && kc.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            continue;
        }
        out.push((key.to_string(), unquote_env_value(value.trim())));
    }
    out
}

/// Strip a matching pair of surrounding quotes from a `.env` value and unescape a
/// double-quoted value. An unquoted value is returned as-is.
fn unquote_env_value(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if first == last && (first == b'"' || first == b'\'') {
            let inner = &s[1..s.len() - 1];
            return if first == b'"' {
                unescape_double_quoted(inner)
            } else {
                inner.to_string()
            };
        }
    }
    s.to_string()
}

/// Single-pass unescape for a double-quoted `.env` value (avoids the double-handling
/// bug of chained `replace` calls).
fn unescape_double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

pub const INIT_IKODE_MD: &str = r#"# Project Instructions (ikode.md)

Loaded into iKode's system prompt at session start — the CLAUDE.md equivalent.
Put standing project context here: conventions, build/test commands, architecture
notes, and "always do X / never do Y" rules. This file should be committed.

## Build & test

<!-- e.g. cargo build / cargo test -->

## Conventions

<!-- e.g. match surrounding style; prefer search_code/outline_file over reading whole files -->
"#;

pub const INIT_CONFIG_TOML: &str = r#"# iKode per-project configuration. CLI flags override these values.

# Default chat model, as "provider::model".
# model = "openai::gpt-5.6-luna"

# Embedding model, as "provider::model". Kept separate from the chat model:
# changing it invalidates and re-derives stored embeddings under .ikode/embeddings/.
# embedding_model = "openai::text-embedding-3-small"

# Summary model, as "provider::model". Used for summarisation passes (/summarize,
# /architecture, /dir-summaries, /relationships, /compact). Defaults to openai::chat-5.6-luna.
# summary_model = "openai::chat-5.6-luna"

# Skip confirmation prompts for shell/edit/create.
# brave = false

# Max history messages sent per request (0 = unlimited).
# max_history = 80

# Early messages always kept for cache stability.
# prefix_keep = 4

# Build the code+markdown index automatically at startup.
# auto_index = false

# Enable graph/index retrieval and its model tools. Set false for a traditional
# file/shell harness; /graph on|off can also switch this during a session.
# graph_enabled = true

# Optional saved-session retention targets. They are never deleted automatically;
# `/sessions prune` previews what would be removed and `--apply` confirms it.
# session_keep = 50
# session_max_bytes = 536870912 # 512 MiB

# After an agent turn that changed files, re-index then summarise + embed the
# affected chunks (hash-gated, so only changed chunks cost model calls).
# auto_enrich = false

# --- Enrichment: the opt-in, hash-gated chunk-summary pass (/enrich, /summarize) ---
# Tests are always indexed and embedded; these only govern the LLM summary pass.

# Summarise test chunks too (their name is usually already descriptive).
# summarize_tests = false

# Skip summarising non-callable chunks below this many non-blank body lines
# (e.g. 3-line serde arg structs). Functions/methods are summarised regardless.
# summarize_min_lines = 4

# Concurrent summary calls during enrichment.
# summarize_concurrency = 4

# --- Goals: the autonomous /goal loop ---
# Maximum model turns a goal takes before pausing for you (Ctrl+C also escapes,
# and it stops early on task_complete/task_block).
# goal_max_steps = 25

# --- Parallel read-only subagents (/agent, /agents) ---
# Maximum workers running at once and maximum model turns per worker. Subagents
# have no write/shell/orchestration tools and use an ephemeral in-memory index.
# agent_max_threads = 4
# agent_max_turns = 12

# --- Web research (web_research tool → sandboxed web agents) ---
# Enabled when a search backend is available: set BRAVE_API_KEY or TAVILY_API_KEY
# in .ikode/.ikenv, or point web_search_endpoint at a self-hosted SearXNG.
# Web agents run on web_model (default: the chat model; /wmodel changes it live),
# with hard per-task budgets and their own concurrency lane inside
# agent_max_threads. JS-heavy pages render via your installed Chrome/Edge when
# one is found at startup.
# web_model = "openai::gpt-5.4-mini"
# web_search_backend = "brave"        # brave | tavily | searxng (default: first available)
# web_search_endpoint = ""            # SearXNG base URL (or SEARXNG_URL env)
# web_max_searches = 3
# web_max_fetches = 4
# web_max_agents = 2
# web_agent_max_turns = 8
"#;

/// A starter `.ikode/settings.local.json`. Machine-local and git-ignored: it sets
/// the operating mode and per-action allow/deny rules. `mode` is one of
/// `plan` (read-only), `agentic` (prompt before mutating — the default), or `yolo`
/// (no prompts). Rules are `"tool"` or `"tool(glob)"`; `deny` beats `allow`.
pub const INIT_SETTINGS_LOCAL_JSON: &str = r#"{
  "mode": "agentic",
  "effort": "auto",
  "mcp_servers": {},
  "permissions": {
    "allow": [
      "execute_command(cargo *)",
      "execute_command(git status*)"
    ],
    "deny": [
      "execute_command(rm *)",
      "execute_command(del *)"
    ]
  }
}
"#;

pub const GITIGNORE_MARKER: &str = "# iKode derived state";
pub const GITIGNORE_BLOCK: &str = "\n# iKode derived state (rebuildable cache — safe to delete)\n.ikode/graph.log*\n.ikode/embeddings/\n.ikode/tesseract/\n.ikode/plan/\n.ikode/sessions/\n.ikode/settings.local.json\n";

/// Scaffold the `.ikode/` project folder per the docs layout.
pub fn run_init(root: &Path) -> Result<()> {
    let ikode_dir = root.join(".ikode");
    fs::create_dir_all(&ikode_dir)?;
    println!("{} Project root: {}", "📁".bright_green(), root.display());

    write_if_absent(
        &ikode_dir.join("ikode.md"),
        INIT_IKODE_MD,
        ".ikode/ikode.md",
    )?;
    write_if_absent(
        &ikode_dir.join("config.toml"),
        INIT_CONFIG_TOML,
        ".ikode/config.toml",
    )?;
    write_if_absent(
        &ikode_dir.join("settings.local.json"),
        INIT_SETTINGS_LOCAL_JSON,
        ".ikode/settings.local.json",
    )?;
    ensure_gitignore(root)?;

    println!(
        "{} Initialized. ikode.md and config.toml are committed; graph.log and other state are git-ignored.",
        "✅ ".bright_green()
    );
    Ok(())
}

fn write_if_absent(path: &Path, contents: &str, label: &str) -> Result<()> {
    if path.exists() {
        println!(
            "  {} {} already exists, leaving unchanged",
            "•".dimmed(),
            label
        );
    } else {
        fs::write(path, contents)?;
        println!("  {} created {}", "+".bright_green(), label);
    }
    Ok(())
}

pub fn ensure_gitignore(root: &Path) -> Result<()> {
    let path = root.join(".gitignore");
    let existing = fs::read_to_string(&path).unwrap_or_default();
    if existing.contains(GITIGNORE_MARKER) {
        println!("  {} .gitignore already has iKode entries", "•".dimmed());
        return Ok(());
    }
    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(GITIGNORE_BLOCK);
    fs::write(&path, updated)?;
    println!("  {} added iKode entries to .gitignore", "+".bright_green());
    Ok(())
}

/// Render an `ask_codebase` result as plain text (no ANSI) — suitable both for
/// feeding back to the model as a tool result and for printing in the REPL.
pub fn render_ask_result(question: &str, result: &AskResult) -> String {
    if result.chunks.is_empty() {
        return format!(
            "No chunks matched '{}'. Try /index or a different phrasing.",
            question
        );
    }
    let mut out = format!(
        "Relevant chunks for '{}' ({}):\n",
        question,
        result.chunks.len()
    );
    for c in &result.chunks {
        out.push_str(&format!(
            "\n• {} [{}] {}:{}-{}\n",
            c.chunk_id, c.kind, c.path, c.line_start, c.line_end
        ));
        // The stored one-line summary (if `/summarize` has run) goes first — it's the
        // fastest way for the reader to grasp the chunk before the raw snippet.
        if let Some(summary) = &c.summary {
            out.push_str(&format!("    ⤷ {}\n", summary));
        }
        for line in c.snippet.lines() {
            out.push_str(&format!("    {}\n", line));
        }
    }
    if result.connectivity.is_empty() {
        out.push_str("\nConnectivity: (no edges link these chunks to each other)\n");
    } else {
        out.push_str("\nConnectivity (how these chunks are wired):\n");
        for e in &result.connectivity {
            out.push_str(&format!("  {} --{}--> {}\n", e.from, e.kind, e.to));
            // A stored per-edge relationship description (from `/relationships`).
            if let Some(summary) = &e.summary {
                out.push_str(&format!("      ⤷ {}\n", summary));
            }
        }
    }
    out.push_str(&render_referenced_by(&result.referenced_by));
    out
}

/// Render the impact surface — who references / implements / tests the retrieved
/// chunks (incoming edges from *outside* the set). Surfaced so a reader planning or
/// making a change can see what it would affect. Empty string when there are none.
fn render_referenced_by(edges: &[ConnEdge]) -> String {
    if edges.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\nReferenced by (impact surface — these depend on the above; review when changing it):\n",
    );
    for e in edges {
        out.push_str(&format!("  {} --{}--> {}\n", e.from, e.kind, e.to));
        if let Some(summary) = &e.summary {
            out.push_str(&format!("      ⤷ {}\n", summary));
        }
    }
    out
}

/// Render the `/ask` inference result: the synthesised answer (with its inline
/// `path:line` citations) followed by the chunks it drew on, so the reader can verify
/// them. When `answer` is `None` the loop exhausted its candidate sets without a
/// sufficient answer, so the best chunks are shown raw with a note.
pub fn render_ask_answer(question: &str, ans: &AskAnswer) -> String {
    if ans.chunks.is_empty() {
        return format!(
            "No chunks matched '{}'. Try /index or a different phrasing.",
            question
        );
    }
    let mut out = String::new();
    match &ans.answer {
        Some(answer) => {
            out.push_str(answer);
            out.push_str(&format!(
                "\n\nSources ({} chunk(s) over {} pass(es)):\n",
                ans.chunks.len(),
                ans.passes
            ));
        }
        None => {
            out.push_str(&format!(
                "Couldn't synthesise an answer from the indexed code after {} pass(es). Most relevant chunks:\n",
                ans.passes
            ));
        }
    }
    for c in &ans.chunks {
        out.push_str(&format!(
            "\n• {} [{}] {}:{}-{}\n",
            c.chunk_id, c.kind, c.path, c.line_start, c.line_end
        ));
        if let Some(summary) = &c.summary {
            out.push_str(&format!("    ⤷ {}\n", summary));
        }
    }
    out.push_str(&render_referenced_by(&ans.referenced_by));
    out
}

/// List the immediate entries of directory `dir`: subdirectories first (each with a
/// trailing `/`), then files, both alphabetically. Used by the `list_directory` tool
/// so the agent can discover project structure without shelling out to `ls`/`dir`.
/// Non-recursive; errors propagate if `dir` isn't a readable directory.
pub fn list_dir(dir: &Path) -> std::io::Result<Vec<String>> {
    let mut dirs: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            dirs.push(format!("{}/", name));
        } else {
            files.push(name);
        }
    }
    dirs.sort();
    files.sort();
    dirs.extend(files);
    Ok(dirs)
}

/// Number of unchanged context lines shown above and below a hunk.
const DIFF_CONTEXT: usize = 3;

// Background/foreground colours for the diff blocks. Dark-tinted backgrounds with a
// light foreground read as a solid red/green block while keeping the text legible —
// the Claude-Code look. Truecolor is fine on the target terminals (Windows Terminal).
const REM_BG: (u8, u8, u8) = (60, 22, 22);
const REM_FG: (u8, u8, u8) = (255, 200, 200);
const ADD_BG: (u8, u8, u8) = (20, 55, 25);
const ADD_FG: (u8, u8, u8) = (200, 255, 200);

/// The header bar naming the file the diff applies to.
fn diff_header(path: &str, suffix: &str) -> String {
    format!("{}\n", format!("  {}{} ", path, suffix).bold().reversed())
}

fn compact_diff_line(line: &str) -> String {
    let trimmed = line.trim_end_matches([' ', '\t']);
    format!("{}\n", trimmed)
}

/// `<num right-aligned to `w`> <sign> <text left-padded to `text_w`>`. The padding
/// makes the coloured backgrounds line up into a clean rectangle across the hunk.
fn block_line(num: usize, w: usize, sign: char, text: &str, text_w: usize) -> String {
    format!(
        "{:>w$} {} {:<text_w$}",
        num,
        sign,
        text,
        w = w,
        text_w = text_w
    )
}

/// An unchanged context line: number, a blank sign column (kept the same width as
/// `" - "`/`" + "` so the text aligns), then the verbatim line.
fn context_line(num: usize, w: usize, text: &str) -> String {
    format!("{:>w$}   {}", num, text, w = w)
}

/// A coloured, line-numbered diff for an in-place edit (`edit_file`/`edit_chunk`):
/// the file name, a few lines of dimmed context, the removed lines (red), and the
/// added lines (green). `content` is the file's current text; `old_text`/`new_text`
/// are the hunk being swapped. Removed lines carry the old file's numbering; added
/// and trailing-context lines carry the new file's numbering (what the file will
/// look like after the edit). For display only — tool results stay plain.
pub fn render_file_diff(path: &str, content: &str, old_text: &str, new_text: &str) -> String {
    let old_lines: Vec<&str> = old_text.lines().collect();
    let new_lines: Vec<&str> = new_text.lines().collect();

    // Locate the hunk so we can number lines and show surrounding context. Callers
    // already matched `old_text` uniquely; if it isn't found verbatim, fall back to a
    // context-free render rather than guessing positions.
    let idx = match content.find(old_text) {
        Some(i) => i,
        None => return render_hunk_no_context(path, &old_lines, &new_lines),
    };
    let start0 = content[..idx].matches('\n').count(); // 0-based line of the hunk
    let file_lines: Vec<&str> = content.lines().collect();

    // Models often pad `old_text`/`new_text` with identical anchor lines to make the
    // match unique. Trim the common leading/trailing lines so they render as context
    // rather than as redundant `-`/`+` pairs — only the lines that actually change
    // get a sign. `lead + trail <= min(len)`, so the slices below never overlap.
    let (lead, trail) = common_affixes(&old_lines, &new_lines);
    let removed = &old_lines[lead..old_lines.len() - trail];
    let added = &new_lines[lead..new_lines.len() - trail];

    let pre_start = start0.saturating_sub(DIFF_CONTEXT);
    let post_first = start0 + old_lines.len();
    let post: Vec<&str> = file_lines
        .iter()
        .skip(post_first)
        .take(DIFF_CONTEXT)
        .copied()
        .collect();

    let first = start0 + 1; // 1-based number of the hunk's first line
    let change_start = first + lead; // first line that actually differs
    let trail_new_start = change_start + added.len(); // trailing common lines, new numbering
    let new_post_first = first + new_lines.len();
    // Pad numbers to at least the width of "1000", growing for larger files.
    let max_num = (first + old_lines.len())
        .max(new_post_first + post.len())
        .max(1000);
    let w = max_num.to_string().len();
    let text_w = block_text_width(removed, added);

    let mut out = diff_header(path, "");
    for (i, line) in file_lines.iter().enumerate().take(start0).skip(pre_start) {
        out.push_str(&compact_diff_line(
            &context_line(i + 1, w, line).dimmed().to_string(),
        ));
    }
    for (j, line) in old_lines[..lead].iter().enumerate() {
        out.push_str(&compact_diff_line(
            &context_line(first + j, w, line).dimmed().to_string(),
        ));
    }
    for (i, line) in removed.iter().enumerate() {
        out.push_str(&compact_diff_line(
            &block_line(change_start + i, w, '-', line, text_w)
                .truecolor(REM_FG.0, REM_FG.1, REM_FG.2)
                .on_truecolor(REM_BG.0, REM_BG.1, REM_BG.2)
                .to_string(),
        ));
    }
    for (i, line) in added.iter().enumerate() {
        out.push_str(&compact_diff_line(
            &block_line(change_start + i, w, '+', line, text_w)
                .truecolor(ADD_FG.0, ADD_FG.1, ADD_FG.2)
                .on_truecolor(ADD_BG.0, ADD_BG.1, ADD_BG.2)
                .to_string(),
        ));
    }
    for (j, line) in old_lines[old_lines.len() - trail..].iter().enumerate() {
        out.push_str(&compact_diff_line(
            &context_line(trail_new_start + j, w, line)
                .dimmed()
                .to_string(),
        ));
    }
    for (i, line) in post.iter().enumerate() {
        out.push_str(&compact_diff_line(
            &context_line(new_post_first + i, w, line)
                .dimmed()
                .to_string(),
        ));
    }
    out
}

/// Count the identical leading and trailing lines shared by `old` and `new`. The two
/// counts never overlap (`lead + trail <= min(old.len(), new.len())`), so callers can
/// slice the differing middle out safely.
fn common_affixes(old: &[&str], new: &[&str]) -> (usize, usize) {
    let max = old.len().min(new.len());
    let mut lead = 0;
    while lead < max && old[lead] == new[lead] {
        lead += 1;
    }
    let mut trail = 0;
    while trail < max - lead && old[old.len() - 1 - trail] == new[new.len() - 1 - trail] {
        trail += 1;
    }
    (lead, trail)
}

/// The block text width: the longest changed line, so removed/added backgrounds form
/// an aligned rectangle.
fn block_text_width(old_lines: &[&str], new_lines: &[&str]) -> usize {
    old_lines
        .iter()
        .chain(new_lines.iter())
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0)
}

/// Fallback render when the hunk can't be located in the file (no line numbers or
/// context available) — just the coloured removed/added blocks.
fn render_hunk_no_context(path: &str, old_lines: &[&str], new_lines: &[&str]) -> String {
    let mut out = diff_header(path, "");
    for line in old_lines {
        out.push_str(&compact_diff_line(&format!(
            " - {}",
            line.trim_end_matches([' ', '\t'])
        )));
    }
    for line in new_lines {
        out.push_str(&compact_diff_line(&format!(
            " + {}",
            line.trim_end_matches([' ', '\t'])
        )));
    }
    out
}

/// A line-numbered "all added" diff for a newly created file, capped at `max` lines.
pub fn render_new_file(path: &str, content: &str, max: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    let w = total.max(1000).to_string().len();
    let text_w = lines
        .iter()
        .take(max)
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0);

    let mut out = diff_header(path, " (new file)");
    for (i, line) in lines.iter().enumerate() {
        if i >= max {
            out.push_str(&format!(
                "{}\n",
                format!("… (+{} more lines)", total - i).dimmed()
            ));
            break;
        }
        out.push_str(&compact_diff_line(
            &block_line(i + 1, w, '+', line, text_w)
                .truecolor(ADD_FG.0, ADD_FG.1, ADD_FG.2)
                .on_truecolor(ADD_BG.0, ADD_BG.1, ADD_BG.2)
                .to_string(),
        ));
    }
    out
}

pub fn render_graph_stats(stats: &GraphStats) -> String {
    let mut out = format!(
        "🕸️  Code graph: {} nodes, {} edges\n",
        stats.nodes_total, stats.edges_total
    );
    if !stats.node_labels.is_empty() {
        let nodes: Vec<String> = stats
            .node_labels
            .iter()
            .map(|(l, n)| format!("{} {}", n, l))
            .collect();
        out.push_str(&format!("  nodes: {}\n", nodes.join(", ")));
    }
    if !stats.edge_labels.is_empty() {
        let edges: Vec<String> = stats
            .edge_labels
            .iter()
            .map(|(l, n)| format!("{} {}", n, l))
            .collect();
        out.push_str(&format!("  edges: {}\n", edges.join(", ")));
    }
    if let Some(wal) = &stats.wal {
        out.push_str(&format!(
            "  storage: {} ({} delta), sequence {}, {} compressed away\n",
            human_bytes(wal.file_bytes),
            human_bytes(wal.bytes_since_checkpoint),
            wal.last_sequence,
            human_bytes(wal.compression_saved_bytes()),
        ));
    }
    out
}

fn human_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= GIB {
        format!("{:.1} GiB", bytes_f / GIB)
    } else if bytes_f >= MIB {
        format!("{:.1} MiB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.1} KiB", bytes_f / KIB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(content: &str) -> std::collections::HashMap<String, String> {
        parse_dotenv(content).into_iter().collect()
    }

    #[test]
    fn file_diff_numbers_context_and_hunk() {
        // Disable ANSI so we can assert on the plain text layout.
        colored::control::set_override(false);
        let content = "line1\nline2\nline3\nline4\nline5\n";
        let out = render_file_diff("foo.rs", content, "line3", "LINE3\nLINE3b");

        assert!(out.contains("foo.rs"), "header names the file:\n{out}");
        // Context above/below is shown with its line numbers (padded to 4 cols).
        assert!(out.contains("   1   line1"), "pre-context line 1:\n{out}");
        assert!(out.contains("   2   line2"), "pre-context line 2:\n{out}");
        // The removed line keeps the old numbering with a '-'.
        assert!(out.contains("   3 - line3"), "removed line:\n{out}");
        // Added lines are numbered in the resulting file with a '+'.
        assert!(out.contains("   3 + LINE3"), "first added line:\n{out}");
        assert!(out.contains("   4 + LINE3b"), "second added line:\n{out}");
        // Trailing context is renumbered to the post-edit file (line4 -> 5, line5 -> 6).
        assert!(
            out.contains("   5   line4"),
            "post-context renumbered:\n{out}"
        );
        assert!(
            out.contains("   6   line5"),
            "post-context renumbered:\n{out}"
        );
    }

    #[test]
    fn file_diff_trims_identical_anchor_lines_to_context() {
        // The model padded the edit with an unchanged leading line ("b") and trailing
        // line ("d"); only "c" -> "CHANGED" actually differs.
        colored::control::set_override(false);
        let content = "a\nb\nc\nd\ne\n";
        let out = render_file_diff("foo.rs", content, "b\nc\nd", "b\nCHANGED\nd");

        // Only the genuinely changed line carries a sign.
        assert!(out.contains("   3 - c"), "changed line removed:\n{out}");
        assert!(out.contains("   3 + CHANGED"), "changed line added:\n{out}");
        // The identical anchors render as context, never as a -/+ pair.
        assert!(
            out.contains("   2   b"),
            "leading anchor is context:\n{out}"
        );
        assert!(
            out.contains("   4   d"),
            "trailing anchor is context:\n{out}"
        );
        assert!(
            !out.contains("- b") && !out.contains("+ b"),
            "no sign on 'b':\n{out}"
        );
        assert!(
            !out.contains("- d") && !out.contains("+ d"),
            "no sign on 'd':\n{out}"
        );
    }

    #[test]
    fn new_file_diff_numbers_every_line_as_added() {
        colored::control::set_override(false);
        let out = render_new_file("new.rs", "a\nb\nc", 40);
        assert!(
            out.contains("new.rs") && out.contains("(new file)"),
            "header:\n{out}"
        );
        assert!(out.contains("   1 + a"), "line 1:\n{out}");
        assert!(out.contains("   2 + b"), "line 2:\n{out}");
        assert!(out.contains("   3 + c"), "line 3:\n{out}");
    }

    #[test]
    fn new_file_diff_caps_at_max_lines() {
        colored::control::set_override(false);
        let content = "1\n2\n3\n4\n5";
        let out = render_new_file("big.txt", content, 2);
        assert!(
            out.contains("   1 + 1") && out.contains("   2 + 2"),
            "first two lines:\n{out}"
        );
        assert!(
            !out.contains("   3 + 3"),
            "third line must be capped:\n{out}"
        );
        assert!(out.contains("(+3 more lines)"), "remainder note:\n{out}");
    }

    #[test]
    fn parses_basic_pairs_comments_and_blanks() {
        let m = map("# a comment\n\nOPENAI_API_KEY=sk-123\n  ANTHROPIC_API_KEY = abc \n");
        assert_eq!(m.get("OPENAI_API_KEY").map(String::as_str), Some("sk-123"));
        // Surrounding whitespace around key and value is trimmed.
        assert_eq!(m.get("ANTHROPIC_API_KEY").map(String::as_str), Some("abc"));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn honours_export_prefix() {
        let m = map("export OLLAMA_URL=http://localhost:11434\n");
        assert_eq!(
            m.get("OLLAMA_URL").map(String::as_str),
            Some("http://localhost:11434")
        );
    }

    #[test]
    fn strips_quotes_and_unescapes_double_quoted() {
        let m = map("A=\"hello\\nworld\"\nB='raw\\nliteral'\nC=\"with # hash\"\n");
        assert_eq!(m.get("A").map(String::as_str), Some("hello\nworld"));
        // Single quotes are literal — the backslash-n is preserved verbatim.
        assert_eq!(m.get("B").map(String::as_str), Some("raw\\nliteral"));
        // A quoted value keeps an embedded '#'.
        assert_eq!(m.get("C").map(String::as_str), Some("with # hash"));
    }

    #[test]
    fn unquoted_value_keeps_hash_verbatim() {
        // No inline-comment stripping for unquoted values, so secrets with '#' survive.
        let m = map("KEY=ab#cd\n");
        assert_eq!(m.get("KEY").map(String::as_str), Some("ab#cd"));
    }

    #[test]
    fn skips_malformed_and_invalid_keys() {
        let m = map("no_equals_here\n=novalue\n123BAD=x\nBAD-KEY=y\nGOOD_KEY=z\n");
        assert_eq!(m.get("GOOD_KEY").map(String::as_str), Some("z"));
        assert!(!m.contains_key("123BAD")); // keys may not start with a digit
        assert!(!m.contains_key("BAD-KEY")); // '-' is not allowed
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn load_dotenv_reads_ikenv_and_real_env_wins_over_root_files() {
        let dir = tempfile::tempdir().unwrap();
        // `.ikenv` is loaded just like `.env`. Use uniquely-named keys so this test
        // can mutate the process environment without colliding with anything else.
        std::fs::write(
            dir.path().join(".ikenv"),
            "IKODE_T_FROM_IKENV=loaded\nIKODE_T_OVERRIDE=from_file\n",
        )
        .unwrap();
        // A ROOT-level file must NOT override a real environment variable — root
        // `.env`/`.ikenv` files are often shared with other tooling.
        std::env::set_var("IKODE_T_OVERRIDE", "from_env");

        let loaded = load_dotenv(dir.path());
        assert!(loaded.contains(&".ikenv".to_string()));
        assert_eq!(std::env::var("IKODE_T_FROM_IKENV").as_deref(), Ok("loaded"));
        assert_eq!(std::env::var("IKODE_T_OVERRIDE").as_deref(), Ok("from_env"));

        std::env::remove_var("IKODE_T_FROM_IKENV");
        std::env::remove_var("IKODE_T_OVERRIDE");
    }

    #[test]
    fn load_dotenv_ikode_scoped_files_override_the_environment() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".ikode")).unwrap();
        // Files under .ikode/ are explicit project configuration: they WIN over
        // an inherited environment variable...
        std::fs::write(
            dir.path().join(".ikode").join(".ikenv"),
            "IKODE_T2_KEY=from_ikode_ikenv\n",
        )
        .unwrap();
        // ...but among files the earlier (higher-precedence) one still wins:
        // .ikode/.env is read before .ikode/.ikenv, and root files never clobber.
        std::fs::write(
            dir.path().join(".ikode").join(".env"),
            "IKODE_T2_FIRST=from_ikode_env\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".ikenv"),
            "IKODE_T2_FIRST=from_root\nIKODE_T2_KEY=from_root\n",
        )
        .unwrap();
        std::env::set_var("IKODE_T2_KEY", "from_env");

        let loaded = load_dotenv(dir.path());
        assert!(loaded.contains(&".ikode/.ikenv".to_string()));
        assert_eq!(
            std::env::var("IKODE_T2_KEY").as_deref(),
            Ok("from_ikode_ikenv"),
            ".ikode-scoped file must override the inherited environment"
        );
        assert_eq!(
            std::env::var("IKODE_T2_FIRST").as_deref(),
            Ok("from_ikode_env"),
            "first file to define a key wins over later files"
        );

        std::env::remove_var("IKODE_T2_KEY");
        std::env::remove_var("IKODE_T2_FIRST");
    }
}
