//! Integration tests for the indexer: chunking, search, the livec graph mirror,
//! REFERENCES linking, impact traversal, stats, and WAL persistence/replay.

use ikode::index::{
    is_symbol_name, parse_generics, parse_impl_header, parse_supertypes, DepEdges, GraphQuerySpec,
    Indexer,
};
use ikode::lang;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn write(root: &Path, rel: &str, content: &str) -> PathBuf {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&p, content).unwrap();
    p
}

fn index_one(idx: &mut Indexer, root: &Path, rel: &str, content: &str) -> usize {
    let p = write(root, rel, content);
    let spec = lang::detect_language(&p).unwrap();
    idx.index_file(&p, &spec).unwrap()
}

#[test]
fn symbol_name_filtering() {
    assert!(is_symbol_name("compute_total"));
    assert!(is_symbol_name("_private"));
    assert!(is_symbol_name("Camel123"));
    assert!(!is_symbol_name("Build & test"));
    assert!(!is_symbol_name("123abc"));
    assert!(!is_symbol_name(""));
}

#[test]
fn new_creates_ikode_dir_and_wal_in_ikode_folder() {
    let dir = tempdir().unwrap();
    let _idx = Indexer::new(dir.path().to_path_buf());
    assert!(dir.path().join(".ikode").is_dir());
    assert!(dir.path().join(".ikode").join("graph.log").exists());
}

#[test]
fn index_file_produces_stable_deduped_chunk_ids() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    let n = index_one(
        &mut idx,
        dir.path(),
        "src/lib.rs",
        "fn alpha() {}\nfn alpha() {}\nstruct Zeta { x: i32 }\n",
    );
    assert_eq!(n, 3);
    let fi = idx.outline("src/lib.rs").unwrap();
    let ids: Vec<&str> = fi.chunks.iter().map(|c| c.chunk_id.as_str()).collect();
    assert!(ids.contains(&"src/lib.rs::alpha"));
    assert!(ids.contains(&"src/lib.rs::alpha#1"));
    assert!(ids.contains(&"src/lib.rs::Zeta"));
}

#[test]
fn chunk_records_factor_out_common_indent() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "src/lib.rs",
        "impl Widget {\n    fn render(&self) {\n        draw();\n    }\n}\n",
    );
    let fi = idx.outline("src/lib.rs").unwrap();
    let render = fi.chunks.iter().find(|c| c.name == "render").unwrap();
    // The method body's shared 4-space prefix moved into `indent`...
    assert_eq!(render.indent, "    ");
    // ...and is no longer repeated on each stored line.
    assert!(render.code.starts_with("fn render"));
    assert!(!render.code.contains("\n    fn render"));
    // The top-level impl has no shared indent.
    let imp = fi.chunks.iter().find(|c| c.kind == "ImplBlock").unwrap();
    assert_eq!(imp.indent, "");
}

#[test]
fn indented_code_reconstructs_the_on_disk_text() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    let src =
        "impl Widget {\n    fn render(&self) {\n        draw();\n        flush();\n    }\n}\n";
    index_one(&mut idx, dir.path(), "src/lib.rs", src);
    let fi = idx.outline("src/lib.rs").unwrap();
    let render = fi.chunks.iter().find(|c| c.name == "render").unwrap();
    // Reconstructed text equals the original lines 2..=5 of the file.
    let want = src.lines().skip(1).take(4).collect::<Vec<_>>().join("\n");
    assert_eq!(render.indented_code(), want);
}

#[test]
fn top_level_chunk_indented_code_is_unchanged() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "src/lib.rs",
        "fn alpha() {\n    let x = 1;\n}\n",
    );
    let fi = idx.outline("src/lib.rs").unwrap();
    let alpha = fi.chunks.iter().find(|c| c.name == "alpha").unwrap();
    assert_eq!(alpha.indent, "");
    assert_eq!(alpha.indented_code(), alpha.code);
    assert_eq!(alpha.indented_code(), "fn alpha() {\n    let x = 1;\n}");
}

#[test]
fn dedent_does_not_disturb_search_or_modifiers() {
    // Stripping leading indent must not change token search hits or inferred
    // visibility (both operate indentation-insensitively).
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "src/lib.rs",
        "impl Widget {\n    pub fn render(&self) {\n        draw();\n    }\n}\n",
    );
    // The nested method is still findable by a body token.
    let hits = idx.search("draw", 8);
    assert!(hits.iter().any(|c| c.name == "render"));
    // And its `pub` visibility was inferred from the (dedented) signature line.
    let fi = idx.outline("src/lib.rs").unwrap();
    let render = fi.chunks.iter().find(|c| c.name == "render").unwrap();
    assert!(render.code.starts_with("pub fn render"));
}

#[test]
fn java_methods_become_nodes_and_satisfy_an_interface() {
    // Keyword-less method extraction lets Java interface/class methods form their own
    // Method nodes — which in turn lets SATISFIES edges form (previously a known gap).
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "Shape.java",
        "public interface Shape {\n    double area();\n}\n",
    );
    write(
        dir.path(),
        "Circle.java",
        "public class Circle implements Shape {\n    public double area() { return 3.14; }\n}\n",
    );
    idx.index_all();
    // The interface and class methods are both nodes.
    assert!(node_exists(&idx, "area"));
    let el = edge_labels(&idx);
    assert!(el.contains(&"IMPLEMENTS".to_string()), "labels = {:?}", el);
    assert!(el.contains(&"SATISFIES".to_string()), "labels = {:?}", el);
}

#[test]
fn java_method_is_pruned_when_removed() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "Svc.java",
        "public class Svc {\n    public void alphaJava() {}\n    public void betaJava() {}\n}\n",
    );
    idx.index_all();
    assert!(node_exists(&idx, "alphaJava"));
    assert!(node_exists(&idx, "betaJava"));

    write(
        dir.path(),
        "Svc.java",
        "public class Svc {\n    public void alphaJava() {}\n}\n",
    );
    idx.index_all();
    assert!(
        node_exists(&idx, "alphaJava"),
        "surviving method must remain"
    );
    assert!(
        !node_exists(&idx, "betaJava"),
        "removed Java method must be pruned"
    );
}

#[test]
fn decorations_are_folded_through_the_indexer() {
    // An attribute + doc comment above a definition become part of the chunk, and the
    // chunk still reconstructs to the faithful on-disk text via indented_code().
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    let src = "/// Sums a slice.\n#[inline]\npub fn sum_all(xs: &[i32]) -> i32 {\n    xs.iter().sum()\n}\n";
    index_one(&mut idx, dir.path(), "src/lib.rs", src);
    let fi = idx.outline("src/lib.rs").unwrap();
    let sum = fi.chunks.iter().find(|c| c.name == "sum_all").unwrap();
    assert_eq!(sum.line_start, 1);
    assert!(sum
        .code
        .starts_with("/// Sums a slice.\n#[inline]\npub fn sum_all"));
    let want = src
        .lines()
        .take(sum.line_end)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(sum.indented_code(), want);
}

#[test]
fn strips_utf8_bom_before_chunking() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(&mut idx, dir.path(), "b.rs", "\u{feff}fn first_fn() {}\n");
    let fi = idx.outline("b.rs").unwrap();
    assert_eq!(fi.chunks[0].name, "first_fn");
}

#[test]
fn reindex_unchanged_content_is_cached() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    let a = index_one(&mut idx, dir.path(), "c.rs", "fn one() {}\n");
    let b = index_one(&mut idx, dir.path(), "c.rs", "fn one() {}\n");
    assert_eq!(a, b);
    assert_eq!(
        idx.language_breakdown()
            .iter()
            .map(|(_, n)| n)
            .sum::<usize>(),
        1
    );
}

#[test]
fn search_ranks_name_matches_above_body_matches() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "s.rs",
        "fn target_func() { let x = 1; }\nfn other_fn() { target_func(); }\n",
    );
    idx.index_all();
    let hits = idx.search("target_func", 10);
    assert!(!hits.is_empty());
    assert_eq!(hits[0].name, "target_func");
    assert!(idx.search("", 10).is_empty());
}

#[test]
fn outline_resolves_path_and_reports_language() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(dir.path(), "o.rs", "fn a() {}\nfn b() {}\n");
    idx.index_all();
    let fi = idx.outline("o.rs").unwrap();
    assert_eq!(fi.language, "Rust");
    assert_eq!(fi.chunks.len(), 2);
    assert!(idx.outline("does/not/exist.rs").is_none());
}

#[test]
fn index_all_skips_ignored_directories() {
    let dir = tempdir().unwrap();
    write(dir.path(), "keep.rs", "fn keep_me() {}\n");
    write(dir.path(), "node_modules/skip.rs", "fn skip_me() {}\n");
    write(dir.path(), "target/skip2.rs", "fn skip_me2() {}\n");
    // .NET build output: bin/ and obj/ are generated artefacts, not source.
    write(dir.path(), "bin/Debug/skip3.cs", "class Skip3 {}\n");
    write(dir.path(), "obj/skip4.cs", "class Skip4 {}\n");
    let mut idx = Indexer::new(dir.path().to_path_buf());
    let stats = idx.index_all();
    assert!(idx.outline("keep.rs").is_some());
    assert!(idx.outline("node_modules/skip.rs").is_none());
    assert!(idx.outline("bin/Debug/skip3.cs").is_none());
    assert!(idx.outline("obj/skip4.cs").is_none());
    assert!(stats.files >= 1);
}

#[test]
fn nested_ikignore_rules_are_scoped_and_prune_existing_graph_files() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "keep.rs",
        "pub fn keep_in_graph() -> i32 { 1 }\n",
    );
    write(
        dir.path(),
        "root_skip.rs",
        "pub fn remove_from_root_rule() -> i32 { 2 }\n",
    );
    write(
        dir.path(),
        "packages/alpha/local.rs",
        "pub fn remove_from_nested_name() -> i32 { 3 }\n",
    );
    write(
        dir.path(),
        "packages/alpha/generated/deep.rs",
        "pub fn remove_from_nested_directory() -> i32 { 4 }\n",
    );
    write(
        dir.path(),
        "packages/beta/local.rs",
        "pub fn keep_same_name_in_sibling() -> i32 { 5 }\n",
    );
    write(
        dir.path(),
        "packages/beta/generated/deep.rs",
        "pub fn keep_same_directory_in_sibling() -> i32 { 6 }\n",
    );

    let mut idx = Indexer::new(dir.path().to_path_buf());
    idx.index_all();
    assert!(idx.outline("root_skip.rs").is_some());
    assert!(idx.outline("packages/alpha/local.rs").is_some());
    assert!(idx.outline("packages/alpha/generated/deep.rs").is_some());

    write(dir.path(), ".ikignore", "# root-relative\nroot_skip.rs\n");
    write(
        dir.path(),
        "packages/alpha/.ikignore",
        "# relative to packages/alpha\nlocal.rs\ngenerated/\n",
    );

    let discrepancy = idx.project_discrepancies();
    assert!(discrepancy.deleted.contains(&"root_skip.rs".to_string()));
    assert!(discrepancy
        .deleted
        .contains(&"packages/alpha/local.rs".to_string()));
    assert!(discrepancy
        .deleted
        .contains(&"packages/alpha/generated/deep.rs".to_string()));

    let stats = idx.index_all();
    assert_eq!(
        stats.ikignore_files,
        vec![
            ".ikignore".to_string(),
            "packages/alpha/.ikignore".to_string()
        ]
    );
    assert_eq!(stats.ignored, 3);
    assert_eq!(stats.removed, 3);
    assert!(stats.error.is_none());

    // Ignored files have left both the in-memory index and persisted graph.
    assert!(idx.outline("root_skip.rs").is_none());
    assert!(idx.outline("packages/alpha/local.rs").is_none());
    assert!(idx.outline("packages/alpha/generated/deep.rs").is_none());
    let graph_paths = idx.graph_file_paths();
    assert!(!graph_paths.contains("root_skip.rs"));
    assert!(!graph_paths.contains("packages/alpha/local.rs"));
    assert!(!graph_paths.contains("packages/alpha/generated/deep.rs"));
    let graph_dirs = idx.graph_dir_paths();
    assert!(!graph_dirs.contains("packages/alpha"));
    assert!(!graph_dirs.contains("packages/alpha/generated"));

    // The nested rules do not leak into a sibling directory.
    assert!(idx.outline("keep.rs").is_some());
    assert!(idx.outline("packages/beta/local.rs").is_some());
    assert!(idx.outline("packages/beta/generated/deep.rs").is_some());
}

#[test]
fn prune_ikignored_applies_rules_added_mid_session_without_a_full_index() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "src/keep.rs",
        "pub fn keep_after_refresh() {}\n",
    );
    write(
        dir.path(),
        "src/generated/remove.rs",
        "pub fn remove_after_refresh() {}\n",
    );
    let mut idx = Indexer::new(dir.path().to_path_buf());
    idx.index_all();
    assert!(idx.outline("src/generated/remove.rs").is_some());

    write(dir.path(), "src/.ikignore", "generated/\n");
    let report = idx.prune_ikignored().unwrap();
    assert_eq!(report.files, vec!["src/.ikignore"]);
    assert_eq!(report.removed, 1);
    assert!(idx.outline("src/generated/remove.rs").is_none());
    assert!(idx.outline("src/keep.rs").is_some());
    assert!(!idx.graph_dir_paths().contains("src/generated"));

    let chunks = index_one(
        &mut idx,
        dir.path(),
        "src/generated/added_later.rs",
        "pub fn never_enters_graph() {}\n",
    );
    assert_eq!(chunks, 0);
    assert!(idx.outline("src/generated/added_later.rs").is_none());
    assert!(!idx
        .graph_file_paths()
        .contains("src/generated/added_later.rs"));
}

#[test]
fn references_are_built_and_impact_traverses_both_directions() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(dir.path(), "a.rs", "pub fn leaf_node() -> i32 { 1 }\n");
    write(
        dir.path(),
        "b.rs",
        "pub fn middle_layer() -> i32 { leaf_node() + 1 }\n",
    );
    write(
        dir.path(),
        "c.rs",
        "pub fn top_caller() -> i32 { middle_layer() + 1 }\n",
    );
    let stats = idx.index_all();
    assert!(
        stats.links >= 2,
        "expected >=2 reference edges, got {}",
        stats.links
    );

    // callers of leaf_node at depth 1 => only middle_layer
    let (found, hits) = idx.impact("leaf_node", 1, true);
    assert!(found);
    let n1: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
    assert!(n1.contains(&"middle_layer"), "depth1 callers = {:?}", n1);
    assert!(!n1.contains(&"top_caller"));

    // depth 2 => middle_layer + top_caller
    let (_f, hits2) = idx.impact("leaf_node", 2, true);
    let n2: Vec<&str> = hits2.iter().map(|h| h.name.as_str()).collect();
    assert!(
        n2.contains(&"middle_layer") && n2.contains(&"top_caller"),
        "depth2 = {:?}",
        n2
    );

    // callees of top_caller at depth 1 => middle_layer
    let (_f2, hits3) = idx.impact("top_caller", 1, false);
    let n3: Vec<&str> = hits3.iter().map(|h| h.name.as_str()).collect();
    assert!(n3.contains(&"middle_layer"), "callees = {:?}", n3);

    // depth ordering is ascending
    assert!(hits2.windows(2).all(|w| w[0].depth <= w[1].depth));

    // unknown symbol => not found, no hits
    let (found_u, hits_u) = idx.impact("nonexistent_symbol", 3, true);
    assert!(!found_u);
    assert!(hits_u.is_empty());
}

/// A function that *calls* a helper but only *names* a type gets a CALLS edge to
/// the helper and a REFERENCES edge to the type — the split that separates the
/// precise call graph from incidental mentions.
#[test]
fn calls_and_references_are_distinguished() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "w.rs",
        "pub struct WidgetThing { count: i32 }\n\
         pub fn build_value() -> i32 { 1 }\n\
         pub fn consume(item: WidgetThing) -> i32 { build_value() + item.count }\n",
    );
    idx.index_all();

    // Retrieve all three chunks so both edges sit inside the connectivity subgraph.
    let result = idx.ask("consume build_value WidgetThing", 10);
    let conn = &result.connectivity;
    // consume calls build_value() => CALLS.
    assert!(
        conn.iter()
            .any(|e| e.kind == "CALLS" && e.from == "w.rs::consume" && e.to == "w.rs::build_value"),
        "expected CALLS consume->build_value; connectivity = {:?}",
        conn
    );
    // consume only *names* WidgetThing in its signature (never `WidgetThing(`) => REFERENCES.
    assert!(
        conn.iter().any(|e| e.kind == "REFERENCES"
            && e.from == "w.rs::consume"
            && e.to == "w.rs::WidgetThing"),
        "expected REFERENCES consume->WidgetThing; connectivity = {:?}",
        conn
    );
    // And the helper is never mislabelled as a mere reference.
    assert!(
        !conn.iter().any(|e| e.kind == "REFERENCES"
            && e.from == "w.rs::consume"
            && e.to == "w.rs::build_value"),
        "build_value should be CALLS-only; connectivity = {:?}",
        conn
    );
}

/// `impact_with` honours the edge-kind filter: a true call is reachable via CALLS,
/// a bare type mention only via REFERENCES.
#[test]
fn impact_filters_by_edge_kind() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "w.rs",
        "pub struct WidgetThing { count: i32 }\n\
         pub fn build_value() -> i32 { 1 }\n\
         pub fn consume(item: WidgetThing) -> i32 { build_value() + item.count }\n",
    );
    idx.index_all();

    // build_value is *called* by consume — reachable via CALLS, and via All.
    let calls_only = idx.impact_with("build_value", 1, true, DepEdges::Calls).1;
    assert!(
        calls_only.iter().any(|h| h.name == "consume"),
        "CALLS callers = {:?}",
        calls_only
    );
    assert!(idx
        .impact_with("build_value", 1, true, DepEdges::All)
        .1
        .iter()
        .any(|h| h.name == "consume"));
    // ...but NOT via REFERENCES (it's a call, not a mention).
    assert!(idx
        .impact_with("build_value", 1, true, DepEdges::References)
        .1
        .is_empty());

    // WidgetThing is only *mentioned* — reachable via REFERENCES, not CALLS.
    assert!(idx
        .impact_with("WidgetThing", 1, true, DepEdges::References)
        .1
        .iter()
        .any(|h| h.name == "consume"));
    assert!(idx
        .impact_with("WidgetThing", 1, true, DepEdges::Calls)
        .1
        .is_empty());
}

/// graph_query: traverse CALLS to find callees/callers, and use a where-only query
/// to list nodes by kind + path + visibility.
#[test]
fn graph_query_traverses_and_filters() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "src/m.rs",
        "pub fn helper_one() -> i32 { 1 }\n\
         pub fn caller_two() -> i32 { helper_one() + 1 }\n\
         fn private_three() {}\n",
    );
    idx.index_all();

    let q = |json: &str| {
        let spec: GraphQuerySpec = serde_json::from_str(json).unwrap();
        idx.graph_query(&spec).unwrap()
    };

    // Callees of caller_two via CALLS (dir out) => helper_one.
    let callees = q(
        r#"{"start":{"symbol":"caller_two"},"traverse":[{"edge":"CALLS","dir":"out","depth":1}]}"#,
    );
    assert!(
        callees.iter().any(|r| r.chunk_id == "src/m.rs::helper_one"),
        "callees = {:?}",
        callees
    );

    // Callers of helper_one via CALLS (dir in) => caller_two.
    let callers = q(
        r#"{"start":{"symbol":"helper_one"},"traverse":[{"edge":"CALLS","dir":"in","depth":1}]}"#,
    );
    assert!(
        callers.iter().any(|r| r.chunk_id == "src/m.rs::caller_two"),
        "callers = {:?}",
        callers
    );

    // where-only: public Functions under src/ => helper_one + caller_two, not private_three.
    let pubs = q(r#"{"where":{"kind":["Function"],"path_prefix":"src/","visibility":"public"}}"#);
    let names: Vec<&str> = pubs.iter().map(|r| r.name.as_str()).collect();
    assert!(
        names.contains(&"helper_one") && names.contains(&"caller_two"),
        "names = {:?}",
        names
    );
    assert!(
        !names.contains(&"private_three"),
        "private fn leaked: {:?}",
        names
    );

    // A query with neither anchor nor filter is rejected as too broad.
    let broad: GraphQuerySpec = serde_json::from_str("{}").unwrap();
    assert!(idx.graph_query(&broad).is_err());
}

#[test]
fn graph_stats_counts_nodes_and_edge_labels() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "g.rs",
        "pub fn caller_fn() { helper_fn(); }\npub fn helper_fn() {}\n",
    );
    idx.index_all();
    let stats = idx.graph_stats().unwrap();
    assert!(stats.nodes_total >= 3, "nodes={}", stats.nodes_total);
    assert!(stats.edges_total >= 1, "edges={}", stats.edges_total);
    let nl: Vec<&str> = stats.node_labels.iter().map(|(l, _)| l.as_str()).collect();
    assert!(nl.contains(&"File"));
    assert!(nl.contains(&"Function"));
    let el: Vec<&str> = stats.edge_labels.iter().map(|(l, _)| l.as_str()).collect();
    assert!(el.contains(&"DEFINES"));
    // `caller_fn` invokes `helper_fn()` — a call, so a CALLS edge (not a bare mention).
    assert!(el.contains(&"CALLS"), "edge labels = {:?}", el);
}

#[test]
fn graph_persists_to_wal_and_replays_on_reopen() {
    let dir = tempdir().unwrap();
    {
        let mut idx = Indexer::new(dir.path().to_path_buf());
        write(
            dir.path(),
            "p.rs",
            "pub fn aaa_fn() { bbb_fn(); }\npub fn bbb_fn() {}\n",
        );
        idx.index_all();
    }
    let wal = dir.path().join(".ikode").join("graph.log");
    assert!(fs::metadata(&wal).unwrap().len() > 0);

    // A fresh Indexer replays the WAL and sees the graph without re-indexing files.
    let idx2 = Indexer::new(dir.path().to_path_buf());
    let stats = idx2.graph_stats().unwrap();
    assert!(
        stats.nodes_total >= 3,
        "replayed nodes = {}",
        stats.nodes_total
    );
}

#[test]
fn squash_preserves_state_and_replays_from_compacted_wal() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "p.rs",
        "pub fn aaa_fn() { bbb_fn(); }\npub fn bbb_fn() {}\n",
    );
    idx.index_all();

    // Churn the WAL: repeatedly rewrite + re-index so it accumulates superseded
    // history (every pass appends; old node/edge versions are never removed).
    for i in 0..6 {
        write(
            dir.path(),
            "p.rs",
            &format!("pub fn aaa_fn() {{ bbb_fn(); }}\npub fn bbb_fn() {{ let _v{i} = {i}; }}\n"),
        );
        idx.index_all();
    }

    let before = idx.graph_stats().unwrap();
    let wal_before = fs::metadata(idx.wal_path()).unwrap().len();

    let s = idx.squash().unwrap();
    assert_eq!(s.before_bytes, wal_before);
    assert_eq!(s.nodes, before.nodes_total, "squash node count");
    assert_eq!(s.edges, before.edges_total, "squash edge count");
    assert!(s.uncompressed_snapshot_bytes > 0);
    assert!(s.last_sequence > 0);
    // Churned history means the compacted log is strictly smaller than the original.
    assert!(
        s.after_bytes < s.before_bytes,
        "after {} not < before {}",
        s.after_bytes,
        s.before_bytes
    );

    // Squash leaves the *live* graph's meaning untouched.
    let after = idx.graph_stats().unwrap();
    assert_eq!(after.nodes_total, before.nodes_total);
    assert_eq!(after.edges_total, before.edges_total);
    assert_eq!(after.node_labels, before.node_labels);
    assert_eq!(after.edge_labels, before.edge_labels);

    // A fresh Indexer replaying the compacted WAL reconstructs the same state.
    drop(idx);
    let idx2 = Indexer::new(dir.path().to_path_buf());
    let replayed = idx2.graph_stats().unwrap();
    assert_eq!(replayed.nodes_total, before.nodes_total, "replayed nodes");
    assert_eq!(replayed.edges_total, before.edges_total, "replayed edges");
    assert_eq!(replayed.node_labels, before.node_labels);
    assert_eq!(replayed.edge_labels, before.edge_labels);
}

#[test]
fn parse_impl_header_handles_generics_and_inherent() {
    assert_eq!(
        parse_impl_header("impl<T: Display> ToString for Wrapper<T> {"),
        Some((Some("ToString".to_string()), "Wrapper".to_string()))
    );
    assert_eq!(
        parse_impl_header("impl Point {"),
        Some((None, "Point".to_string()))
    );
    assert_eq!(
        parse_impl_header("impl crate::fmt::Display for foo::Bar {"),
        Some((Some("Display".to_string()), "Bar".to_string()))
    );
    assert!(parse_impl_header("implements something").is_none());
    assert!(parse_impl_header("fn not_an_impl() {}").is_none());
}

#[test]
fn parse_supertypes_across_languages() {
    assert_eq!(
        parse_supertypes("Java", "public class Foo extends Bar implements Baz, Qux {"),
        vec!["Bar", "Baz", "Qux"]
    );
    assert_eq!(
        parse_supertypes("Python", "class Foo(Bar, Baz):"),
        vec!["Bar", "Baz"]
    );
    assert_eq!(
        parse_supertypes("C#", "class Foo : Base, IBar {"),
        vec!["Base", "IBar"]
    );
    // Python implicit object base is dropped.
    assert!(parse_supertypes("Python", "class Foo(object):").is_empty());
    // Rust is handled by parse_impl_header, not here.
    assert!(parse_supertypes("Rust", "struct Foo;").is_empty());
}

#[test]
fn test_chunks_create_tests_edges_and_guarding_tests() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "src/lib.rs",
        "pub fn target_fn() -> i32 { 1 }\n",
    );
    write(
        dir.path(),
        "tests/lib_test.rs",
        "fn test_target() { target_fn(); }\n",
    );
    idx.index_all();

    // A TESTS edge label exists in the graph.
    let stats = idx.graph_stats().unwrap();
    let el: Vec<&str> = stats.edge_labels.iter().map(|(l, _)| l.as_str()).collect();
    assert!(el.contains(&"TESTS"), "edge labels = {:?}", el);

    // guarding_tests(target_fn) finds the test chunk.
    let tests = idx.guarding_tests(&["target_fn".to_string()]);
    let names: Vec<&str> = tests.iter().map(|t| t.name.as_str()).collect();
    assert!(
        names.contains(&"test_target"),
        "guarding tests = {:?}",
        names
    );
}

#[test]
fn rust_impl_creates_implements_for_type_has_impl_edges() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "shapes.rs",
        "pub trait Area { fn area(&self) -> f64; }\n\
         pub struct Circle { r: f64 }\n\
         impl Area for Circle { fn area(&self) -> f64 { 3.0 } }\n",
    );
    idx.index_all();
    let stats = idx.graph_stats().unwrap();
    let el: Vec<&str> = stats.edge_labels.iter().map(|(l, _)| l.as_str()).collect();
    for want in ["IMPLEMENTS", "FOR_TYPE", "HAS_IMPL"] {
        assert!(el.contains(&want), "missing {} in {:?}", want, el);
    }
}

#[test]
fn java_implements_creates_implements_edge() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(dir.path(), "Shape.java", "public interface Shape { }\n");
    write(
        dir.path(),
        "Circle.java",
        "public class Circle implements Shape { }\n",
    );
    idx.index_all();
    let stats = idx.graph_stats().unwrap();
    let el: Vec<&str> = stats.edge_labels.iter().map(|(l, _)| l.as_str()).collect();
    assert!(el.contains(&"IMPLEMENTS"), "edge labels = {:?}", el);
}

#[test]
fn ask_returns_chunks_and_connectivity() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "m.rs",
        "pub fn helper_one() -> i32 { 1 }\npub fn caller_two() -> i32 { helper_one() + 1 }\n",
    );
    idx.index_all();
    let result = idx.ask("helper_one caller_two", 10);
    let ids: Vec<&str> = result.chunks.iter().map(|c| c.chunk_id.as_str()).collect();
    assert!(ids.contains(&"m.rs::helper_one"));
    assert!(ids.contains(&"m.rs::caller_two"));
    // The connectivity subgraph links the two retrieved chunks (caller_two calls
    // helper_one() — a CALLS edge).
    assert!(
        result.connectivity.iter().any(|e| e.kind == "CALLS"
            && e.from == "m.rs::caller_two"
            && e.to == "m.rs::helper_one"),
        "connectivity = {:?}",
        result.connectivity
    );
}

#[test]
fn ask_surfaces_external_referrers_as_impact_set() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "m.rs",
        "pub fn helper_one() -> i32 { 1 }\npub fn caller_two() -> i32 { helper_one() + 1 }\n",
    );
    idx.index_all();
    // Retrieve only helper_one (k=1 — the name match outranks the body match in caller_two).
    let result = idx.ask("helper_one", 1);
    let ids: Vec<&str> = result.chunks.iter().map(|c| c.chunk_id.as_str()).collect();
    assert_eq!(ids, vec!["m.rs::helper_one"]);
    // caller_two is *outside* the retrieved set, so it shows up as impact, not
    // connectivity (it calls helper_one() — a CALLS edge).
    assert!(
        result.referenced_by.iter().any(|e| e.kind == "CALLS"
            && e.from == "m.rs::caller_two"
            && e.to == "m.rs::helper_one"),
        "referenced_by = {:?}",
        result.referenced_by
    );
    // And an external referrer must not be double-counted as intra-set connectivity.
    assert!(
        result.connectivity.is_empty(),
        "connectivity = {:?}",
        result.connectivity
    );
}

#[test]
fn ask_with_no_match_is_empty() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(dir.path(), "z.rs", "pub fn something() {}\n");
    idx.index_all();
    let result = idx.ask("zzz_nonexistent_query_xyz", 10);
    assert!(result.chunks.is_empty());
    assert!(result.connectivity.is_empty());
}

#[test]
fn rebuild_wipes_and_reindexes() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "r.rs",
        "pub fn aaa() { bbb(); }\npub fn bbb() {}\n",
    );
    idx.index_all();
    let before = idx.graph_stats().unwrap().nodes_total;
    assert!(before >= 3);

    let stats = idx.rebuild();
    assert!(stats.files >= 1);
    let after = idx.graph_stats().unwrap().nodes_total;
    // Rebuild reconstructs the same graph (not doubled, not empty).
    assert_eq!(
        before, after,
        "rebuild should reproduce the same node count"
    );
}

/// True if a node named `sym` exists in the graph (depth-0 impact resolves the
/// start node without needing any edges).
fn node_exists(idx: &Indexer, sym: &str) -> bool {
    idx.impact(sym, 0, true).0
}

fn edge_labels(idx: &Indexer) -> Vec<String> {
    idx.graph_stats()
        .unwrap()
        .edge_labels
        .into_iter()
        .map(|(l, _)| l)
        .collect()
}

#[test]
fn parse_generics_handles_bounds_and_lifetimes() {
    assert_eq!(
        parse_generics("fn foo<T: Display + Clone, U>(x: T) {"),
        vec![
            (
                "T".to_string(),
                vec!["Display".to_string(), "Clone".to_string()]
            ),
            ("U".to_string(), vec![]),
        ]
    );
    // Java-style extends bound.
    assert_eq!(
        parse_generics("class Box<T extends Comparable> {"),
        vec![("T".to_string(), vec!["Comparable".to_string()])]
    );
    // Nested generic in a bound stays one parameter.
    assert_eq!(
        parse_generics("impl<T: Into<String>> Foo for T {"),
        vec![("T".to_string(), vec!["Into".to_string()])]
    );
    // Lifetimes are skipped; no generics yields empty.
    assert_eq!(
        parse_generics("fn bar<'a, T>(x: &'a T) {"),
        vec![("T".to_string(), vec![])]
    );
    assert!(parse_generics("fn plain() {}").is_empty());
    // A `<` that is a comparison in the body is not mistaken for generics.
    assert!(parse_generics("fn cmp() { if a < b {} }").is_empty());
}

#[test]
fn rust_trait_impl_creates_satisfies_edges() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "area.rs",
        "pub trait Area {\n  fn area(&self) -> f64;\n}\n\
         pub struct Circle { r: f64 }\n\
         impl Area for Circle {\n  fn area(&self) -> f64 { 3.0 }\n}\n",
    );
    idx.index_all();
    // The trait method and impl method are both Method nodes, linked by SATISFIES.
    assert!(node_exists(&idx, "area"));
    assert!(
        edge_labels(&idx).contains(&"SATISFIES".to_string()),
        "labels = {:?}",
        edge_labels(&idx)
    );

    // The impl's area() Method points at the trait's area() Method.
    let result = idx.ask("area Circle", 20);
    let has_sat = result.connectivity.iter().any(|e| e.kind == "SATISFIES");
    assert!(has_sat, "connectivity = {:?}", result.connectivity);
}

#[test]
fn kotlin_interface_impl_creates_satisfies_edges() {
    // Method-level nodes (and thus SATISFIES) form for languages with a function
    // keyword. Kotlin (`fun`) is one; Java/C#/C++ methods have no leading keyword so
    // they are a known gap.
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "Shape.kt",
        "interface Shape {\n  fun areaOf(): Double\n}\n\
         class Square : Shape {\n  override fun areaOf(): Double { return 4.0 }\n}\n",
    );
    idx.index_all();
    assert!(
        edge_labels(&idx).contains(&"SATISFIES".to_string()),
        "labels = {:?}",
        edge_labels(&idx)
    );
}

#[test]
fn generics_create_typeparam_nodes_and_bound_by_edges() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "g.rs",
        "pub trait Render {}\n\
         pub fn draw<T: Render>(item: T) {}\n",
    );
    idx.index_all();
    let nl: Vec<String> = idx
        .graph_stats()
        .unwrap()
        .node_labels
        .into_iter()
        .map(|(l, _)| l)
        .collect();
    assert!(
        nl.contains(&"TypeParam".to_string()),
        "node labels = {:?}",
        nl
    );
    let el = edge_labels(&idx);
    assert!(
        el.contains(&"HAS_TYPE_PARAM".to_string()),
        "labels = {:?}",
        el
    );
    assert!(el.contains(&"BOUND_BY".to_string()), "labels = {:?}", el);
}

#[test]
fn satisfies_and_typeparam_are_pruned_when_file_changes() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "p.rs",
        "pub trait Render {}\n\
         pub fn draw<T: Render>(item: T) {}\n",
    );
    idx.index_all();
    assert!(edge_labels(&idx).contains(&"BOUND_BY".to_string()));

    // Remove the generic param; its TypeParam node must be pruned with the file.
    write(
        dir.path(),
        "p.rs",
        "pub trait Render {}\npub fn draw(item: i32) {}\n",
    );
    idx.index_all();
    let chunks: Vec<String> = idx
        .graph_chunks_for_file("p.rs")
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert!(
        !chunks.iter().any(|c| c.contains("::<T>")),
        "stale TypeParam survived: {:?}",
        chunks
    );
}

#[test]
fn reindex_prunes_a_removed_function_node() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "f.rs",
        "pub fn keep_me() {}\npub fn remove_me() {}\n",
    );
    idx.index_all();
    assert!(node_exists(&idx, "keep_me"));
    assert!(node_exists(&idx, "remove_me"));

    // Re-index with remove_me gone.
    write(dir.path(), "f.rs", "pub fn keep_me() {}\n");
    idx.index_all();
    assert!(node_exists(&idx, "keep_me"), "survivor must remain");
    assert!(
        !node_exists(&idx, "remove_me"),
        "deleted symbol's node must be pruned"
    );

    let ids: Vec<String> = idx
        .graph_chunks_for_file("f.rs")
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert!(ids.iter().any(|i| i == "f.rs::keep_me"));
    assert!(!ids.iter().any(|i| i == "f.rs::remove_me"));
}

#[test]
fn reindex_prunes_a_removed_method_and_keeps_siblings() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "svc.rs",
        "pub struct Svc;\nimpl Svc {\n  pub fn alpha_m(&self) {}\n  pub fn beta_m(&self) {}\n}\n",
    );
    idx.index_all();
    assert!(node_exists(&idx, "alpha_m"));
    assert!(node_exists(&idx, "beta_m"));

    write(
        dir.path(),
        "svc.rs",
        "pub struct Svc;\nimpl Svc {\n  pub fn alpha_m(&self) {}\n}\n",
    );
    idx.index_all();
    assert!(node_exists(&idx, "alpha_m"), "remaining method must stay");
    assert!(
        !node_exists(&idx, "beta_m"),
        "removed method node must be pruned"
    );
}

#[test]
fn reindex_renamed_class_prunes_old_node() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "Old.java",
        "public class OldName { void m() {} }\n",
    );
    idx.index_all();
    assert!(node_exists(&idx, "OldName"));

    write(
        dir.path(),
        "Old.java",
        "public class NewName { void m() {} }\n",
    );
    idx.index_all();
    assert!(node_exists(&idx, "NewName"));
    assert!(
        !node_exists(&idx, "OldName"),
        "renamed-away class must be pruned"
    );
}

#[test]
fn removing_a_parent_prunes_its_method_children() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "mod.rs",
        "pub struct Gone;\nimpl Gone {\n  pub fn child_method(&self) {}\n}\npub fn standalone() {}\n",
    );
    idx.index_all();
    assert!(node_exists(&idx, "child_method"));

    // Drop the whole impl/struct; the nested method must not be orphaned.
    write(dir.path(), "mod.rs", "pub fn standalone() {}\n");
    idx.index_all();
    assert!(node_exists(&idx, "standalone"));
    assert!(
        !node_exists(&idx, "child_method"),
        "orphaned method must be pruned"
    );
    assert!(!node_exists(&idx, "Gone"));
}

#[test]
fn reindex_drops_a_stale_reference_edge() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "call.rs",
        "pub fn callee_xyz() {}\npub fn caller_abc() { callee_xyz(); }\n",
    );
    idx.index_all();
    // caller_abc references callee_xyz.
    let callers: Vec<String> = idx
        .impact("callee_xyz", 1, true)
        .1
        .into_iter()
        .map(|h| h.name)
        .collect();
    assert!(
        callers.contains(&"caller_abc".to_string()),
        "callers = {:?}",
        callers
    );

    // Remove the call; both functions survive but the edge must disappear.
    write(
        dir.path(),
        "call.rs",
        "pub fn callee_xyz() {}\npub fn caller_abc() { let _x = 1; }\n",
    );
    idx.index_all();
    assert!(node_exists(&idx, "caller_abc") && node_exists(&idx, "callee_xyz"));
    let callers2: Vec<String> = idx
        .impact("callee_xyz", 1, true)
        .1
        .into_iter()
        .map(|h| h.name)
        .collect();
    assert!(
        !callers2.contains(&"caller_abc".to_string()),
        "stale edge survived: {:?}",
        callers2
    );
}

#[test]
fn deleting_a_file_prunes_its_file_and_chunk_nodes() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    let gone = write(dir.path(), "gone.rs", "pub fn doomed_fn() {}\n");
    write(dir.path(), "stay.rs", "pub fn survivor_fn() {}\n");
    idx.index_all();
    assert!(idx.graph_file_paths().contains("gone.rs"));
    assert!(node_exists(&idx, "doomed_fn"));

    fs::remove_file(&gone).unwrap();
    let stats = idx.index_all();
    assert_eq!(stats.removed, 1, "one file should be reported removed");
    assert!(
        !idx.graph_file_paths().contains("gone.rs"),
        "deleted File node must be pruned"
    );
    assert!(
        !node_exists(&idx, "doomed_fn"),
        "deleted file's chunk must be pruned"
    );
    assert!(idx.graph_file_paths().contains("stay.rs"));
    assert!(node_exists(&idx, "survivor_fn"));
    assert!(
        idx.outline("gone.rs").is_none(),
        "in-memory index must drop the file too"
    );
}

#[test]
fn deleting_referenced_file_cascades_incoming_edges() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(dir.path(), "lib.rs", "pub fn provided_fn() {}\n");
    let user = write(
        dir.path(),
        "user.rs",
        "pub fn consumer_fn() { provided_fn(); }\n",
    );
    idx.index_all();
    assert!(idx
        .impact("provided_fn", 1, true)
        .1
        .iter()
        .any(|h| h.name == "consumer_fn"));

    // Delete the consumer; the provider survives with no dangling incoming edge.
    fs::remove_file(&user).unwrap();
    idx.index_all();
    assert!(node_exists(&idx, "provided_fn"));
    assert!(!node_exists(&idx, "consumer_fn"));
    assert!(
        idx.impact("provided_fn", 1, true).1.is_empty(),
        "incoming edge from deleted file must be cascaded away"
    );
}

#[test]
fn unchanged_reindex_does_not_grow_or_duplicate_the_graph() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(
        dir.path(),
        "u.rs",
        "pub fn aa_fn() { bb_fn(); }\npub fn bb_fn() {}\n",
    );
    idx.index_all();
    let before = idx.graph_stats().unwrap();
    // Re-index with no changes on disk.
    idx.index_all();
    let after = idx.graph_stats().unwrap();
    assert_eq!(
        before.nodes_total, after.nodes_total,
        "node count must be stable"
    );
    assert_eq!(
        before.edges_total, after.edges_total,
        "edge count must be stable"
    );
}

#[test]
fn cross_session_change_prunes_stale_node_via_wal_hash() {
    let dir = tempdir().unwrap();
    // Session 1: index a file with two symbols, then drop the Indexer.
    {
        let mut idx = Indexer::new(dir.path().to_path_buf());
        write(dir.path(), "s.rs", "pub fn old_a() {}\npub fn old_b() {}\n");
        idx.index_all();
    }
    // Session 2: a fresh Indexer replays the WAL (in-memory files map starts empty),
    // the file changed on disk -> the File-node content_hash mismatch must trigger
    // pruning of the now-absent symbol.
    let mut idx2 = Indexer::new(dir.path().to_path_buf());
    write(dir.path(), "s.rs", "pub fn old_a() {}\n");
    idx2.index_all();
    assert!(node_exists(&idx2, "old_a"));
    assert!(
        !node_exists(&idx2, "old_b"),
        "cross-session stale node must be pruned"
    );
}

#[test]
fn unchanged_file_across_session_keeps_nodes() {
    let dir = tempdir().unwrap();
    {
        let mut idx = Indexer::new(dir.path().to_path_buf());
        write(dir.path(), "k.rs", "pub fn stable_fn() {}\n");
        idx.index_all();
    }
    let mut idx2 = Indexer::new(dir.path().to_path_buf());
    // No change on disk; re-index must keep the node (hash short-circuit).
    idx2.index_all();
    assert!(node_exists(&idx2, "stable_fn"));
}

#[test]
fn directory_nodes_form_a_relative_path_tree() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(dir.path(), "src/util/math.rs", "pub fn add() {}\n");
    write(dir.path(), "top.rs", "pub fn t() {}\n");
    idx.index_all();

    let stats = idx.graph_stats().unwrap();
    let nl: Vec<&str> = stats.node_labels.iter().map(|(l, _)| l.as_str()).collect();
    let el: Vec<&str> = stats.edge_labels.iter().map(|(l, _)| l.as_str()).collect();
    assert!(nl.contains(&"Directory"), "node labels = {:?}", nl);
    assert!(el.contains(&"CONTAINS"), "edge labels = {:?}", el);

    // Directory nodes are addressed by their project-relative path (root = ".").
    let dirs = idx.graph_dir_paths();
    assert!(dirs.contains("."), "dirs = {:?}", dirs); // root directory
    assert!(dirs.contains("src"), "dirs = {:?}", dirs);
    assert!(dirs.contains("src/util"), "dirs = {:?}", dirs);
    // Directory paths are relative to the project root, not absolute.
    assert!(
        dirs.iter().all(|p| !p.contains(':') && !p.starts_with('/')),
        "dir paths must be relative: {:?}",
        dirs
    );
    // The File node's path is relative to the project root, not absolute.
    assert!(idx.graph_file_paths().contains("src/util/math.rs"));
    assert!(
        idx.graph_file_paths()
            .iter()
            .all(|p| !p.contains(':') && !p.starts_with('/')),
        "file paths must be relative: {:?}",
        idx.graph_file_paths()
    );
    // graph_file_paths is File-only: directory paths must not leak in.
    assert!(!idx.graph_file_paths().contains("src"));
    assert!(!idx.graph_file_paths().contains("."));
}

#[test]
fn directory_tree_is_stable_across_unchanged_reindex() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(dir.path(), "src/a.rs", "pub fn a() {}\n");
    idx.index_all();
    let before = idx.graph_stats().unwrap();
    idx.index_all();
    let after = idx.graph_stats().unwrap();
    // Directory/CONTAINS merges are idempotent — no growth or duplication.
    assert_eq!(before.nodes_total, after.nodes_total);
    assert_eq!(before.edges_total, after.edges_total);
}

#[test]
fn config_and_project_files_are_indexed_as_whole_file_chunks() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    // A repo can hold many manifests; each is indexed (one chunk) and searchable.
    index_one(
        &mut idx,
        dir.path(),
        "Cargo.toml",
        "[package]\nname = \"demo_pkg\"\nversion = \"0.1.0\"\n",
    );
    index_one(&mut idx, dir.path(), "App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>demo_pkg</PropertyGroup>\n</Project>\n");

    let toml = idx.outline("Cargo.toml").unwrap();
    assert_eq!(toml.language, "Config");
    assert_eq!(toml.chunks.len(), 1);
    let csproj = idx.outline("App.csproj").unwrap();
    assert_eq!(csproj.language, "Config");

    // They participate in search like any other chunk (content is retrievable).
    let hits = idx.search("demo_pkg", 8);
    assert!(hits.iter().any(|c| c.path == "Cargo.toml"));
    assert!(hits.iter().any(|c| c.path == "App.csproj"));
}

#[test]
fn impact_on_isolated_symbol_is_found_but_empty() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    write(dir.path(), "iso.rs", "pub fn lonely_func() {}\n");
    idx.index_all();
    let (found, hits) = idx.impact("lonely_func", 2, true);
    assert!(found);
    assert!(hits.is_empty());
}

#[test]
fn removing_a_file_prunes_it_from_the_graph_and_index() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    let a = write(dir.path(), "a.rs", "pub fn alpha_fn() {}\n");
    write(dir.path(), "b.rs", "pub fn beta_fn() {}\n");
    idx.index_all();
    assert!(idx.graph_file_paths().contains("a.rs"));
    assert!(idx.graph_file_paths().contains("b.rs"));

    // remove_path drops the File node + its chunks from the graph and the in-memory
    // index (the agent's delete_file uses this to stay consistent with disk).
    fs::remove_file(&a).unwrap();
    idx.remove_path(&a);

    assert!(
        !idx.graph_file_paths().contains("a.rs"),
        "deleted file should be pruned"
    );
    assert!(
        idx.graph_file_paths().contains("b.rs"),
        "sibling file should remain"
    );
    assert!(idx.outline("a.rs").is_none());
}

#[test]
fn html_and_css_are_indexed_as_whole_file_markup_chunks() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "index.html",
        "<!doctype html>\n<html><body><h1>widget dashboard</h1></body></html>\n",
    );
    index_one(
        &mut idx,
        dir.path(),
        "styles.css",
        ".widget { color: red; }\n",
    );

    let html = idx.outline("index.html").unwrap();
    assert_eq!(html.language, "Markup");
    assert_eq!(html.chunks.len(), 1, "html is one whole-file chunk");
    let css = idx.outline("styles.css").unwrap();
    assert_eq!(css.language, "Markup");

    // Markup content is retrievable via search like any other chunk.
    let hits = idx.search("widget", 8);
    assert!(hits.iter().any(|c| c.path == "index.html"));
}

#[test]
fn project_discrepancies_reports_added_changed_and_deleted() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let mut idx = Indexer::new(root.to_path_buf());

    // Initial project, mirrored into the graph.
    write(root, "src/keep.rs", "fn keep() {}\n");
    write(root, "src/edit.rs", "fn before() {}\n");
    write(root, "src/gone.rs", "fn gone() {}\n");
    idx.index_all();

    // A freshly-loaded graph that matches disk reports no drift.
    assert!(
        idx.project_discrepancies().is_empty(),
        "in-sync project should report no discrepancies"
    );

    // Mutate the working tree without re-indexing: add, change, delete.
    write(root, "src/added.rs", "fn added() {}\n");
    write(root, "src/edit.rs", "fn before() {}\nfn after() {}\n");
    fs::remove_file(root.join("src/gone.rs")).unwrap();

    let d = idx.project_discrepancies();
    assert_eq!(d.added, vec!["src/added.rs".to_string()]);
    assert_eq!(d.changed, vec!["src/edit.rs".to_string()]);
    assert_eq!(d.deleted, vec!["src/gone.rs".to_string()]);
    assert_eq!(d.total(), 3);
    assert!(!d.is_empty());

    // The check is read-only: it must not have mutated the graph.
    let again = idx.project_discrepancies();
    assert_eq!(
        again.total(),
        3,
        "project_discrepancies must not mutate state"
    );

    // Re-indexing reconciles everything back to in-sync.
    idx.index_all();
    assert!(idx.project_discrepancies().is_empty());
}
