//! `/visualize` — a small local web app over the indexed codebase.
//!
//! Starts an [`axum`] server (riding the tokio runtime already in use) that serves a
//! single static page ([`index.html`], embedded at compile time — no npm/build step)
//! plus JSON APIs over a *snapshot* of the index:
//!
//!   - `GET /` — the page
//!   - `GET /api/tree` — dirs → files → symbols containment tree (the left-to-right
//!     collapsible org chart, rendered with D3)
//!   - `GET /api/graph` — nodes + relationship edges laid out in 3D (the higher-dim
//!     force layout, projected via PCA; rendered with three.js / 3d-force-graph)
//!   - `GET /api/chunk` — full code + summary for one chunk (the code box)
//!   - `GET /api/search` — semantic (cosine over stored embeddings) or keyword search
//!   - `GET /api/stats` — headline counts
//!
//! The server holds an owned snapshot (tree + per-chunk details + embeddings) plus a
//! shared [`GaiseClient`] handle used only to embed search queries, so the REPL keeps
//! ownership of its live `Indexer`/session while the server runs. The command blocks
//! until Ctrl+C (see [`App::run_visualize`]).

mod layout;

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use layout::{build_graph_view, GraphView, LayoutNode, LayoutParams};

use anyhow::{anyhow, Result};
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse, Json},
    routing::get,
    Router,
};
use colored::*;
use serde::{Deserialize, Serialize};

use gaise_core::GaiseClient;
use ikode::index::{query_embedding_request, ChunkRecord, FileIndex, Indexer};

use crate::app::App;

/// The static single-page app, embedded so the binary is self-contained.
const INDEX_HTML: &str = include_str!("index.html");

/// One node of the containment tree sent to the browser. `children` is always
/// present (possibly empty); the frontend decides what to collapse. Heavy content
/// (full source) is *not* here — the page fetches it lazily via `/api/chunk`.
#[derive(Debug, Clone, Serialize)]
pub struct TreeNode {
    /// Stable identity: directory/file `path`, or a chunk's `chunk_id`. The frontend
    /// uses this to expand-to / highlight a node from a search hit.
    pub id: String,
    pub name: String,
    /// `"Directory"`, `"File"`, or the chunk kind (`Function`, `Struct`, …).
    pub kind: String,
    /// Source language, on `File` nodes only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// `[start, end]` 1-based line span, on chunk nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<[usize; 2]>,
    /// One-line summary, when the chunk has been summarised.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    fn dir(path: &str, name: &str) -> Self {
        TreeNode {
            id: path.to_string(),
            name: name.to_string(),
            kind: "Directory".to_string(),
            language: None,
            lines: None,
            summary: None,
            children: Vec::new(),
        }
    }
}

/// Full detail for one chunk — served on demand when a tree node is opened.
#[derive(Debug, Clone, Serialize)]
pub struct ChunkDetail {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub line_start: usize,
    pub line_end: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// The faithful on-disk source of the chunk (indentation restored).
    pub code: String,
}

/// One search result row.
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub lines: [usize; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub score: f32,
}

/// Everything the server needs, owned — so it outlives the borrow of the `Indexer`.
pub struct VizState {
    root_name: String,
    tree: TreeNode,
    /// `chunk_id -> detail`, for the code box.
    chunks: HashMap<String, ChunkDetail>,
    /// `chunk_id -> embedding`, for semantic (cosine) search.
    embeddings: HashMap<String, Vec<f32>>,
    /// Nodes for the 3D graph: symbols + synthesised file/directory containers.
    layout_nodes: Vec<LayoutNode>,
    /// Edges for the 3D graph: semantic relationships + `CONTAINS`/`DEFINES` containment.
    layout_edges: Vec<(String, String, String)>,
    /// The 3D layout, computed once on first `/api/graph` hit (it's CPU-bound, and the
    /// tree view doesn't need it) and cached thereafter.
    graph_view: OnceLock<Arc<GraphView>>,
    /// Shared client, used only to embed the search query.
    client: Arc<dyn GaiseClient>,
    embedding_model: String,
    file_count: usize,
}

impl VizState {
    fn has_embeddings(&self) -> bool {
        !self.embeddings.is_empty()
    }

    /// The 3D graph view, building it on first use from the chunk set + relationship
    /// edges. Cached so repeated `/api/graph` requests are cheap.
    fn graph_view(&self) -> Arc<GraphView> {
        Arc::clone(self.graph_view.get_or_init(|| {
            Arc::new(build_graph_view(
                &self.layout_nodes,
                &self.layout_edges,
                &LayoutParams::default(),
            ))
        }))
    }
}

/// Build the server snapshot from the live index (read-only borrow). Pulls the
/// containment hierarchy and code from the in-memory `Indexer::files`, summaries and
/// embeddings from the graph.
pub fn build_state(
    indexer: &Indexer,
    client: Arc<dyn GaiseClient>,
    embedding_model: String,
    root_name: String,
) -> Arc<VizState> {
    let files = indexer.files();
    let summaries = indexer.summaries();
    let embeddings = indexer.embeddings_map();
    let rels = indexer.relationship_edges();

    let tree = build_tree(&root_name, files, &summaries);
    let chunks = build_chunk_details(files, &summaries);
    let (layout_nodes, layout_edges) = build_layout_inputs(files, rels);

    Arc::new(VizState {
        root_name,
        tree,
        chunks,
        embeddings,
        layout_nodes,
        layout_edges,
        graph_view: OnceLock::new(),
        client,
        embedding_model,
        file_count: files.len(),
    })
}

/// The last path segment of a `/`-separated relative path (the display name).
fn leaf_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Assemble the dirs → files → symbols tree. Directories are synthesised from file
/// paths; files carry their chunks, nested by `parent_id` (so methods sit under their
/// impl/class). Everything is sorted for a stable layout.
fn build_tree(
    root_name: &str,
    files: &HashMap<String, FileIndex>,
    summaries: &HashMap<String, String>,
) -> TreeNode {
    let mut root = TreeNode::dir(".", root_name);

    let mut paths: Vec<&String> = files.keys().collect();
    paths.sort();

    for path in paths {
        let fi = &files[path];
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            continue;
        }

        // Walk (creating as needed) the directory chain above the file.
        let mut cur = &mut root;
        let mut acc = String::new();
        for dir in &parts[..parts.len() - 1] {
            acc = if acc.is_empty() {
                dir.to_string()
            } else {
                format!("{}/{}", acc, dir)
            };
            let idx = cur
                .children
                .iter()
                .position(|c| c.kind == "Directory" && c.id == acc);
            let idx = match idx {
                Some(i) => i,
                None => {
                    cur.children.push(TreeNode::dir(&acc, dir));
                    cur.children.len() - 1
                }
            };
            cur = &mut cur.children[idx];
        }

        cur.children.push(build_file_node(fi, summaries));
    }

    sort_tree(&mut root);
    root
}

/// A `File` node with its chunk subtree attached.
fn build_file_node(fi: &FileIndex, summaries: &HashMap<String, String>) -> TreeNode {
    // Group chunks by parent (`""` = top-level / directly under the file).
    let mut by_parent: HashMap<&str, Vec<&ChunkRecord>> = HashMap::new();
    for ch in &fi.chunks {
        let key = ch.parent_id.as_deref().unwrap_or("");
        by_parent.entry(key).or_default().push(ch);
    }
    TreeNode {
        id: fi.path.clone(),
        name: leaf_name(&fi.path).to_string(),
        kind: "File".to_string(),
        language: Some(fi.language.clone()),
        lines: None,
        summary: None,
        children: build_chunk_children("", &by_parent, summaries),
    }
}

/// Recursively build the chunk nodes whose parent is `parent` (`""` for top-level).
fn build_chunk_children(
    parent: &str,
    by_parent: &HashMap<&str, Vec<&ChunkRecord>>,
    summaries: &HashMap<String, String>,
) -> Vec<TreeNode> {
    let mut kids = match by_parent.get(parent) {
        Some(k) => k.clone(),
        None => return Vec::new(),
    };
    kids.sort_by_key(|c| c.line_start);
    kids.into_iter()
        .map(|ch| TreeNode {
            id: ch.chunk_id.clone(),
            name: ch.name.clone(),
            kind: ch.kind.clone(),
            language: None,
            lines: Some([ch.line_start, ch.line_end]),
            summary: summaries.get(&ch.chunk_id).cloned(),
            children: build_chunk_children(&ch.chunk_id, by_parent, summaries),
        })
        .collect()
}

/// Sort each node's children: directories first, then files, then chunks; alphabetical
/// within a kind (chunks keep their by-line order from [`build_chunk_children`]).
fn sort_tree(node: &mut TreeNode) {
    node.children.sort_by(|a, b| {
        let rank = |k: &str| match k {
            "Directory" => 0,
            "File" => 1,
            _ => 2,
        };
        rank(&a.kind)
            .cmp(&rank(&b.kind))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    for c in &mut node.children {
        sort_tree(c);
    }
}

/// `chunk_id -> ChunkDetail` for every indexed chunk (full source for the code box).
fn build_chunk_details(
    files: &HashMap<String, FileIndex>,
    summaries: &HashMap<String, String>,
) -> HashMap<String, ChunkDetail> {
    let mut out = HashMap::new();
    for fi in files.values() {
        for ch in &fi.chunks {
            out.insert(
                ch.chunk_id.clone(),
                ChunkDetail {
                    id: ch.chunk_id.clone(),
                    name: ch.name.clone(),
                    kind: ch.kind.clone(),
                    path: ch.path.clone(),
                    line_start: ch.line_start,
                    line_end: ch.line_end,
                    summary: summaries.get(&ch.chunk_id).cloned(),
                    code: ch.indented_code(),
                },
            );
        }
    }
    out
}

/// Assemble the 3D graph inputs: symbol nodes plus synthesised **File** and
/// **Directory** container nodes, and the edge list combining semantic relationships
/// (`rels`) with structural **CONTAINS** (dir→dir/file) and **DEFINES** (file/parent→
/// symbol) containment. This mirrors the tree's hierarchy, so in the 3D layout symbols
/// cluster under their file and directory. The frontend can toggle the containers off
/// to recover the pure symbol-relationship view.
fn build_layout_inputs(
    files: &HashMap<String, FileIndex>,
    rels: Vec<(String, String, String)>,
) -> (Vec<LayoutNode>, Vec<(String, String, String)>) {
    let mut nodes: Vec<LayoutNode> = Vec::new();
    let mut edges = rels;
    let mut seen_dirs: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Deterministic order so the layout is reproducible.
    let mut paths: Vec<&String> = files.keys().collect();
    paths.sort();

    for path in paths {
        let fi = &files[path];
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            continue;
        }

        // Directory chain above the file: a node per cumulative prefix, wired
        // parent→child with CONTAINS (duplicates coalesce in the layout).
        let mut acc = String::new();
        let mut prev_dir: Option<String> = None;
        for dir in &parts[..parts.len() - 1] {
            acc = if acc.is_empty() {
                dir.to_string()
            } else {
                format!("{}/{}", acc, dir)
            };
            if seen_dirs.insert(acc.clone()) {
                nodes.push(LayoutNode {
                    id: acc.clone(),
                    name: dir.to_string(),
                    kind: "Directory".to_string(),
                    path: acc.clone(),
                    lines: [0, 0],
                });
            }
            if let Some(parent) = &prev_dir {
                edges.push((parent.clone(), acc.clone(), "CONTAINS".to_string()));
            }
            prev_dir = Some(acc.clone());
        }

        // File node, contained by its directory (if it has one).
        nodes.push(LayoutNode {
            id: path.clone(),
            name: leaf_name(path).to_string(),
            kind: "File".to_string(),
            path: path.clone(),
            lines: [0, 0],
        });
        if let Some(parent) = &prev_dir {
            edges.push((parent.clone(), path.clone(), "CONTAINS".to_string()));
        }

        // Symbols, each DEFINES'd by its parent symbol (nested member) or its file.
        for ch in &fi.chunks {
            nodes.push(LayoutNode {
                id: ch.chunk_id.clone(),
                name: ch.name.clone(),
                kind: ch.kind.clone(),
                path: ch.path.clone(),
                lines: [ch.line_start, ch.line_end],
            });
            let parent = ch.parent_id.clone().unwrap_or_else(|| path.clone());
            edges.push((parent, ch.chunk_id.clone(), "DEFINES".to_string()));
        }
    }

    (nodes, edges)
}

/// Cosine similarity, defensive against mismatched dims / zero vectors.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return -1.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// Embed one query string (as a code-retrieval query, matching how the index was
/// embedded), returning the single vector.
async fn embed_query(client: &dyn GaiseClient, model: &str, text: &str) -> Result<Vec<f32>> {
    let req = query_embedding_request(model, text);
    let resp = client
        .embeddings(&req)
        .await
        .map_err(|e| anyhow!("query embedding failed: {e}"))?;
    resp.output
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("embedding response was empty"))
}

// ----- HTTP handlers -------------------------------------------------------------

async fn page() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn api_tree(State(state): State<Arc<VizState>>) -> impl IntoResponse {
    Json(state.tree.clone())
}

async fn api_graph(State(state): State<Arc<VizState>>) -> impl IntoResponse {
    // The layout is CPU-bound; build it off the async worker so a big graph doesn't
    // stall the runtime. `graph_view()` caches, so this only does real work once.
    let view = tokio::task::spawn_blocking(move || state.graph_view())
        .await
        .map(|v| (*v).clone())
        .unwrap_or_else(|_| GraphView {
            nodes: Vec::new(),
            edges: Vec::new(),
            truncated: 0,
            dims: 0,
        });
    Json(view)
}

#[derive(Serialize)]
struct StatsResponse {
    root: String,
    files: usize,
    chunks: usize,
    embedded: usize,
    semantic: bool,
}

async fn api_stats(State(state): State<Arc<VizState>>) -> impl IntoResponse {
    Json(StatsResponse {
        root: state.root_name.clone(),
        files: state.file_count,
        chunks: state.chunks.len(),
        embedded: state.embeddings.len(),
        semantic: state.has_embeddings(),
    })
}

#[derive(Deserialize)]
struct ChunkParams {
    id: String,
}

async fn api_chunk(
    State(state): State<Arc<VizState>>,
    Query(params): Query<ChunkParams>,
) -> impl IntoResponse {
    match state.chunks.get(&params.id) {
        Some(detail) => Json(detail.clone()).into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "no such chunk").into_response(),
    }
}

#[derive(Deserialize)]
struct SearchParams {
    q: String,
    /// `"semantic"`, `"keyword"`, or `"auto"` (default).
    mode: Option<String>,
}

#[derive(Serialize)]
struct SearchResponse {
    /// The mode actually used (auto resolves to semantic/keyword; semantic falls back
    /// to keyword when there's no embedding index or the embed call fails).
    mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    hits: Vec<SearchHit>,
}

const SEARCH_K: usize = 25;

async fn api_search(
    State(state): State<Arc<VizState>>,
    Query(params): Query<SearchParams>,
) -> impl IntoResponse {
    let q = params.q.trim();
    if q.is_empty() {
        return Json(SearchResponse {
            mode: "keyword".to_string(),
            note: None,
            hits: Vec::new(),
        });
    }
    let want = params.mode.as_deref().unwrap_or("auto");
    let use_semantic = match want {
        "keyword" => false,
        "semantic" | "auto" => state.has_embeddings(),
        _ => state.has_embeddings(),
    };

    if use_semantic {
        match embed_query(state.client.as_ref(), &state.embedding_model, q).await {
            Ok(qvec) => {
                return Json(SearchResponse {
                    mode: "semantic".to_string(),
                    note: None,
                    hits: semantic_rank(&state.chunks, &state.embeddings, &qvec),
                });
            }
            Err(e) => {
                // Degrade to keyword rather than erroring, mirroring `/ask`.
                return Json(SearchResponse {
                    mode: "keyword".to_string(),
                    note: Some(format!(
                        "semantic unavailable ({e}); showing keyword matches"
                    )),
                    hits: keyword_rank(&state.chunks, q),
                });
            }
        }
    }

    let note = if want == "semantic" && !state.has_embeddings() {
        Some("no embedding index yet — run /embed for semantic search".to_string())
    } else {
        None
    };
    Json(SearchResponse {
        mode: "keyword".to_string(),
        note,
        hits: keyword_rank(&state.chunks, q),
    })
}

/// Rank chunks by cosine similarity of their stored embedding to the query vector.
fn semantic_rank(
    chunks: &HashMap<String, ChunkDetail>,
    embeddings: &HashMap<String, Vec<f32>>,
    qvec: &[f32],
) -> Vec<SearchHit> {
    let mut scored: Vec<(&String, f32)> = embeddings
        .iter()
        .map(|(id, v)| (id, cosine(v, qvec)))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored
        .into_iter()
        .take(SEARCH_K)
        .filter_map(|(id, score)| {
            chunks.get(id).map(|d| SearchHit {
                id: d.id.clone(),
                name: d.name.clone(),
                kind: d.kind.clone(),
                path: d.path.clone(),
                lines: [d.line_start, d.line_end],
                summary: d.summary.clone(),
                score,
            })
        })
        .collect()
}

/// Simple weighted keyword match over name / summary / path / code. Each query term
/// contributes its field weight; a name prefix/exact match gets a bonus, so the most
/// on-the-nose hits float to the top.
fn keyword_rank(chunks: &HashMap<String, ChunkDetail>, query: &str) -> Vec<SearchHit> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }

    let mut hits: Vec<SearchHit> = Vec::new();
    for d in chunks.values() {
        let name_l = d.name.to_lowercase();
        let path_l = d.path.to_lowercase();
        let summary_l = d.summary.as_deref().unwrap_or("").to_lowercase();
        let code_l = d.code.to_lowercase();

        let mut score = 0.0f32;
        let mut matched_any = false;
        for t in &terms {
            let mut term_score = 0.0f32;
            if name_l == *t {
                term_score += 12.0;
            } else if name_l.starts_with(t.as_str()) {
                term_score += 8.0;
            } else if name_l.contains(t.as_str()) {
                term_score += 5.0;
            }
            if summary_l.contains(t.as_str()) {
                term_score += 3.0;
            }
            if path_l.contains(t.as_str()) {
                term_score += 2.0;
            }
            if code_l.contains(t.as_str()) {
                term_score += 1.0;
            }
            if term_score > 0.0 {
                matched_any = true;
                score += term_score;
            }
        }
        if matched_any {
            hits.push(SearchHit {
                id: d.id.clone(),
                name: d.name.clone(),
                kind: d.kind.clone(),
                path: d.path.clone(),
                lines: [d.line_start, d.line_end],
                summary: d.summary.clone(),
                score,
            });
        }
    }
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.len().cmp(&b.name.len()))
    });
    hits.truncate(SEARCH_K);
    hits
}

/// The axum router for the visualizer, with the snapshot state attached.
pub fn router(state: Arc<VizState>) -> Router {
    Router::new()
        .route("/", get(page))
        .route("/api/tree", get(api_tree))
        .route("/api/graph", get(api_graph))
        .route("/api/stats", get(api_stats))
        .route("/api/chunk", get(api_chunk))
        .route("/api/search", get(api_search))
        .with_state(state)
}

/// Best-effort: open `url` in the user's default browser. Failure is silent — the
/// link is always printed, so the user can click/paste it themselves.
fn open_browser(url: &str) {
    let _spawn = |program: &str, args: &[&str]| {
        let _ = std::process::Command::new(program).args(args).spawn();
    };
    #[cfg(target_os = "windows")]
    _spawn("cmd", &["/c", "start", "", url]);
    #[cfg(target_os = "macos")]
    _spawn("open", &[url]);
    #[cfg(all(unix, not(target_os = "macos")))]
    _spawn("xdg-open", &[url]);
}

impl App {
    /// `/visualize [port]` — start the local web app over the current index and block
    /// until Ctrl+C, then return to the prompt. Indexes first if the graph is empty.
    /// Rides the shared `interrupt` watch channel for cancellation, like every other
    /// long-running op.
    pub(crate) async fn run_visualize(&mut self, port: u16) {
        if self.indexer.is_empty() {
            self.run_index();
        }

        let root_name = self
            .working_directory
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());

        let state = build_state(
            &self.indexer,
            Arc::clone(&self.client),
            self.embedding_model.clone(),
            root_name,
        );

        if state.chunks.is_empty() {
            println!(
                "{} Nothing indexed yet — run {} first, then {} again.",
                "⚠️ ".bright_yellow(),
                "/index".cyan(),
                "/visualize".cyan()
            );
            return;
        }

        // Bind first so a port clash is reported cleanly instead of aborting the REPL.
        let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            Ok(l) => l,
            Err(e) => {
                println!(
                    "{} Could not bind localhost:{} ({e}). Try {} with a free port.",
                    "⚠️ ".bright_yellow(),
                    port,
                    format!("/visualize {}", port + 1).cyan()
                );
                return;
            }
        };

        let url = format!("http://localhost:{}", port);
        println!(
            "{} Visualizer running at {}",
            "🌐".bright_cyan(),
            url.bright_cyan().bold().underline()
        );
        println!(
            "   {}",
            format!(
                "{} files · {} symbols · {} — press Ctrl+C to stop and return to the prompt.",
                state.file_count,
                state.chunks.len(),
                if state.has_embeddings() {
                    "semantic + keyword search"
                } else {
                    "keyword search (run /embed for semantic)"
                }
            )
            .dimmed()
        );
        open_browser(&url);

        let app = router(state);

        // Race the server against Ctrl+C, exactly like `App::cancellable`: ignore any
        // interrupt pressed before we start, then shut down on the next one.
        let mut interrupt = self.interrupt.clone();
        interrupt.borrow_and_update();
        tokio::select! {
            biased;
            _ = interrupt.changed() => {
                println!("\n{} Visualizer stopped — back to the prompt.", "⏹ ".bright_yellow());
            }
            r = axum::serve(listener, app) => {
                if let Err(e) = r {
                    println!("{} Visualizer server error: {e}", "⚠️ ".bright_yellow());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
