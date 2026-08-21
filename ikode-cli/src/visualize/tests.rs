//! Tests for visualizer state, ranking, and local HTTP routes.

use super::*;
use std::collections::HashMap;

fn chunk(
    id: &str,
    name: &str,
    kind: &str,
    path: &str,
    parent: Option<&str>,
    code: &str,
) -> ChunkRecord {
    ChunkRecord {
        chunk_id: id.to_string(),
        name: name.to_string(),
        kind: kind.to_string(),
        path: path.to_string(),
        line_start: 1,
        line_end: 9,
        code: code.to_string(),
        indent: String::new(),
        parent_id: parent.map(str::to_string),
    }
}

/// A tiny two-file project: one file at the root, one nested two dirs deep with a
/// struct that has a method inside it.
fn sample_files() -> HashMap<String, FileIndex> {
    let mut files = HashMap::new();
    files.insert(
        "README.md".to_string(),
        FileIndex {
            path: "README.md".to_string(),
            language: "Markdown".to_string(),
            content_hash: 0,
            chunks: vec![chunk(
                "README.md::intro",
                "intro",
                "Section",
                "README.md",
                None,
                "# hi",
            )],
        },
    );
    files.insert(
        "src/app/server.rs".to_string(),
        FileIndex {
            path: "src/app/server.rs".to_string(),
            language: "Rust".to_string(),
            content_hash: 0,
            chunks: vec![
                chunk(
                    "src/app/server.rs::Server",
                    "Server",
                    "Struct",
                    "src/app/server.rs",
                    None,
                    "struct Server { port: u16 }",
                ),
                chunk(
                    "src/app/server.rs::Server::serve",
                    "serve",
                    "Method",
                    "src/app/server.rs",
                    Some("src/app/server.rs::Server"),
                    "fn serve(&self) { listen() }",
                ),
            ],
        },
    );
    files
}

fn child<'a>(node: &'a TreeNode, name: &str) -> &'a TreeNode {
    node.children
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no child named {name} under {}", node.name))
}

/// The tree reflects the dirs → files → symbols containment hierarchy, with methods
/// nested under their parent type, directories sorted before files, and file
/// language carried through.
#[test]
fn build_tree_nests_dirs_files_and_symbols() {
    let files = sample_files();
    let summaries = HashMap::new();
    let root = build_tree("myproj", &files, &summaries);

    assert_eq!(root.name, "myproj");
    assert_eq!(root.kind, "Directory");
    // Directory ("src") sorts before the root-level file ("README.md").
    assert_eq!(root.children[0].kind, "Directory");
    assert_eq!(root.children[0].name, "src");

    let src = child(&root, "src");
    let app = child(src, "app");
    assert_eq!(app.id, "src/app"); // dir ids are cumulative paths
    let server = child(app, "server.rs");
    assert_eq!(server.kind, "File");
    assert_eq!(server.language.as_deref(), Some("Rust"));

    // The method is nested under the struct, not directly under the file.
    let struct_node = child(server, "Server");
    assert_eq!(struct_node.kind, "Struct");
    assert_eq!(struct_node.children.len(), 1);
    assert_eq!(struct_node.children[0].name, "serve");
    assert_eq!(struct_node.children[0].lines, Some([1, 9]));
}

/// Summaries are attached to the matching chunk node by chunk_id.
#[test]
fn build_tree_attaches_summaries() {
    let files = sample_files();
    let mut summaries = HashMap::new();
    summaries.insert(
        "src/app/server.rs::Server::serve".to_string(),
        "Starts listening for connections.".to_string(),
    );
    let root = build_tree("p", &files, &summaries);
    let serve = child(
        child(child(child(&root, "src"), "app"), "server.rs"),
        "Server",
    );
    assert_eq!(
        serve.children[0].summary.as_deref(),
        Some("Starts listening for connections.")
    );
}

/// Keyword search ranks an exact-name match above a body-only match, and a query
/// that hits nothing returns nothing.
#[test]
fn keyword_rank_prefers_name_matches() {
    let files = sample_files();
    let details = build_chunk_details(&files, &HashMap::new());

    let hits = keyword_rank(&details, "serve");
    assert!(!hits.is_empty());
    assert_eq!(hits[0].name, "serve"); // exact name beats the "listen()" body hit

    assert!(keyword_rank(&details, "nonexistent_zzz").is_empty());
}

/// Semantic ranking orders by cosine similarity to the query vector.
#[test]
fn semantic_rank_orders_by_cosine() {
    let files = sample_files();
    let details = build_chunk_details(&files, &HashMap::new());
    let mut embeddings = HashMap::new();
    // Server points the same way as the query; serve points away.
    embeddings.insert("src/app/server.rs::Server".to_string(), vec![1.0, 0.0]);
    embeddings.insert(
        "src/app/server.rs::Server::serve".to_string(),
        vec![0.0, 1.0],
    );

    let hits = semantic_rank(&details, &embeddings, &[1.0, 0.0]);
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, "src/app/server.rs::Server");
    assert!(hits[0].score > hits[1].score);
}

#[test]
fn cosine_handles_alignment_and_mismatch() {
    assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
    assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
    // Different dims are non-comparable, not a panic.
    assert_eq!(cosine(&[1.0, 0.0], &[1.0]), -1.0);
}

// ----- live HTTP smoke test --------------------------------------------------
// Proves the router actually binds, serves, routes, and serializes — i.e. the
// whole server path short of the browser (which the harness can't drive; the page
// itself is verified by loading it). The endpoints exercised here never touch the
// client, so the stub below only needs to satisfy the trait.

use async_trait::async_trait;
use gaise_core::contracts::{
    GaiseEmbeddingsRequest, GaiseEmbeddingsResponse, GaiseInstructRequest,
    GaiseInstructResponse, GaiseInstructStreamResponse,
};
use std::error::Error;
use std::io::{Read, Write as _};
use std::net::TcpStream;
use std::pin::Pin;

struct StubClient;

#[async_trait]
impl GaiseClient for StubClient {
    async fn instruct_stream(
        &self,
        _r: &GaiseInstructRequest,
    ) -> Result<
        Pin<
            Box<
                dyn futures_util::Stream<
                        Item = Result<GaiseInstructStreamResponse, Box<dyn Error + Send + Sync>>,
                    > + Send,
            >,
        >,
        Box<dyn Error + Send + Sync>,
    > {
        Err("unused".into())
    }

    async fn instruct(
        &self,
        _r: &GaiseInstructRequest,
    ) -> Result<GaiseInstructResponse, Box<dyn Error + Send + Sync>> {
        Err("unused".into())
    }

    async fn embeddings(
        &self,
        _r: &GaiseEmbeddingsRequest,
    ) -> Result<GaiseEmbeddingsResponse, Box<dyn Error + Send + Sync>> {
        Err("unused".into())
    }
}

fn test_state() -> Arc<VizState> {
    let files = sample_files();
    let summaries = HashMap::new();
    let rels = vec![(
        "src/app/server.rs::Server::serve".to_string(),
        "src/app/server.rs::Server".to_string(),
        "REFERENCES".to_string(),
    )];
    let (layout_nodes, layout_edges) = build_layout_inputs(&files, rels);
    Arc::new(VizState {
        root_name: "myproj".to_string(),
        tree: build_tree("myproj", &files, &summaries),
        chunks: build_chunk_details(&files, &summaries),
        embeddings: HashMap::new(),
        layout_nodes,
        layout_edges,
        graph_view: OnceLock::new(),
        client: Arc::new(StubClient),
        embedding_model: "test::model".to_string(),
        file_count: files.len(),
    })
}

/// One-shot HTTP/1.1 GET over a raw socket (`Connection: close`, so the server ends
/// the body by closing — no chunked parsing needed). Returns the full raw response.
fn raw_get(addr: std::net::SocketAddr, path: &str) -> String {
    let mut s = TcpStream::connect(addr).expect("connect");
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).expect("write");
    let mut buf = String::new();
    s.read_to_string(&mut buf).expect("read");
    buf
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_serves_page_tree_search_and_404() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(test_state());
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    // The page.
    let page = tokio::task::spawn_blocking(move || raw_get(addr, "/"))
        .await
        .unwrap();
    assert!(page.starts_with("HTTP/1.1 200"));
    assert!(page.contains("<title>iKode · visualize</title>"));

    // Stats JSON.
    let stats = tokio::task::spawn_blocking(move || raw_get(addr, "/api/stats"))
        .await
        .unwrap();
    assert!(stats.contains("\"root\":\"myproj\""));
    assert!(stats.contains("\"files\":2"));

    // Tree JSON carries the containment hierarchy.
    let tree = tokio::task::spawn_blocking(move || raw_get(addr, "/api/tree"))
        .await
        .unwrap();
    assert!(tree.contains("\"kind\":\"Directory\""));
    assert!(tree.contains("\"name\":\"server.rs\""));

    // 3D graph JSON: nodes carry projected coords, the edge connects the two chunks.
    let graph = tokio::task::spawn_blocking(move || raw_get(addr, "/api/graph"))
        .await
        .unwrap();
    assert!(graph.contains("\"nodes\":"));
    assert!(graph.contains("\"edges\":"));
    assert!(graph.contains("\"label\":\"REFERENCES\""));

    // Keyword search (no client touched).
    let search =
        tokio::task::spawn_blocking(move || raw_get(addr, "/api/search?q=serve&mode=keyword"))
            .await
            .unwrap();
    assert!(search.contains("\"mode\":\"keyword\""));
    assert!(search.contains("\"name\":\"serve\""));

    // A fetch for an unknown chunk is a clean 404.
    let missing = tokio::task::spawn_blocking(move || raw_get(addr, "/api/chunk?id=nope"))
        .await
        .unwrap();
    assert!(missing.starts_with("HTTP/1.1 404"));
}
