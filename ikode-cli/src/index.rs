use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::Result;
use walkdir::WalkDir;

use gaise_core::contracts::{
    EmbeddingTaskControl, GaiseContent, GaiseEmbeddingTask, GaiseEmbeddingsRequest,
    GaiseInstructRequest, OneOrMany,
};
use gaise_core::GaiseClient;
use livec_graph::livec_adapter::{
    EdgeAction, EdgeIdent, IndexAction, IndexScope, IndexTarget, IndexType, LabelRef, LabelSingle,
    NodeAction, NodeIdent, NodeRef, PropKey, Props, Value as GValue,
};
use livec_graph::{GraphService, GraphTxn, LValue, LivecGraph, NodeId, PropKeyId, WalStats};

use crate::ikignore::IkIgnore;
use crate::lang;
use crate::prompts;

mod inference;
mod retrieval;

/// Identifiers too generic to make useful REFERENCES edges (would link everything).
const REF_STOPWORDS: &[&str] = &[
    "new", "main", "run", "get", "set", "len", "from", "into", "default", "drop", "clone", "self",
    "fmt", "str", "val", "key", "add", "map", "vec", "the", "and", "for", "out", "err", "res",
    "next", "iter", "push", "name", "path", "args", "this", "data", "init", "with", "type",
];

const IGNORED_DIRS: &[&str] = &[
    ".git",
    ".ikode",
    "target",
    "node_modules",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".idea",
    ".vscode",
    "vendor",
    ".next",
    "out",
    "bin",
    "obj",
];

const MAX_INDEX_FILE_SIZE: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ChunkRecord {
    pub chunk_id: String,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub line_start: usize,
    pub line_end: usize,
    /// Body with the shared leading indentation (`indent`) stripped from each line.
    /// Use `indented_code()` to get the faithful on-disk text back.
    pub code: String,
    /// Common leading-whitespace prefix factored out of `code` (stored once rather
    /// than repeated per line). Empty for top-level / single-line chunks.
    pub indent: String,
    /// `chunkId` of the enclosing chunk (a class/impl/trait/module) for nested
    /// members; `None` for top-level chunks. Drives `DEFINES` containment.
    pub parent_id: Option<String>,
}

impl ChunkRecord {
    /// The chunk's body with its common indentation re-applied — the inverse of the
    /// dedent done at chunk time, reproducing the on-disk text.
    pub fn indented_code(&self) -> String {
        lang::reindent(&self.indent, &self.code)
    }
}

#[derive(Debug, Clone)]
pub struct FileIndex {
    pub path: String,
    pub language: String,
    pub content_hash: u64,
    pub chunks: Vec<ChunkRecord>,
}

pub struct IndexStats {
    pub files: usize,
    pub chunks: usize,
    pub skipped: usize,
    pub links: usize,
    /// Source paths excluded from the walk by `.ikignore` rules. An ignored
    /// directory counts once because its descendants are never traversed.
    pub ignored: usize,
    /// Project-relative `.ikignore` files discovered for this pass.
    pub ikignore_files: Vec<String>,
    /// Fatal `.ikignore` discovery/read failure; indexing is aborted when present.
    pub error: Option<String>,
    /// Files pruned this pass because they vanished from disk or became ignored.
    pub removed: usize,
}

/// Result of applying the currently discovered `.ikignore` files to an already
/// loaded in-memory/persisted graph without performing a full source re-index.
pub struct IkIgnoreReport {
    pub files: Vec<String>,
    pub removed: usize,
}

/// Outcome of a `/squash`: how much the WAL shrank and what the compacted log now
/// reconstructs. `before_bytes`/`after_bytes` are the on-disk `graph.log` sizes
/// either side of the rewrite.
pub struct SquashStats {
    pub nodes: usize,
    pub edges: usize,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub uncompressed_snapshot_bytes: u64,
    pub last_sequence: u64,
}

/// A purely-algorithmic (no-inference) comparison of the on-disk project against
/// the graph loaded from the WAL. Lets the app tell the user, at startup, that
/// the persisted graph has drifted from the working tree and offer to re-sync.
#[derive(Default)]
pub struct Discrepancy {
    /// Indexable files on disk with no File node in the graph.
    pub added: Vec<String>,
    /// Files whose current `content_hash` differs from the graph's.
    pub changed: Vec<String>,
    /// File nodes no longer present in the effective source walk because their
    /// path vanished from disk or became excluded by `.ikignore`.
    pub deleted: Vec<String>,
}

impl Discrepancy {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.changed.is_empty() && self.deleted.is_empty()
    }

    pub fn total(&self) -> usize {
        self.added.len() + self.changed.len() + self.deleted.len()
    }
}

/// Which dependency edge kinds an [`Indexer::impact_with`] traversal follows.
/// `Calls` = the precise call graph only; `References` = the looser mention graph
/// only; `All` = both (the default blast-radius view).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepEdges {
    All,
    Calls,
    References,
}

impl DepEdges {
    /// The edge labels this filter follows.
    fn labels(self) -> &'static [&'static str] {
        match self {
            DepEdges::All => &["CALLS", "REFERENCES"],
            DepEdges::Calls => &["CALLS"],
            DepEdges::References => &["REFERENCES"],
        }
    }

    /// Parse a tool/CLI string (`calls` / `references` / `all` / `mentions`).
    /// Anything unrecognised (and `None`) falls back to `All`.
    pub fn parse(s: Option<&str>) -> Self {
        match s.map(|x| x.trim().to_ascii_lowercase()).as_deref() {
            Some("calls") | Some("call") => DepEdges::Calls,
            Some("references") | Some("reference") | Some("mentions") | Some("mention") => {
                DepEdges::References
            }
            _ => DepEdges::All,
        }
    }
}

/// Where a [`GraphQuerySpec`] traversal begins. A `chunk_id` anchors an exact
/// node; `symbol` matches any node whose name or chunk_id equals it; `kind`
/// restricts the start set to one label. All fields optional — an empty start
/// (with a `where` filter) scans all nodes.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct QueryStart {
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub chunk_id: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
}

/// One hop of a [`GraphQuerySpec`] traversal: follow `edge`-labelled edges in
/// direction `dir` (`out`, default, or `in`) for `depth` hops (default 1). Each
/// step's reached set becomes the next step's frontier.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct QueryStep {
    pub edge: String,
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default)]
    pub depth: Option<usize>,
}

/// Post-traversal node filter for a [`GraphQuerySpec`]. Every set field must hold.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct QueryWhere {
    /// Keep only nodes with one of these labels (case-insensitive).
    #[serde(default)]
    pub kind: Vec<String>,
    /// Keep only nodes whose file/path starts with this prefix.
    #[serde(default)]
    pub path_prefix: Option<String>,
    /// Keep only nodes with this visibility (`pub`, `private`, …).
    #[serde(default)]
    pub visibility: Option<String>,
    /// Keep only nodes whose name equals or contains this.
    #[serde(default)]
    pub name: Option<String>,
}

/// A structured, deterministic query over the code graph: resolve a start node
/// set, walk labelled edges, filter, and project. Built to be deserialised from
/// the `graph_query` tool's JSON (and the `/query` REPL command). No inference.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct GraphQuerySpec {
    #[serde(default)]
    pub start: Option<QueryStart>,
    #[serde(default)]
    pub traverse: Vec<QueryStep>,
    #[serde(default, rename = "where")]
    pub filter: Option<QueryWhere>,
    #[serde(default)]
    pub select: Option<Vec<String>>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// One result row from [`Indexer::graph_query`] — a node projected to its common
/// fields. Files/directories carry their `path` in `chunk_id` and an empty
/// line range.
#[derive(Debug, Clone)]
pub struct QueryRow {
    pub chunk_id: String,
    pub kind: String,
    pub name: String,
    pub path: String,
    pub line_start: i64,
    pub line_end: i64,
}

/// A chunk reached by a graph traversal, with its distance from the start node.
#[derive(Debug, Clone)]
pub struct GraphHit {
    pub chunk_id: String,
    pub kind: String,
    pub name: String,
    pub path: String,
    pub line_start: i64,
    pub line_end: i64,
    pub depth: usize,
}

/// A chunk returned by `ask_codebase`, with a relevance rank and a short snippet.
#[derive(Debug, Clone)]
pub struct AskChunk {
    pub chunk_id: String,
    pub kind: String,
    pub name: String,
    pub path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub snippet: String,
    /// One-line natural-language summary, if `summarize_index` has been run and
    /// stored a `summary` prop on this chunk's node. `None` when un-summarised.
    pub summary: Option<String>,
}

/// Outcome of an enrichment pass (`embed_index` / `summarize_index`).
#[derive(Debug, Default, Clone)]
pub struct EnrichStats {
    /// Chunks (re)embedded this pass.
    pub embedded: usize,
    /// Chunks (re)summarised this pass.
    pub summarized: usize,
    /// Chunks skipped because their stored enrichment was already up to date.
    pub skipped: usize,
    /// Chunks excluded from model-backed work by the project's `.ikignore`.
    pub ignored: usize,
}

/// Policy for a chunk-summary pass: which chunks are worth an LLM call, and how
/// many calls to run concurrently. Filtering is purely algorithmic (path/name/size
/// — no inference), keeping with iKode's "do the work algorithmically first"
/// principle: tests and trivial boilerplate are *indexed and embedded* regardless,
/// they just don't each burn a summary call by default.
#[derive(Debug, Clone, Copy)]
pub struct SummarizeOptions {
    /// Summarise test chunks (see [`is_test_chunk`]). Off by default — a test's name
    /// is already its summary, so the LLM sentence rarely adds retrieval value.
    pub include_tests: bool,
    /// Skip *non-callable* chunks whose body has fewer than this many non-blank
    /// lines (serde arg structs, tiny type aliases). Functions and methods are
    /// always summarised regardless of size. `0` disables the size filter.
    pub min_body_lines: usize,
    /// Number of summary calls in flight at once (`0`/`1` = sequential).
    pub concurrency: usize,
}

impl Default for SummarizeOptions {
    fn default() -> Self {
        SummarizeOptions {
            include_tests: false,
            min_body_lines: 4,
            concurrency: 4,
        }
    }
}

impl SummarizeOptions {
    /// Summarise every chunk, one at a time — the historical behaviour, used by the
    /// [`Indexer::summarize_index`] convenience wrapper and the test-suite call sites.
    pub fn unfiltered() -> Self {
        SummarizeOptions {
            include_tests: true,
            min_body_lines: 0,
            concurrency: 1,
        }
    }
}

/// Per-item progress status reported by the summary pass, so callers can show both
/// freshly-summarised chunks and ones skipped because they were already current.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryProgress {
    /// The chunk was (re)summarised this pass.
    Summarised,
    /// The chunk was skipped because its stored summary is already current (its body
    /// hash matched). Policy exclusions (tests, trivial chunks) are *not* reported —
    /// they're filtered silently.
    Skipped,
}

/// Count the non-blank lines in a chunk body — the cheap size signal the summary
/// policy uses to skip trivial chunks.
fn non_blank_lines(s: &str) -> usize {
    s.lines().filter(|l| !l.trim().is_empty()).count()
}

/// Is this chunk a callable (a `Function` or `Method`)? Callables are exempt from
/// the trivial-size filter: a small, widely-reused helper is exactly the kind of
/// thing worth a one-line summary, whereas a 3-line serde arg struct is not.
fn is_callable_kind(kind: &str) -> bool {
    matches!(kind, "Function" | "Method")
}

/// One edge of the connectivity subgraph linking retrieved chunks.
#[derive(Debug, Clone)]
pub struct ConnEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
    /// One-line relationship description, if `summarize_relationships` has stored a
    /// `summary` prop on this edge. `None` when the edge is un-described.
    pub summary: Option<String>,
}

/// Result of `ask_codebase`: the relevant chunks plus how they are wired together.
#[derive(Debug, Default)]
pub struct AskResult {
    pub chunks: Vec<AskChunk>,
    pub connectivity: Vec<ConnEdge>,
    /// Incoming dependency edges from *outside* the retrieved set — who references /
    /// implements / tests these chunks. This is the impact surface a change here would
    /// touch, surfaced so the agent can plan (or make) a change with callers in view.
    pub referenced_by: Vec<ConnEdge>,
}

/// Result of the `/ask` inference loop ([`Indexer::ask_inferred`]): a synthesised
/// natural-language answer over a small, embedding-retrieved set of chunks, plus the
/// chunks it considered (so the answer's inline `path:line` citations can be checked)
/// and how they're wired. `answer` is `None` when the loop exhausted every candidate
/// set without the chunks ever being sufficient — the caller then shows the best
/// `chunks` raw ("best chunks + a note").
#[derive(Debug, Default)]
pub struct AskAnswer {
    pub answer: Option<String>,
    pub chunks: Vec<AskChunk>,
    pub connectivity: Vec<ConnEdge>,
    /// Incoming dependency edges from outside the considered set — who references /
    /// implements / tests the cited chunks (the impact surface for a change). See
    /// [`AskResult::referenced_by`].
    pub referenced_by: Vec<ConnEdge>,
    /// Inference passes actually issued (1 = answered from the first set).
    pub passes: usize,
}

#[derive(Debug, Default)]
pub struct GraphStats {
    pub nodes_total: usize,
    pub edges_total: usize,
    pub node_labels: Vec<(String, u32)>,
    pub edge_labels: Vec<(String, u32)>,
    pub wal: Option<WalStats>,
}

/// Heuristic: does this path/name belong to test code? Used to overlay `TESTS`
/// edges (a test chunk that references X is treated as testing X). Conservative —
/// false negatives just mean a missing TESTS edge, not a wrong one.
pub fn is_test_chunk(path: &str, name: &str) -> bool {
    let p = path.to_ascii_lowercase();
    let path_hit = p.contains("/tests/")
        || p.starts_with("tests/")
        || p.contains("/test/")
        || p.contains("_test.")
        || p.contains("test_")
        || p.contains(".test.")
        || p.contains(".spec.")
        || p.contains("_spec.");
    let n = name.to_ascii_lowercase();
    let name_hit =
        n.starts_with("test_") || n.starts_with("test") && n.len() > 4 && !n.starts_with("tester");
    path_hit || name_hit
}

/// Parse a Rust-style `impl` header line into `(trait_name, type_name)`.
/// Handles `impl Type`, `impl Trait for Type`, and generic forms like
/// `impl<T: Bound> Trait<T> for Type<T>`. Returns base (ungeneric, unqualified)
/// names so they resolve against the symbol table. `trait_name` is `None` for
/// inherent impls. Returns `None` if the header isn't an impl.
pub fn parse_impl_header(code: &str) -> Option<(Option<String>, String)> {
    let line = code.lines().next()?.trim_start();
    let rest = line.strip_prefix("impl")?;
    // Must be a word boundary after `impl` (so we don't match `implements`).
    if rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        return None;
    }
    // Drop a leading generic parameter list: `<T: Bound, U>`.
    let rest = skip_angles(rest.trim_start());
    let rest = rest.trim();
    // Cut at the body/where clause.
    let head = rest
        .split('{')
        .next()
        .unwrap_or(rest)
        .split(" where ")
        .next()
        .unwrap_or(rest)
        .trim();

    let (trait_name, type_part) = match head.split_once(" for ") {
        Some((t, ty)) => (Some(base_ident(t)), ty),
        None => (None, head),
    };
    let type_name = base_ident(type_part);
    if type_name.is_empty() {
        return None;
    }
    Some((trait_name.filter(|t| !t.is_empty()), type_name))
}

/// Parse base types from a class/interface/struct header across languages:
/// `extends`/`implements` (Java, TypeScript, Kotlin), `: Base, IFace` (C#, C++,
/// Swift), and `class Foo(Base):` (Python). Returns base identifiers; the caller
/// resolves each against the symbol table and decides IMPLEMENTS vs EXTENDS by the
/// resolved node's kind. Rust is handled by `parse_impl_header` instead.
pub fn parse_supertypes(lang: &str, code: &str) -> Vec<String> {
    if lang == "Rust" {
        return Vec::new();
    }
    let line = match code.lines().next() {
        Some(l) => l.trim(),
        None => return Vec::new(),
    };
    let head = line.split('{').next().unwrap_or(line);
    let mut out: Vec<String> = Vec::new();
    let push = |id: String, out: &mut Vec<String>| {
        if !id.is_empty() && id != "object" && !out.contains(&id) {
            out.push(id);
        }
    };

    if lang == "Python" {
        if let (Some(s), Some(e)) = (head.find('('), head.rfind(')')) {
            if e > s {
                for part in head[s + 1..e].split(',') {
                    let p = part.split('=').next().unwrap_or(part); // drop metaclass=…
                    push(base_ident(p), &mut out);
                }
            }
        }
        return out;
    }

    // Java / TypeScript / Kotlin: `extends A` and/or `implements B, C`.
    let mut matched = false;
    for kw in ["extends", "implements"] {
        if let Some(seg) = keyword_segment(head, kw) {
            matched = true;
            for part in seg.split(',') {
                push(base_ident(part), &mut out);
            }
        }
    }
    if matched {
        return out;
    }

    // C# / C++ / Swift / Kotlin: `: Base, IFace` (after stripping access specifiers).
    if let Some(pos) = head.find(':') {
        let after = head[pos + 1..].split(" where ").next().unwrap_or("");
        for part in after.split(',') {
            let cleaned = part
                .replace("public", " ")
                .replace("private", " ")
                .replace("protected", " ")
                .replace("virtual", " ");
            push(base_ident(&cleaned), &mut out);
        }
    }
    out
}

/// Return the substring after the word `kw` in `head`, up to the next
/// `extends`/`implements` keyword or end-of-string.
fn keyword_segment<'a>(head: &'a str, kw: &str) -> Option<&'a str> {
    let needle = format!(" {} ", kw);
    let i = head.find(&needle)?;
    let rest = &head[i + needle.len()..];
    let mut end = rest.len();
    for other in [" extends ", " implements "] {
        if let Some(j) = rest.find(other) {
            if j < end {
                end = j;
            }
        }
    }
    Some(&rest[..end])
}

/// Skip a balanced `<...>` at the start of `s`, returning the remainder.
fn skip_angles(s: &str) -> &str {
    if !s.starts_with('<') {
        return s;
    }
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return &s[i + 1..];
                }
            }
            _ => {}
        }
    }
    s
}

/// Reduce a type/trait reference to its base identifier: strip leading `&`/`mut`,
/// a path prefix (`a::b::Name` -> `Name`), and any generic arguments (`Vec<T>` -> `Vec`).
fn base_ident(s: &str) -> String {
    let s = s.trim().trim_start_matches('&').trim();
    let s = s.strip_prefix("mut ").unwrap_or(s).trim();
    let s = s.split('<').next().unwrap_or(s);
    let s = s.rsplit("::").next().unwrap_or(s);
    s.trim().to_string()
}

/// Parse a definition's generic type parameters from its signature line into
/// `(param_name, bounds)` pairs. Handles the angle-bracket group that follows the
/// name before the `(`/`{`/`=`: `<T: Bound + Other, U>` (Rust/Kotlin/Swift/Scala),
/// `<T extends Comparable>` (Java), and bare `<T, R>` (TypeScript/C#). Lifetimes
/// (`'a`) and non-identifier params are skipped. Best-effort, no inference.
pub fn parse_generics(code: &str) -> Vec<(String, Vec<String>)> {
    let head = match code.lines().next() {
        Some(l) => l,
        None => return Vec::new(),
    };
    // Only treat a `<` that appears before the body/params as a generic list.
    let stop = head.find(['(', '{', '=']).unwrap_or(head.len());
    let lt = match head[..stop].find('<') {
        Some(i) => i,
        None => return Vec::new(),
    };
    // Find the matching `>` for that `<`.
    let mut depth = 0i32;
    let mut end = None;
    for (i, c) in head[lt..].char_indices() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(lt + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let inner = match end {
        Some(e) => &head[lt + 1..e],
        None => return Vec::new(),
    };

    let mut out = Vec::new();
    for part in split_top_level(inner) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (name_part, bound_part) = if let Some((n, b)) = part.split_once(':') {
            (n, Some(b))
        } else if let Some(i) = part.find(" extends ") {
            (&part[..i], Some(&part[i + 9..]))
        } else {
            (part, None)
        };
        let name = base_ident(name_part);
        if name.is_empty() || !is_symbol_name(&name) {
            continue;
        }
        let bounds = bound_part
            .map(|b| {
                b.split(['+', '&', ','])
                    .map(base_ident)
                    .filter(|s| !s.is_empty() && is_symbol_name(s))
                    .collect()
            })
            .unwrap_or_default();
        out.push((name, bounds));
    }
    out
}

/// Split `s` on top-level commas, respecting nested `<...>` (so `Map<K, V>` stays
/// one element).
fn split_top_level(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '<' => {
                depth += 1;
                cur.push(c);
            }
            '>' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur);
    }
    parts
}

/// Identifiers that appear in *call position* in `code` — an identifier run
/// immediately followed by `(` (ignoring intervening spaces/tabs). Combined with
/// [`is_callable_kind`], these distinguish a true invocation (`CALLS`) from an
/// incidental mention (`REFERENCES`): a type in a signature, a field, a string, or
/// a same-named symbol used without calling it. Identifiers are ASCII runs, so
/// byte scanning is safe across UTF-8 (non-ASCII bytes act as separators).
fn called_identifiers(code: &str) -> HashSet<&str> {
    let b = code.as_bytes();
    let n = b.len();
    let is_id = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut out: HashSet<&str> = HashSet::new();
    let mut i = 0;
    while i < n {
        if !is_id(b[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && is_id(b[i]) {
            i += 1;
        }
        // `i` now sits just past the identifier run; peek the next non-space/tab.
        let mut j = i;
        while j < n && (b[j] == b' ' || b[j] == b'\t') {
            j += 1;
        }
        if j < n && b[j] == b'(' {
            out.insert(&code[start..i]);
        }
    }
    out
}

/// True if `s` looks like a code identifier (so markdown headings / prose are skipped).
pub fn is_symbol_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn node_str(g: &LivecGraph, nid: NodeId, key: PropKeyId) -> Option<String> {
    match g.get_node_prop(nid, key) {
        Some(LValue::Str(s)) => g.strings.resolve(*s).map(|x| x.to_string()),
        _ => None,
    }
}

fn node_i64(g: &LivecGraph, nid: NodeId, key: PropKeyId) -> i64 {
    match g.get_node_prop(nid, key) {
        Some(LValue::I64(v)) => *v,
        _ => 0,
    }
}

fn node_kind(g: &LivecGraph, nid: NodeId) -> String {
    g.nodes
        .get(nid as usize)
        .and_then(|n| n.labels.first())
        .and_then(|&l| g.label_names.resolve(l))
        .unwrap_or("Chunk")
        .to_string()
}

/// Max characters of a chunk body fed to embedding / summarisation, so one large
/// function can't dominate a request (and to bound token cost).
const ENRICH_BODY_CHARS: usize = 2000;

/// The text used to *summarise* a chunk: its identity plus a bounded slice of its
/// (re-indented) body. The summariser needs the raw code, so this stays code-first.
/// (The *embedding* text is composed differently — see [`embedding_document`].)
fn enrichment_text(rec: &ChunkRecord) -> String {
    let body = rec.indented_code();
    let body: String = body.chars().take(ENRICH_BODY_CHARS).collect();
    format!("{} {} in {}\n{}", rec.kind, rec.name, rec.path, body)
}

/// Body length (chars) at/under which a chunk's full source is inlined into its
/// embedding document; larger chunks rely on signature + comments + summary instead.
const EMBED_CONTENT_CHARS: usize = 1200;
/// Cap on the joined comment text folded into an embedding document.
const EMBED_COMMENTS_CHARS: usize = 600;
/// Cap on entity keywords / relationship lines in an embedding document.
const EMBED_MAX_RELATIONS: usize = 30;

/// Edge labels that count as semantic *relationships* for embedding/retrieval
/// (a chunk's keywords + relationship lines are drawn from these). Pure structural
/// edges (DEFINES, CONTAINS, HAS_TYPE_PARAM, HAS_IMPL) are deliberately excluded —
/// they describe containment, not what the chunk *uses*.
const EMBED_RELATION_EDGES: &[&str] = &[
    "CALLS",
    "REFERENCES",
    "IMPLEMENTS",
    "EXTENDS",
    "FOR_TYPE",
    "SATISFIES",
    "TESTS",
];
/// The subset of [`EMBED_RELATION_EDGES`] whose targets are treated as entity
/// keywords (things the chunk depends on / is), as opposed to incoming-style links.
const EMBED_KEYWORD_EDGES: &[&str] = &["CALLS", "REFERENCES", "IMPLEMENTS", "EXTENDS", "FOR_TYPE"];

/// Edge kinds that, *incoming* to a chunk, mean "something depends on this chunk":
/// callers (`CALLS`/`REFERENCES`), subtypes/impls (`IMPLEMENTS`/`SATISFIES`/`EXTENDS`/
/// `FOR_TYPE`), and tests (`TESTS`). Used to compute the `referenced_by` impact set.
const IMPACT_EDGE_KINDS: &[&str] = &[
    "CALLS",
    "REFERENCES",
    "IMPLEMENTS",
    "SATISFIES",
    "EXTENDS",
    "FOR_TYPE",
    "TESTS",
];
/// Cap on the `referenced_by` impact edges attached to an ask result, so a heavily
/// used helper can't flood the answer. `0` would mean unlimited.
const IMPACT_CAP: usize = 20;

/// `/ask` inference loop: chunks shown to the chat model per pass. The window widens
/// by this many chunks each time the model reports the current set is insufficient.
const ASK_BATCH: usize = 5;
/// `/ask` inference loop: max passes before giving up and returning the best chunks
/// raw. `ASK_BATCH * ASK_MAX_PASSES` is the hard ceiling on chunks ever shown.
const ASK_MAX_PASSES: usize = 3;

/// `/summarize`: persist accumulated summaries to the WAL every this-many chunks, so a
/// Ctrl+C mid-pass (which drops the in-flight future) keeps work already done rather
/// than discarding the whole pass. The remainder is flushed once the loop ends.
const SUMMARIZE_FLUSH: usize = 25;

/// `/embed`: number of chunk documents sent per embeddings request. Embedding the
/// whole out-of-date set in one request can blow past the provider's per-request
/// token cap (e.g. OpenAI's 300k), which fails wholesale and stores nothing — so a
/// large repo could never finish embedding. Small batches stay well under the cap,
/// persist incrementally (Ctrl+C-safe), and only ever carry chunks whose `embed_hash`
/// is already known to be stale.
const EMBED_BATCH: usize = 8;

/// Embedding task under which every indexed chunk document is embedded. Stored on
/// each node as `embed_task` so the staleness gate in `embed_index` can tell whether
/// a vector was produced under the current convention.
const EMBED_DOCUMENT_TASK: GaiseEmbeddingTask = GaiseEmbeddingTask::Document;

/// Embedding task for natural-language questions searched against the indexed
/// chunks. `CodeQuery` is the asymmetric counterpart of `Document` for code
/// retrieval: providers that distinguish the two sides (Gemini `taskType`, nomic /
/// EmbeddingGemma prefixes, Cohere `input_type`) embed the question into the same
/// space the documents landed in; providers without a task concept ignore it.
const EMBED_QUERY_TASK: GaiseEmbeddingTask = GaiseEmbeddingTask::CodeQuery;

/// Build an embeddings request for `model` (`provider::id`) tagged with `task`.
/// Dimensions and normalisation are left to the provider/registry defaults so the
/// vectors stay comparable with what `embed_index` stored.
fn embedding_request(
    model: &str,
    task: GaiseEmbeddingTask,
    input: OneOrMany<String>,
) -> GaiseEmbeddingsRequest {
    GaiseEmbeddingsRequest {
        model: model.to_string(),
        input,
        task: Some(task),
        ..Default::default()
    }
}

/// The request used to embed a search question against the index. Shared by
/// `ask_semantic` and the `/visualize` search box so both sides query the same
/// vector space the index was embedded into.
pub fn query_embedding_request(model: &str, text: &str) -> GaiseEmbeddingsRequest {
    embedding_request(model, EMBED_QUERY_TASK, OneOrMany::One(text.to_string()))
}

/// Whether `model`'s vectors depend on the embedding task (`embed_task`). Looks the
/// bare model id up in GAISe's bundled registry the same way the provider adapters
/// do: a profile with any task control other than `none` (Gemini `taskType`, nomic
/// prefixes, Cohere `input_type`, …) means document and query vectors differ, so
/// vectors stored under a different task are stale. Models the registry does not
/// know are treated as task-sensitive — re-embedding once is cheap and hash-gated,
/// whereas silently mixing vector spaces degrades every later search.
fn embedding_task_sensitive(model: &str) -> bool {
    let Some((provider, id)) = model.split_once("::") else {
        return true;
    };
    match gaise_core::registry::embedding_profile(provider, id) {
        Some(profile) => profile.task_control != EmbeddingTaskControl::None,
        None => true,
    }
}

/// Extract human-written comment text from a chunk body, preserving intent that
/// raw token-matching ignores. Purely line-oriented (no inference) and conservative:
/// only lines whose trimmed form *starts* with a comment marker are captured, so
/// string literals and URLs inside code never produce false positives. Handles
/// `//`/`///`/`//!` (C-family/Rust), `/* … */` blocks, `#` (Python/shell/Ruby — but
/// not Rust `#[attr]`/`#!`), and `--` (SQL/Lua/Haskell). Markers are stripped.
fn extract_comments(code: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut in_block = false;
    for raw in code.lines() {
        let line = raw.trim();
        if in_block {
            let content = line.trim_start_matches('*').trim_end_matches("*/").trim();
            if !content.is_empty() {
                out.push(content.to_string());
            }
            if line.contains("*/") {
                in_block = false;
            }
            continue;
        }
        if line.starts_with("/*") {
            let inner = line.trim_start_matches("/*").trim_end_matches("*/").trim();
            if !inner.is_empty() {
                out.push(inner.to_string());
            }
            in_block = !line.contains("*/");
        } else if let Some(rest) = line
            .strip_prefix("///")
            .or_else(|| line.strip_prefix("//!"))
            .or_else(|| line.strip_prefix("//"))
        {
            let t = rest.trim();
            if !t.is_empty() {
                out.push(t.to_string());
            }
        } else if !line.starts_with("#[") && !line.starts_with("#!") {
            if let Some(rest) = line.strip_prefix('#').or_else(|| line.strip_prefix("--")) {
                let t = rest.trim();
                if !t.is_empty() {
                    out.push(t.to_string());
                }
            }
        }
    }
    out
}

/// Compose the structured document fed to the embedder for one chunk. Per the
/// design, the vector is built from a small *document* — not raw code — so that
/// semantically similar-but-textually-different code still lands near a natural
/// language query, and so a chunk's summary, comments, entities and relationships
/// all steer its position in the vector space:
///
/// ```text
/// Context: <kind> `<name>` (<language>)[, within <parent kind> <parent name>]
/// File: <path>
/// Summary: <one-line LLM summary, when summarised>
/// Signature: <first non-empty source line — the structural shape>
/// Comments: <captured comment text — intent>
/// Keywords: <entity names the chunk uses / is>
/// Relationships: <EDGE target; …>
/// Content:
/// <full body — only when small>
/// ```
///
/// Every field is omitted when empty, so the document is stable and minimal.
/// Hashing this document yields the embedding's deterministic key: it changes when
/// the comments, signature, summary, entities or relationships change — which is
/// exactly when the embedding should be recomputed.
fn embedding_document(
    rec: &ChunkRecord,
    language: &str,
    parent: Option<(&str, &str)>,
    summary: Option<&str>,
    relations: &[(String, String)],
) -> String {
    let mut doc = String::new();
    let within = match parent {
        Some((pk, pn)) => format!(", within {} {}", pk, pn),
        None => String::new(),
    };
    doc.push_str(&format!(
        "Context: {} `{}` ({}){}\n",
        rec.kind, rec.name, language, within
    ));
    doc.push_str(&format!("File: {}\n", rec.path));

    if let Some(s) = summary.map(str::trim).filter(|s| !s.is_empty()) {
        doc.push_str(&format!("Summary: {}\n", s));
    }

    let body = rec.indented_code();
    if let Some(sig) = body.lines().map(str::trim).find(|l| !l.is_empty()) {
        doc.push_str(&format!("Signature: {}\n", sig));
    }

    let comments = extract_comments(&body);
    if !comments.is_empty() {
        let joined: String = comments
            .join(" ")
            .chars()
            .take(EMBED_COMMENTS_CHARS)
            .collect();
        doc.push_str(&format!("Comments: {}\n", joined.trim()));
    }

    let mut keywords: Vec<&str> = relations
        .iter()
        .filter(|(k, _)| EMBED_KEYWORD_EDGES.contains(&k.as_str()))
        .map(|(_, n)| n.as_str())
        .collect();
    keywords.sort_unstable();
    keywords.dedup();
    keywords.truncate(EMBED_MAX_RELATIONS);
    if !keywords.is_empty() {
        doc.push_str(&format!("Keywords: {}\n", keywords.join(", ")));
    }

    if !relations.is_empty() {
        let rels: Vec<String> = relations
            .iter()
            .take(EMBED_MAX_RELATIONS)
            .map(|(k, n)| format!("{} {}", k, n))
            .collect();
        doc.push_str(&format!("Relationships: {}\n", rels.join("; ")));
    }

    let trimmed = body.trim();
    if trimmed.chars().count() <= EMBED_CONTENT_CHARS {
        doc.push_str(&format!("Content:\n{}\n", trimmed));
    }
    doc
}

/// Extract the first text content out of a model message (used to read back an
/// `instruct` summary).
fn first_message_text(content: &Option<OneOrMany<GaiseContent>>) -> Option<String> {
    match content {
        Some(OneOrMany::One(GaiseContent::Text { text })) => Some(text.clone()),
        Some(OneOrMany::Many(parts)) => parts.iter().find_map(|c| match c {
            GaiseContent::Text { text } => Some(text.clone()),
            _ => None,
        }),
        _ => None,
    }
}

pub struct Indexer {
    root: PathBuf,
    graph: Option<GraphService>,
    files: HashMap<String, FileIndex>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

fn label(name: &str) -> LabelRef {
    LabelRef::Single(LabelSingle::Name(name.to_string()))
}

fn prop(key: &str, value: GValue) -> (PropKey, Option<GValue>) {
    (PropKey::Name(key.to_string()), Some(value))
}

/// Build a `Merge` edge between two chunk nodes addressed by their `chunkId`.
fn chunk_edge(
    from_kind: &str,
    from_id: &str,
    to_kind: &str,
    to_id: &str,
    edge: &str,
) -> EdgeAction {
    EdgeAction::Merge {
        ident: EdgeIdent::ByEndpoints {
            from: NodeRef::ByIndex {
                label: label(from_kind),
                key: PropKey::Name("chunk_id".to_string()),
                value: GValue::Str(from_id.to_string()),
            },
            to: NodeRef::ByIndex {
                label: label(to_kind),
                key: PropKey::Name("chunk_id".to_string()),
                value: GValue::Str(to_id.to_string()),
            },
            label: label(edge),
        },
        props: Props(vec![]),
    }
}

/// Like `chunk_edge`, but carries props (used to store a relationship `summary` +
/// `rel_hash` on an existing edge). Addressed by endpoints + label so it updates the
/// same edge `link_references` created.
fn chunk_edge_with_props(
    from_kind: &str,
    from_id: &str,
    to_kind: &str,
    to_id: &str,
    edge: &str,
    props: Vec<(PropKey, Option<GValue>)>,
) -> EdgeAction {
    EdgeAction::Merge {
        ident: EdgeIdent::ByEndpoints {
            from: NodeRef::ByIndex {
                label: label(from_kind),
                key: PropKey::Name("chunk_id".to_string()),
                value: GValue::Str(from_id.to_string()),
            },
            to: NodeRef::ByIndex {
                label: label(to_kind),
                key: PropKey::Name("chunk_id".to_string()),
                value: GValue::Str(to_id.to_string()),
            },
            label: label(edge),
        },
        props: Props(props),
    }
}

/// A `TypeParam` node (a generic parameter like `T`), addressed by a chunkId
/// derived from its owning definition so it prunes with that file.
fn type_param_node(tp_id: &str, name: &str, file_path: &str) -> NodeAction {
    NodeAction::Merge {
        ident: NodeIdent::ByIndex {
            label: label("TypeParam"),
            key: PropKey::Name("chunk_id".to_string()),
            value: GValue::Str(tp_id.to_string()),
        },
        label: label("TypeParam"),
        props: Props(vec![
            prop("chunk_id", GValue::Str(tp_id.to_string())),
            prop("name", GValue::Str(name.to_string())),
            prop("file_path", GValue::Str(file_path.to_string())),
        ]),
        alias: None,
    }
}

/// A `Directory` node addressed by its project-relative `path` (the root is `"."`).
/// Pure structural scaffolding — not embedded or summarised.
fn directory_node(path: &str, name: &str) -> NodeAction {
    NodeAction::Merge {
        ident: NodeIdent::ByIndex {
            label: label("Directory"),
            key: PropKey::Name("path".to_string()),
            value: GValue::Str(path.to_string()),
        },
        label: label("Directory"),
        props: Props(vec![
            prop("path", GValue::Str(path.to_string())),
            prop("name", GValue::Str(name.to_string())),
        ]),
        alias: None,
    }
}

/// A `CONTAINS` edge from a `Directory` (by `path`) to a child `Directory` or `File`
/// (by `path`), building the project tree.
fn contains_edge(parent_path: &str, child_label: &str, child_path: &str) -> EdgeAction {
    EdgeAction::Merge {
        ident: EdgeIdent::ByEndpoints {
            from: NodeRef::ByIndex {
                label: label("Directory"),
                key: PropKey::Name("path".to_string()),
                value: GValue::Str(parent_path.to_string()),
            },
            to: NodeRef::ByIndex {
                label: label(child_label),
                key: PropKey::Name("path".to_string()),
                value: GValue::Str(child_path.to_string()),
            },
            label: label("CONTAINS"),
        },
        props: Props(vec![]),
    }
}

const TRAIT_KINDS: &[&str] = &["Trait", "Interface"];
const TYPE_KINDS: &[&str] = &["Struct", "Enum", "Class", "Union", "TypeAlias"];
/// Definition kinds that can carry generic type parameters.
const GENERIC_KINDS: &[&str] = &[
    "Function",
    "Method",
    "Struct",
    "Class",
    "Enum",
    "Trait",
    "Interface",
    "ImplBlock",
    "TypeAlias",
    "Union",
];

impl Indexer {
    /// Build an in-memory-only indexer. Parallel read-only subagents use this so
    /// each worker can index/search independently without opening or writing the
    /// shared `.ikode/graph.log` WAL.
    pub fn new_ephemeral(root: PathBuf) -> Self {
        Self {
            root,
            graph: None,
            files: HashMap::new(),
        }
    }

    pub fn new(root: PathBuf) -> Self {
        let ikode_dir = root.join(".ikode");
        let _ = std::fs::create_dir_all(&ikode_dir);
        let wal_path = ikode_dir.join("graph.log");

        let started = std::time::Instant::now();
        let graph = match GraphService::new_with_log(&wal_path) {
            Ok(svc) => {
                let recovery = svc.recovery_report();
                eprintln!(
                    "ikode: WAL recovery took {}ms ({} txn(s), {} snapshot(s), {} repaired byte(s))",
                    started.elapsed().as_millis(),
                    recovery.recovered_transactions,
                    recovery.snapshots_loaded,
                    recovery.repaired_bytes,
                );
                if let Err(e) = Self::bootstrap_indexes(&svc) {
                    eprintln!("ikode: graph index bootstrap skipped: {e}");
                }
                Some(svc)
            }
            Err(e) => {
                eprintln!("ikode: graph persistence disabled ({e}); using in-memory index only");
                None
            }
        };

        Self {
            root,
            graph,
            files: HashMap::new(),
        }
    }

    fn bootstrap_indexes(graph: &GraphService) -> Result<()> {
        let txn = GraphTxn {
            idempotency_key: None,
            timestamp_ms: now_ms(),
            nodes: None,
            edges: None,
            indexes: Some(vec![
                IndexAction::CreateIndex {
                    scope: IndexScope::Global,
                    target: IndexTarget::Node(PropKey::Name("path".to_string())),
                    idx_type: IndexType::Unique,
                },
                IndexAction::CreateIndex {
                    scope: IndexScope::Global,
                    target: IndexTarget::Node(PropKey::Name("chunk_id".to_string())),
                    idx_type: IndexType::Unique,
                },
            ]),
        };
        graph.submit_txn(txn).map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(())
    }

    /// Destructively rebuild the graph: drop the in-memory state, delete the WAL,
    /// recreate the graph fresh, and re-index from scratch. Used by `/rebuild`.
    /// Loses any derived data not re-derivable from the source tree.
    pub fn rebuild(&mut self) -> IndexStats {
        let ikode_dir = self.root.join(".ikode");
        let wal_path = ikode_dir.join("graph.log");
        self.graph = None; // release the file handle before removing the WAL
        for path in [
            wal_path.clone(),
            ikode_dir.join("graph.log.next"),
            ikode_dir.join("graph.log.previous"),
            ikode_dir.join("graph.log.corrupt"),
        ] {
            let _ = std::fs::remove_file(path);
        }
        self.files.clear();
        let _ = std::fs::create_dir_all(&ikode_dir);
        self.graph = match GraphService::new_with_log(&wal_path) {
            Ok(svc) => {
                if let Err(e) = Self::bootstrap_indexes(&svc) {
                    eprintln!("ikode: graph index bootstrap skipped: {e}");
                }
                Some(svc)
            }
            Err(e) => {
                eprintln!("ikode: graph persistence disabled ({e}); using in-memory index only");
                None
            }
        };
        self.index_all()
    }

    /// Absolute path of the on-disk WAL (`.ikode/graph.log`).
    pub fn wal_path(&self) -> PathBuf {
        self.root.join(".ikode").join("graph.log")
    }

    /// Current framed WAL/snapshot file size (0 if persistence is unavailable).
    pub fn wal_size_bytes(&self) -> u64 {
        self.wal_stats()
            .map(|stats| stats.file_bytes)
            .unwrap_or_else(|| {
                std::fs::metadata(self.wal_path())
                    .map(|metadata| metadata.len())
                    .unwrap_or(0)
            })
    }

    /// Detailed byte counters, sequence position, and compression totals.
    pub fn wal_stats(&self) -> Option<WalStats> {
        self.graph.as_ref().map(GraphService::wal_stats)
    }

    /// Compact the WAL (`/squash`) into one compressed, checksummed snapshot of the
    /// exact graph state. Node IDs, internal indexes, idempotency keys, and free lists
    /// are preserved; historical transaction frames are discarded.
    pub fn squash(&mut self) -> Result<SquashStats> {
        let graph = self
            .graph
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("graph persistence is disabled; nothing to squash"))?;
        let mut node_count = 0_usize;
        let mut edge_count = 0_usize;
        graph.with_read(|live| {
            node_count = live.nodes.iter().filter(|node| node.alive).count();
            edge_count = live.edges.iter().filter(|edge| edge.alive).count();
        });
        let checkpoint = graph
            .checkpoint()
            .map_err(|error| anyhow::anyhow!("WAL checkpoint failed: {error}"))?;
        Ok(SquashStats {
            nodes: node_count,
            edges: edge_count,
            before_bytes: checkpoint.before_bytes,
            after_bytes: checkpoint.after_bytes,
            uncompressed_snapshot_bytes: checkpoint.uncompressed_snapshot_bytes,
            last_sequence: checkpoint.last_sequence,
        })
    }

    fn rel_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn discover_ikignore(&self) -> std::io::Result<IkIgnore> {
        IkIgnore::discover(&self.root, IGNORED_DIRS)
    }

    /// Every chunk node currently in the graph for file `rel`, as `(chunkId, label)`.
    /// Matched on the (non-indexed) `filePath` prop, so it includes nested members.
    /// Read-only; the labels are read back so a later delete addresses the right node.
    pub fn graph_chunks_for_file(&self, rel: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let graph = match &self.graph {
            Some(g) => g,
            None => return out,
        };
        graph.with_read(|g| {
            let cid_key = match g.prop_keys.get_id("chunk_id") {
                Some(k) => k,
                None => return,
            };
            let fp_key = match g.prop_keys.get_id("file_path") {
                Some(k) => k,
                None => return,
            };
            let want = match g.strings.get_id(rel) {
                Some(s) => s,
                None => return,
            };
            for (i, nr) in g.nodes.iter().enumerate() {
                if !nr.alive {
                    continue;
                }
                let nid = i as NodeId;
                let fp_match =
                    matches!(g.get_node_prop(nid, fp_key), Some(LValue::Str(s)) if *s == want);
                if !fp_match {
                    continue;
                }
                if let Some(LValue::Str(cid)) = g.get_node_prop(nid, cid_key) {
                    if let Some(cid_str) = g.strings.resolve(*cid) {
                        out.push((cid_str.to_string(), node_kind(g, nid)));
                    }
                }
            }
        });
        out
    }

    /// All `File`-node paths currently in the graph. Both `File` and `Directory`
    /// nodes carry a relative `path`, so this filters to the `File` label (chunks
    /// carry `file_path`, never `path`) — directories must not be mistaken for files
    /// by the deletion-pruning pass.
    pub fn graph_file_paths(&self) -> HashSet<String> {
        let mut out = HashSet::new();
        let graph = match &self.graph {
            Some(g) => g,
            None => return out,
        };
        graph.with_read(|g| {
            let path_key = match g.prop_keys.get_id("path") {
                Some(k) => k,
                None => return,
            };
            for (i, nr) in g.nodes.iter().enumerate() {
                if !nr.alive {
                    continue;
                }
                if node_kind(g, i as NodeId) != "File" {
                    continue;
                }
                if let Some(LValue::Str(s)) = g.get_node_prop(i as NodeId, path_key) {
                    if let Some(p) = g.strings.resolve(*s) {
                        out.insert(p.to_string());
                    }
                }
            }
        });
        out
    }

    /// All `Directory`-node paths currently in the graph (project-relative, root =
    /// `"."`). The directory-tree analogue of `graph_file_paths`.
    pub fn graph_dir_paths(&self) -> HashSet<String> {
        let mut out = HashSet::new();
        let graph = match &self.graph {
            Some(g) => g,
            None => return out,
        };
        graph.with_read(|g| {
            let path_key = match g.prop_keys.get_id("path") {
                Some(k) => k,
                None => return,
            };
            for (i, nr) in g.nodes.iter().enumerate() {
                if !nr.alive {
                    continue;
                }
                if node_kind(g, i as NodeId) != "Directory" {
                    continue;
                }
                if let Some(LValue::Str(s)) = g.get_node_prop(i as NodeId, path_key) {
                    if let Some(p) = g.strings.resolve(*s) {
                        out.insert(p.to_string());
                    }
                }
            }
        });
        out
    }

    /// The `contentHash` stored on the File node for `rel`, if present. Lets a
    /// re-index skip work (and stale-node pruning) when the file is byte-identical
    /// to what the graph already holds — even across sessions.
    fn graph_file_hash(&self, rel: &str) -> Option<String> {
        let graph = self.graph.as_ref()?;
        let mut hash = None;
        graph.with_read(|g| {
            let path_key = match g.prop_keys.get_id("path") {
                Some(k) => k,
                None => return,
            };
            let hash_key = match g.prop_keys.get_id("content_hash") {
                Some(k) => k,
                None => return,
            };
            let want = match g.strings.get_id(rel) {
                Some(s) => s,
                None => return,
            };
            for (i, nr) in g.nodes.iter().enumerate() {
                if !nr.alive {
                    continue;
                }
                let nid = i as NodeId;
                if matches!(g.get_node_prop(nid, path_key), Some(LValue::Str(s)) if *s == want) {
                    if let Some(LValue::Str(h)) = g.get_node_prop(nid, hash_key) {
                        hash = g.strings.resolve(*h).map(|x| x.to_string());
                    }
                    return;
                }
            }
        });
        hash
    }

    /// All File nodes as `path -> content_hash` (hex), read in a single graph
    /// pass. The bulk analogue of `graph_file_hash`, used by
    /// [`Self::project_discrepancies`] so the whole project can be diffed without
    /// re-scanning the graph once per file.
    fn file_hash_map(&self) -> HashMap<String, String> {
        let mut out = HashMap::new();
        let graph = match &self.graph {
            Some(g) => g,
            None => return out,
        };
        graph.with_read(|g| {
            let path_key = match g.prop_keys.get_id("path") {
                Some(k) => k,
                None => return,
            };
            let hash_key = match g.prop_keys.get_id("content_hash") {
                Some(k) => k,
                None => return,
            };
            for (i, nr) in g.nodes.iter().enumerate() {
                if !nr.alive {
                    continue;
                }
                let nid = i as NodeId;
                if node_kind(g, nid) != "File" {
                    continue;
                }
                let path = match g.get_node_prop(nid, path_key) {
                    Some(LValue::Str(s)) => g.strings.resolve(*s).map(|x| x.to_string()),
                    _ => None,
                };
                let hash = match g.get_node_prop(nid, hash_key) {
                    Some(LValue::Str(s)) => g.strings.resolve(*s).map(|x| x.to_string()),
                    _ => None,
                };
                if let (Some(p), Some(h)) = (path, hash) {
                    out.insert(p, h);
                }
            }
        });
        out
    }

    /// Diff the working tree against the loaded graph WITHOUT mutating either —
    /// classify indexable files as added / changed / deleted. Reuses the exact
    /// walk, built-in and `.ikignore` filters, size cap, language detection and hashing of
    /// [`Self::index_all`], so the result is precisely what a re-index would
    /// reconcile. Purely algorithmic (no inference), per the project's
    /// algorithmic-first design. Returns an empty `Discrepancy` when no graph is
    /// loaded (nothing to compare against).
    pub fn project_discrepancies(&self) -> Discrepancy {
        let mut d = Discrepancy::default();
        if self.graph.is_none() {
            return d;
        }
        let stored = self.file_hash_map();
        let ignore = match self.discover_ikignore() {
            Ok(ignore) => ignore,
            Err(error) => {
                eprintln!("ikode: .ikignore discovery failed ({error})");
                return d;
            }
        };

        let walker = WalkDir::new(&self.root).into_iter().filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            let built_in =
                e.depth() > 0 && e.file_type().is_dir() && IGNORED_DIRS.contains(&name.as_ref());
            !built_in && (e.depth() == 0 || !ignore.is_ignored(&e.path().to_string_lossy()))
        });
        let mut walked: HashSet<String> = HashSet::new();
        for entry in walker.filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if lang::detect_language(path).is_none() {
                continue;
            }
            if entry
                .metadata()
                .map(|m| m.len() > MAX_INDEX_FILE_SIZE)
                .unwrap_or(false)
            {
                continue;
            }
            let rel = self.rel_path(path);
            walked.insert(rel.clone());
            match stored.get(&rel) {
                None => d.added.push(rel),
                Some(stored_hex) => {
                    if let Ok(raw) = std::fs::read_to_string(path) {
                        let source = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
                        let hex = format!("{:x}", hash_bytes(source.as_bytes()));
                        if &hex != stored_hex {
                            d.changed.push(rel);
                        }
                    }
                }
            }
        }

        for path in self.graph_file_paths() {
            if !walked.contains(&path) {
                d.deleted.push(path);
            }
        }

        d.added.sort();
        d.changed.sort();
        d.deleted.sort();
        d
    }

    /// Delete chunk nodes by `(chunkId, label)`, cascading their incident edges.
    /// Victims must already exist (derive them from `graph_chunks_for_file`), since a
    /// delete of a missing node would abort the whole transaction.
    fn delete_chunk_nodes(&self, victims: &[(String, String)]) {
        if victims.is_empty() {
            return;
        }
        let graph = match &self.graph {
            Some(g) => g,
            None => return,
        };
        let nodes: Vec<NodeAction> = victims
            .iter()
            .map(|(cid, kind)| NodeAction::Delete {
                ident: NodeIdent::ByIndex {
                    label: label(kind),
                    key: PropKey::Name("chunk_id".to_string()),
                    value: GValue::Str(cid.clone()),
                },
            })
            .collect();
        let txn = GraphTxn {
            idempotency_key: None,
            timestamp_ms: now_ms(),
            nodes: Some(nodes),
            edges: None,
            indexes: None,
        };
        let _ = graph.submit_txn(txn);
    }

    /// Capture the *derived enrichment* props (summary + embedding, and their hashes)
    /// of every chunk node currently stored for file `rel`, keyed by chunk_id. A content
    /// change deletes and re-mirrors the file's chunk nodes; carrying these across the
    /// re-index lets unchanged chunks keep their summary/embedding so `/enrich` skips
    /// them (no wasted tokens). Only props that actually exist are captured.
    fn capture_enrichment(&self, rel: &str) -> HashMap<String, Vec<(PropKey, Option<GValue>)>> {
        let mut out: HashMap<String, Vec<(PropKey, Option<GValue>)>> = HashMap::new();
        let graph = match &self.graph {
            Some(g) => g,
            None => return out,
        };
        graph.with_read(|g| {
            let cid_key = match g.prop_keys.get_id("chunk_id") {
                Some(k) => k,
                None => return,
            };
            let fp_key = match g.prop_keys.get_id("file_path") {
                Some(k) => k,
                None => return,
            };
            let want = match g.strings.get_id(rel) {
                Some(s) => s,
                None => return,
            };
            let str_props = [
                "summary",
                "summary_hash",
                "embed_hash",
                "embed_model",
                "embed_task",
            ];
            let str_keys: Vec<(&str, _)> = str_props
                .iter()
                .map(|n| (*n, g.prop_keys.get_id(n)))
                .collect();
            let emb_key = g.prop_keys.get_id("embedding");
            for (i, nr) in g.nodes.iter().enumerate() {
                if !nr.alive {
                    continue;
                }
                let nid = i as NodeId;
                let fp_match =
                    matches!(g.get_node_prop(nid, fp_key), Some(LValue::Str(s)) if *s == want);
                if !fp_match {
                    continue;
                }
                let cid = match g.get_node_prop(nid, cid_key) {
                    Some(LValue::Str(s)) => match g.strings.resolve(*s) {
                        Some(c) => c.to_string(),
                        None => continue,
                    },
                    _ => continue,
                };
                let mut props: Vec<(PropKey, Option<GValue>)> = Vec::new();
                for (name, key_opt) in &str_keys {
                    if let Some(k) = *key_opt {
                        if let Some(LValue::Str(s)) = g.get_node_prop(nid, k) {
                            if let Some(v) = g.strings.resolve(*s) {
                                props.push(prop(name, GValue::Str(v.to_string())));
                            }
                        }
                    }
                }
                if let Some(k) = emb_key {
                    if let Some(LValue::ArrayFloat(v)) = g.get_node_prop(nid, k) {
                        props.push(prop("embedding", GValue::ArrayFloat(v.clone())));
                    }
                }
                if !props.is_empty() {
                    out.insert(cid, props);
                }
            }
        });
        out
    }

    /// Re-apply enrichment props captured by [`capture_enrichment`] to the chunks of the
    /// freshly re-mirrored `file` that still exist (same chunk_id). Merge upserts, so this
    /// only restores the carried props and leaves the structural ones intact. A chunk
    /// whose body actually changed keeps its *old* `summary_hash`/`embed_hash` here, which
    /// then no longer matches its new body — so `/enrich` correctly re-derives just it.
    fn reapply_enrichment(
        &self,
        file: &FileIndex,
        preserved: &HashMap<String, Vec<(PropKey, Option<GValue>)>>,
    ) {
        if preserved.is_empty() {
            return;
        }
        let graph = match &self.graph {
            Some(g) => g,
            None => return,
        };
        let mut nodes: Vec<NodeAction> = Vec::new();
        for ch in &file.chunks {
            if let Some(props) = preserved.get(&ch.chunk_id) {
                nodes.push(NodeAction::Merge {
                    ident: NodeIdent::ByIndex {
                        label: label(&ch.kind),
                        key: PropKey::Name("chunk_id".to_string()),
                        value: GValue::Str(ch.chunk_id.clone()),
                    },
                    label: label(&ch.kind),
                    props: Props(props.clone()),
                    alias: None,
                });
            }
        }
        if nodes.is_empty() {
            return;
        }
        let txn = GraphTxn {
            idempotency_key: None,
            timestamp_ms: now_ms(),
            nodes: Some(nodes),
            edges: None,
            indexes: None,
        };
        let _ = graph.submit_txn(txn);
    }

    /// Remove a file that no longer exists on disk: delete all its chunk nodes
    /// (cascading edges) and then the File node itself.
    fn prune_deleted_file(&self, rel: &str) {
        let victims = self.graph_chunks_for_file(rel);
        self.delete_chunk_nodes(&victims);
        let graph = match &self.graph {
            Some(g) => g,
            None => return,
        };
        let txn = GraphTxn {
            idempotency_key: None,
            timestamp_ms: now_ms(),
            nodes: Some(vec![NodeAction::Delete {
                ident: NodeIdent::ByIndex {
                    label: label("File"),
                    key: PropKey::Name("path".to_string()),
                    value: GValue::Str(rel.to_string()),
                },
            }]),
            edges: None,
            indexes: None,
        };
        let _ = graph.submit_txn(txn);
    }

    /// Remove `Directory` nodes no longer needed by any live indexed file. File
    /// deletion cascades its incident `CONTAINS` edge, but without this reconciliation
    /// an ignored/deleted subtree would leave empty directory shells in the graph.
    fn prune_stale_directories(&self, live_files: &HashSet<String>) {
        let graph = match &self.graph {
            Some(graph) => graph,
            None => return,
        };
        let mut live_dirs: HashSet<String> = HashSet::from([".".to_string()]);
        for file in live_files {
            let parts: Vec<&str> = file.split('/').filter(|part| !part.is_empty()).collect();
            let mut current = String::new();
            for part in parts.iter().take(parts.len().saturating_sub(1)) {
                if !current.is_empty() {
                    current.push('/');
                }
                current.push_str(part);
                live_dirs.insert(current.clone());
            }
        }

        let mut stale: Vec<String> = self
            .graph_dir_paths()
            .into_iter()
            .filter(|path| !live_dirs.contains(path))
            .collect();
        // Delete children first for a deterministic transaction and straightforward
        // incident-edge cascading.
        stale.sort_by(|a, b| {
            b.matches('/')
                .count()
                .cmp(&a.matches('/').count())
                .then_with(|| b.cmp(a))
        });
        if stale.is_empty() {
            return;
        }
        let nodes = stale
            .into_iter()
            .map(|path| NodeAction::Delete {
                ident: NodeIdent::ByIndex {
                    label: label("Directory"),
                    key: PropKey::Name("path".to_string()),
                    value: GValue::Str(path),
                },
            })
            .collect();
        let txn = GraphTxn {
            idempotency_key: None,
            timestamp_ms: now_ms(),
            nodes: Some(nodes),
            edges: None,
            indexes: None,
        };
        let _ = graph.submit_txn(txn);
    }

    /// Forget a file the caller has just deleted from disk: prune its `File` node and
    /// every chunk node (cascading incident edges) from the graph, and drop it from the
    /// in-memory index. The graph/index analogue of `delete_file`.
    pub fn remove_path(&mut self, path: &Path) {
        let rel = self.rel_path(path);
        self.prune_deleted_file(&rel);
        self.files.remove(&rel);
        let live = self.graph_file_paths();
        self.prune_stale_directories(&live);
    }

    /// Immediately evict paths newly covered by root or nested `.ikignore` rules.
    /// Used before model-backed passes so editing an ignore file mid-session cannot
    /// leave ignored nodes searchable until some later full index.
    pub fn prune_ikignored(&mut self) -> Result<IkIgnoreReport> {
        let ignore = self
            .discover_ikignore()
            .map_err(|error| anyhow::anyhow!(".ikignore discovery failed: {error}"))?;
        let mut candidates = self.graph_file_paths();
        candidates.extend(self.files.keys().cloned());
        let ignored: Vec<String> = candidates
            .into_iter()
            .filter(|path| ignore.is_ignored(path))
            .collect();
        for path in &ignored {
            self.prune_deleted_file(path);
            self.files.remove(path);
        }
        let live = self.graph_file_paths();
        self.prune_stale_directories(&live);
        Ok(IkIgnoreReport {
            files: ignore.files().to_vec(),
            removed: ignored.len(),
        })
    }

    pub fn index_all(&mut self) -> IndexStats {
        self.index_all_reporting(&mut |_| {})
    }

    /// As [`index_all`], but invokes `on_chunk` with each chunk's id as a file is
    /// (re)chunked, so callers can stream per-chunk progress to the console. Files
    /// whose contents are unchanged since last indexed are not re-chunked, so they
    /// emit nothing.
    pub fn index_all_reporting(&mut self, on_chunk: &mut dyn FnMut(&str)) -> IndexStats {
        let mut stats = IndexStats {
            files: 0,
            chunks: 0,
            skipped: 0,
            links: 0,
            ignored: 0,
            ikignore_files: Vec::new(),
            error: None,
            removed: 0,
        };

        let ignore = match self.discover_ikignore() {
            Ok(ignore) => ignore,
            Err(error) => {
                stats.error = Some(format!(".ikignore discovery failed: {error}"));
                return stats;
            }
        };
        stats.ikignore_files = ignore.files().to_vec();
        let ignored = std::cell::Cell::new(0_usize);
        let walker = WalkDir::new(&self.root).into_iter().filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            let built_in =
                e.depth() > 0 && e.file_type().is_dir() && IGNORED_DIRS.contains(&name.as_ref());
            if built_in {
                return false;
            }
            if e.depth() > 0 && ignore.is_ignored(&e.path().to_string_lossy()) {
                ignored.set(ignored.get() + 1);
                return false;
            }
            true
        });

        // Paths we (re)indexed this pass — the authoritative live set on disk.
        let mut walked: HashSet<String> = HashSet::new();
        for entry in walker.filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let spec = match lang::detect_language(path) {
                Some(s) => s,
                None => continue,
            };
            if entry
                .metadata()
                .map(|m| m.len() > MAX_INDEX_FILE_SIZE)
                .unwrap_or(false)
            {
                stats.skipped += 1;
                continue;
            }
            walked.insert(self.rel_path(path));
            match self.index_file_unchecked(path, &spec, on_chunk) {
                Ok(n) => {
                    stats.files += 1;
                    stats.chunks += n;
                }
                Err(_) => stats.skipped += 1,
            }
        }
        stats.ignored = ignored.get();

        // Prune files that disappeared from disk since they were last indexed:
        // delete the File node and all its chunk nodes (cascading edges).
        let deleted: Vec<String> = self
            .graph_file_paths()
            .into_iter()
            .filter(|p| !walked.contains(p))
            .collect();
        for path in &deleted {
            self.prune_deleted_file(path);
            self.files.remove(path);
        }
        stats.removed = deleted.len();
        self.prune_stale_directories(&walked);

        stats.links = self.link_references();
        stats
    }

    pub fn index_file(&mut self, path: &Path, spec: &lang::LangSpec) -> Result<usize> {
        self.index_file_with(path, spec, &mut |_| {})
    }

    /// As [`index_file`], but invokes `on_chunk` with each chunk's id as it is created.
    /// Only called when the file is actually (re)chunked — an unchanged file returns
    /// its cached count early and emits nothing.
    pub fn index_file_with(
        &mut self,
        path: &Path,
        spec: &lang::LangSpec,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<usize> {
        let ignore = self
            .discover_ikignore()
            .map_err(|error| anyhow::anyhow!(".ikignore discovery failed: {error}"))?;
        if ignore.is_ignored(&path.to_string_lossy()) {
            let rel = self.rel_path(path);
            self.prune_deleted_file(&rel);
            self.files.remove(&rel);
            let live = self.graph_file_paths();
            self.prune_stale_directories(&live);
            return Ok(0);
        }
        self.index_file_unchecked(path, spec, on_chunk)
    }

    fn index_file_unchecked(
        &mut self,
        path: &Path,
        spec: &lang::LangSpec,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<usize> {
        let raw = std::fs::read_to_string(path)?;
        let source = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
        let rel = self.rel_path(path);
        let content_hash = hash_bytes(source.as_bytes());

        if let Some(existing) = self.files.get(&rel) {
            if existing.content_hash == content_hash {
                return Ok(existing.chunks.len());
            }
        }

        let raw_chunks = lang::chunk_source(spec, source);
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut chunks = Vec::with_capacity(raw_chunks.len());
        // Map a chunk's start line -> its assigned chunkId. A parent is always
        // emitted before its children (the chunker pushes the container first), so
        // this is populated by the time a child looks it up.
        let mut line_to_id: HashMap<usize, String> = HashMap::new();

        for ch in raw_chunks {
            let base_id = format!("{}::{}", rel, ch.name);
            let chunk_id = match seen.get_mut(&base_id) {
                Some(count) => {
                    *count += 1;
                    format!("{}#{}", base_id, *count)
                }
                None => {
                    seen.insert(base_id.clone(), 0);
                    base_id
                }
            };
            line_to_id.insert(ch.line_start, chunk_id.clone());
            let parent_id = ch.parent_line.and_then(|l| line_to_id.get(&l).cloned());
            on_chunk(&chunk_id);
            chunks.push(ChunkRecord {
                chunk_id,
                name: ch.name,
                kind: ch.kind,
                path: rel.clone(),
                line_start: ch.line_start,
                line_end: ch.line_end,
                code: ch.code,
                indent: ch.indent,
                parent_id,
            });
        }

        let count = chunks.len();
        let file_index = FileIndex {
            path: rel.clone(),
            language: spec.name.to_string(),
            content_hash,
            chunks,
        };

        // Reconcile the graph only when the file differs from what's already stored
        // there (compared via the File node's contentHash, so this also holds across
        // sessions). When it differs, delete the file's prior chunk nodes — cascading
        // their incident edges — before re-mirroring, so renamed/removed symbols don't
        // linger. The global `link_references` then rebuilds relationship edges.
        let hex = format!("{:x}", content_hash);
        if self.graph_file_hash(&rel).as_deref() != Some(hex.as_str()) {
            // Carry each chunk's derived enrichment across the delete+re-mirror so an
            // unchanged chunk keeps its summary/embedding (and thus is skipped by the
            // hash-gated enrich passes); only chunks whose body actually changed re-derive.
            let preserved = self.capture_enrichment(&rel);
            let stale = self.graph_chunks_for_file(&rel);
            self.delete_chunk_nodes(&stale);
            self.mirror_to_graph(&file_index);
            self.reapply_enrichment(&file_index, &preserved);
        }
        self.files.insert(rel, file_index);
        Ok(count)
    }

    /// Append the `Directory` nodes and `CONTAINS` edges for `file_path`'s ancestor
    /// chain (relative to the project root, which is the `"."` directory). Idempotent
    /// via `Merge`, so re-mirroring or sharing directories across files is safe.
    fn mirror_file_tree(file_path: &str, nodes: &mut Vec<NodeAction>, edges: &mut Vec<EdgeAction>) {
        const ROOT: &str = ".";
        nodes.push(directory_node(ROOT, ROOT));
        let parts: Vec<&str> = file_path.split('/').filter(|s| !s.is_empty()).collect();
        let mut parent = ROOT.to_string();
        for (i, comp) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                // The final component is the file itself.
                edges.push(contains_edge(&parent, "File", file_path));
            } else {
                let dpath = if parent == ROOT {
                    comp.to_string()
                } else {
                    format!("{}/{}", parent, comp)
                };
                nodes.push(directory_node(&dpath, comp));
                edges.push(contains_edge(&parent, "Directory", &dpath));
                parent = dpath;
            }
        }
    }

    fn mirror_to_graph(&self, file: &FileIndex) {
        let graph = match &self.graph {
            Some(g) => g,
            None => return,
        };

        let mut nodes: Vec<NodeAction> = Vec::with_capacity(file.chunks.len() + 1);
        let mut edges: Vec<EdgeAction> = Vec::with_capacity(file.chunks.len());

        // chunkId -> kind, so a parent `DEFINES` edge can address the parent node by
        // its correct label.
        let kind_of: HashMap<&str, &str> = file
            .chunks
            .iter()
            .map(|c| (c.chunk_id.as_str(), c.kind.as_str()))
            .collect();

        nodes.push(NodeAction::Merge {
            ident: livec_graph::livec_adapter::NodeIdent::ByIndex {
                label: label("File"),
                key: PropKey::Name("path".to_string()),
                value: GValue::Str(file.path.clone()),
            },
            label: label("File"),
            props: Props(vec![
                prop("path", GValue::Str(file.path.clone())),
                prop("language", GValue::Str(file.language.clone())),
                prop(
                    "content_hash",
                    GValue::Str(format!("{:x}", file.content_hash)),
                ),
            ]),
            alias: None,
        });

        // Mirror the project tree: a `Directory` node per ancestor (relative path,
        // root = "."), wired File/dir into its parent with `CONTAINS`.
        Self::mirror_file_tree(&file.path, &mut nodes, &mut edges);

        for ch in &file.chunks {
            // Infer visibility/modifiers from the signature line (no inference).
            let sig = ch.code.lines().next().unwrap_or("");
            let mods = lang::parse_modifiers(&file.language, &ch.name, sig);
            let mut props = vec![
                prop("chunk_id", GValue::Str(ch.chunk_id.clone())),
                prop("name", GValue::Str(ch.name.clone())),
                // NB: "file_path" (not "path") — "path" carries a *unique* index for
                // File nodes; many chunks share one file, so they must not collide on it.
                prop("file_path", GValue::Str(ch.path.clone())),
                prop("line_start", GValue::I64(ch.line_start as i64)),
                prop("line_end", GValue::I64(ch.line_end as i64)),
                prop("visibility", GValue::Str(mods.visibility.to_string())),
            ];
            // Only emit boolean modifier props when true, to keep nodes lean.
            for (key, on) in [
                ("is_static", mods.is_static),
                ("is_const", mods.is_const),
                ("is_async", mods.is_async),
                ("is_abstract", mods.is_abstract),
                ("is_virtual", mods.is_virtual),
                ("is_final", mods.is_final),
            ] {
                if on {
                    props.push(prop(key, GValue::Bool(true)));
                }
            }
            nodes.push(NodeAction::Merge {
                ident: livec_graph::livec_adapter::NodeIdent::ByIndex {
                    label: label(&ch.kind),
                    key: PropKey::Name("chunk_id".to_string()),
                    value: GValue::Str(ch.chunk_id.clone()),
                },
                label: label(&ch.kind),
                props: Props(props),
                alias: None,
            });

            // Containment: a nested member is DEFINES'd by its parent chunk; a
            // top-level chunk is DEFINES'd by the File.
            match ch
                .parent_id
                .as_deref()
                .and_then(|pid| kind_of.get(pid).map(|k| (pid, *k)))
            {
                Some((parent_id, parent_kind)) => {
                    edges.push(chunk_edge(
                        parent_kind,
                        parent_id,
                        &ch.kind,
                        &ch.chunk_id,
                        "DEFINES",
                    ));
                }
                None => {
                    edges.push(EdgeAction::Merge {
                        ident: livec_graph::livec_adapter::EdgeIdent::ByEndpoints {
                            from: NodeRef::ByIndex {
                                label: label("File"),
                                key: PropKey::Name("path".to_string()),
                                value: GValue::Str(file.path.clone()),
                            },
                            to: NodeRef::ByIndex {
                                label: label(&ch.kind),
                                key: PropKey::Name("chunk_id".to_string()),
                                value: GValue::Str(ch.chunk_id.clone()),
                            },
                            label: label("DEFINES"),
                        },
                        props: Props(vec![]),
                    });
                }
            }
        }

        let txn = GraphTxn {
            idempotency_key: None,
            timestamp_ms: now_ms(),
            nodes: Some(nodes),
            edges: Some(edges),
            indexes: None,
        };
        let _ = graph.submit_txn(txn);
    }

    /// Read-only view of the indexed files (`path -> FileIndex`), so callers can walk
    /// the in-memory chunk records directly — e.g. the `/visualize` tree builder, which
    /// reconstructs the dirs → files → symbols containment hierarchy from these.
    pub fn files(&self) -> &HashMap<String, FileIndex> {
        &self.files
    }

    /// `chunk_id -> one-line summary` for every chunk node that has a stored `summary`.
    /// Thin public wrapper over the internal prop map, used by the `/visualize` server
    /// to label tree nodes and code boxes.
    pub fn summaries(&self) -> HashMap<String, String> {
        self.chunk_prop_map("summary")
    }

    /// `chunk_id -> embedding vector` for every chunk node carrying an `embedding`
    /// prop. Snapshotting these once lets the `/visualize` server rank by cosine for
    /// semantic search without re-reading the graph on each query. Empty when nothing
    /// has been embedded (or graph persistence is disabled).
    pub fn embeddings_map(&self) -> HashMap<String, Vec<f32>> {
        let mut out = HashMap::new();
        let graph = match &self.graph {
            Some(g) => g,
            None => return out,
        };
        graph.with_read(|g| {
            let cid_key = match g.prop_keys.get_id("chunk_id") {
                Some(k) => k,
                None => return,
            };
            let emb_key = match g.prop_keys.get_id("embedding") {
                Some(k) => k,
                None => return,
            };
            for (i, nr) in g.nodes.iter().enumerate() {
                if !nr.alive {
                    continue;
                }
                let nid = i as NodeId;
                let emb = match g.get_node_prop(nid, emb_key) {
                    Some(LValue::ArrayFloat(v)) => v.clone(),
                    _ => continue,
                };
                if let Some(LValue::Str(s)) = g.get_node_prop(nid, cid_key) {
                    if let Some(cid) = g.strings.resolve(*s) {
                        out.insert(cid.to_string(), emb);
                    }
                }
            }
        });
        out
    }

    /// Every *semantic relationship* edge as `(src_chunk_id, dst_chunk_id, label)`,
    /// for the `/visualize` 3D graph layout. Restricted to [`EMBED_RELATION_EDGES`]
    /// (REFERENCES / IMPLEMENTS / EXTENDS / FOR_TYPE / SATISFIES / TESTS) — the
    /// meaningful "connections" that should pull symbols together — and skips pure
    /// structural containment (DEFINES / CONTAINS). Endpoints without a `chunk_id`
    /// (e.g. `Directory`/`File`/`TypeParam` nodes) are dropped.
    pub fn relationship_edges(&self) -> Vec<(String, String, String)> {
        let mut out = Vec::new();
        let graph = match &self.graph {
            Some(g) => g,
            None => return out,
        };
        graph.with_read(|g| {
            let cid_key = match g.prop_keys.get_id("chunk_id") {
                Some(k) => k,
                None => return,
            };
            // Map the relationship label ids we care about to their names once.
            let wanted: HashMap<_, &str> = EMBED_RELATION_EDGES
                .iter()
                .filter_map(|&name| g.label_names.get_id(name).map(|id| (id, name)))
                .collect();
            if wanted.is_empty() {
                return;
            }
            for e in g.alive_edges() {
                let label = match wanted.get(&e.type_id) {
                    Some(name) => *name,
                    None => continue,
                };
                let src = node_str(g, e.src, cid_key);
                let dst = node_str(g, e.dst, cid_key);
                if let (Some(s), Some(d)) = (src, dst) {
                    out.push((s, d, label.to_string()));
                }
            }
        });
        out
    }

    pub fn graph_stats(&self) -> Option<GraphStats> {
        let graph = self.graph.as_ref()?;
        let mut stats = GraphStats {
            wal: Some(graph.wal_stats()),
            ..GraphStats::default()
        };
        graph.with_read(|g| {
            stats.nodes_total = g.alive_nodes().count();
            stats.edges_total = g.alive_edges().count();
            for (lid, c) in g.count_labels() {
                if let Some(name) = g.label_names.resolve(lid) {
                    stats.node_labels.push((name.to_string(), c));
                }
            }
            for (lid, c) in g.count_edges() {
                if let Some(name) = g.label_names.resolve(lid) {
                    stats.edge_labels.push((name.to_string(), c));
                }
            }
        });
        stats
            .node_labels
            .sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        stats
            .edge_labels
            .sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        Some(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(name: &str, kind: &str, code: &str) -> ChunkRecord {
        ChunkRecord {
            chunk_id: format!("src/lib.rs::{name}"),
            name: name.to_string(),
            kind: kind.to_string(),
            path: "src/lib.rs".to_string(),
            line_start: 1,
            line_end: 5,
            code: code.to_string(),
            indent: String::new(),
            parent_id: None,
        }
    }

    /// A fully-populated chunk produces every document field in the documented order,
    /// drawing Keywords from the keyword edges and Relationships from all relations.
    #[test]
    fn embedding_document_has_structured_fields() {
        let rec = chunk(
            "parse_config",
            "Function",
            "fn parse_config(s: &str) -> Config {\n    // read the file\n    Config::from(s)\n}",
        );
        let relations = vec![
            ("REFERENCES".to_string(), "Config".to_string()),
            ("TESTS".to_string(), "loader".to_string()),
        ];
        let doc = embedding_document(
            &rec,
            "Rust",
            Some(("Struct", "Loader")),
            Some("Parses a config string into a Config."),
            &relations,
        );

        assert!(doc.contains("Context: Function `parse_config` (Rust), within Struct Loader"));
        assert!(doc.contains("File: src/lib.rs"));
        assert!(doc.contains("Summary: Parses a config string into a Config."));
        assert!(doc.contains("Signature: fn parse_config(s: &str) -> Config {"));
        assert!(doc.contains("Comments: read the file"));
        // Keywords come only from EMBED_KEYWORD_EDGES (REFERENCES), not TESTS.
        assert!(doc.contains("Keywords: Config"));
        assert!(!doc
            .lines()
            .any(|l| l.starts_with("Keywords:") && l.contains("loader")));
        // Relationships include every relation edge.
        assert!(doc.contains("Relationships: REFERENCES Config; TESTS loader"));
        // Small body is inlined.
        assert!(doc.contains("Content:\nfn parse_config"));
    }

    /// A body over the inline threshold omits the Content field but keeps the
    /// structural signature + comments so the chunk is still represented.
    #[test]
    fn embedding_document_omits_content_when_large() {
        let big = format!(
            "fn big() {{\n    // intent here\n{}\n}}",
            "    let _x = 1;\n".repeat(200)
        );
        assert!(big.chars().count() > EMBED_CONTENT_CHARS);
        let rec = chunk("big", "Function", &big);
        let doc = embedding_document(&rec, "Rust", None, None, &[]);
        assert!(!doc.contains("Content:"));
        assert!(doc.contains("Signature: fn big() {"));
        assert!(doc.contains("Comments: intent here"));
    }

    /// Empty optional inputs are simply skipped — no blank Summary/Keywords lines.
    #[test]
    fn embedding_document_skips_empty_fields() {
        let rec = chunk("noop", "Function", "fn noop() {}");
        let doc = embedding_document(&rec, "Rust", None, Some("   "), &[]);
        assert!(!doc.contains("Summary:"));
        assert!(!doc.contains("Keywords:"));
        assert!(!doc.contains("Relationships:"));
        assert!(!doc.contains("Comments:"));
        assert!(doc.contains("Context: Function `noop` (Rust)\n"));
    }

    /// Comment extraction captures only leading-marker comment lines (and block
    /// comments) — never a `//`-like sequence sitting inside a string literal, and
    /// never a Rust `#[attr]`.
    #[test]
    fn extract_comments_captures_markers_only() {
        let code = "\
#[derive(Debug)]
fn f() {
    /// doc line
    // line comment
    let url = \"https://example.com\"; // trailing — not captured (mid-line)
    # python-style
    /* block start
       block middle */
    -- sql style
}";
        let got = extract_comments(code);
        assert!(got.contains(&"doc line".to_string()));
        assert!(got.contains(&"line comment".to_string()));
        assert!(got.contains(&"python-style".to_string()));
        assert!(got.contains(&"block start".to_string()));
        assert!(got.contains(&"block middle".to_string()));
        assert!(got.contains(&"sql style".to_string()));
        // The attribute and the in-string URL must not be captured.
        assert!(!got.iter().any(|c| c.contains("derive")));
        assert!(!got.iter().any(|c| c.contains("example.com")));
    }

    /// The deterministic key (hash of the document) is sensitive to comments,
    /// summary and relationships — changing any re-triggers an embed; an unrelated
    /// repeat is stable.
    #[test]
    fn embedding_key_tracks_comments_summary_relations() {
        let with_comment = chunk("f", "Function", "fn f() {\n    // alpha\n    g()\n}");
        let with_other = chunk("f", "Function", "fn f() {\n    // beta\n    g()\n}");
        let rels = vec![("REFERENCES".to_string(), "g".to_string())];

        let key = |rec: &ChunkRecord, sum: Option<&str>, r: &[(String, String)]| {
            format!(
                "{:x}",
                hash_bytes(embedding_document(rec, "Rust", None, sum, r).as_bytes())
            )
        };

        let base = key(&with_comment, None, &[]);
        // Stable across identical recomputation.
        assert_eq!(base, key(&with_comment, None, &[]));
        // A changed comment changes the key.
        assert_ne!(base, key(&with_other, None, &[]));
        // Adding a summary changes the key.
        assert_ne!(base, key(&with_comment, Some("does a thing"), &[]));
        // Adding a relationship changes the key.
        assert_ne!(base, key(&with_comment, None, &rels));
    }

    /// Only identifiers in call position (`name(`, ignoring spaces/tabs) are
    /// reported — these become CALLS edges; a bare mention does not.
    #[test]
    fn called_identifiers_finds_call_position_only() {
        let code = "fn run() {\n    let cfg: Config = load();\n    parse (cfg);\n    helper.method();\n    let _ = Config;\n}";
        let called = called_identifiers(code);
        // Invoked names — `parse` with an intervening space still counts.
        assert!(called.contains("load"));
        assert!(called.contains("parse"));
        assert!(called.contains("method"));
        // `Config` is only ever a type annotation / value mention, never called.
        assert!(!called.contains("Config"));
    }

    /// A non-ASCII char between an identifier and `(` separates them, so byte
    /// scanning never mis-slices UTF-8 or invents a call across a unicode boundary.
    #[test]
    fn called_identifiers_handles_unicode_separators() {
        let called = called_identifiers("α = compute(); λ(x)");
        assert!(called.contains("compute"));
        // `λ` is non-ASCII so it isn't an ASCII identifier run; nothing spurious.
        assert!(!called.contains("λ"));
        assert_eq!(called.len(), 1);
    }
}
