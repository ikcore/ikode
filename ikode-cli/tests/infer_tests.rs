//! Tests for the inference layer: embedding-based semantic search and chunk /
//! architecture summarisation. Uses a deterministic fake `GaiseClient` so the
//! retrieval/storage logic is exercised without any network or real model.

use async_trait::async_trait;
use gaise_core::contracts::{
    GaiseContent, GaiseEmbeddingsRequest, GaiseEmbeddingsResponse, GaiseInstructRequest,
    GaiseInstructResponse, GaiseInstructStreamResponse, GaiseMessage, OneOrMany,
};
use gaise_core::GaiseClient;
use ikode::index::{Indexer, SummarizeOptions};
use ikode::lang;
use std::error::Error;
use std::path::Path;
use std::pin::Pin;
use tempfile::tempdir;

/// Fixed vocabulary; an embedding is the per-word occurrence count, so cosine
/// similarity rewards shared vocabulary — enough to assert ranking deterministically.
const VOCAB: &[&str] = &[
    "alpha", "beta", "gamma", "render", "compute", "parse", "widget",
];

fn fake_embed(text: &str) -> Vec<f32> {
    let lower = text.to_lowercase();
    VOCAB
        .iter()
        .map(|w| lower.matches(w).count() as f32)
        .collect()
}

struct FakeClient {
    summary: String,
}

#[async_trait]
impl GaiseClient for FakeClient {
    async fn instruct_stream(
        &self,
        _request: &GaiseInstructRequest,
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
        Err("instruct_stream is not exercised in these tests".into())
    }

    async fn instruct(
        &self,
        _request: &GaiseInstructRequest,
    ) -> Result<GaiseInstructResponse, Box<dyn Error + Send + Sync>> {
        Ok(GaiseInstructResponse {
            output: OneOrMany::One(GaiseMessage {
                role: "assistant".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text {
                    text: self.summary.clone(),
                })),
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
            }),
            external_id: None,
            usage: None,
        })
    }

    async fn embeddings(
        &self,
        request: &GaiseEmbeddingsRequest,
    ) -> Result<GaiseEmbeddingsResponse, Box<dyn Error + Send + Sync>> {
        let inputs: Vec<String> = match &request.input {
            OneOrMany::One(s) => vec![s.clone()],
            OneOrMany::Many(v) => v.clone(),
        };
        Ok(GaiseEmbeddingsResponse {
            external_id: None,
            output: inputs.iter().map(|s| fake_embed(s)).collect(),
            usage: None,
        })
    }
}

fn fake() -> FakeClient {
    FakeClient {
        summary: "This is a one-line summary.".to_string(),
    }
}

/// Like `FakeClient` but records the input size of every embeddings request, so a
/// test can assert the embed pass batches rather than sending one giant request.
struct CountingClient {
    batches: std::sync::Mutex<Vec<usize>>,
}

impl CountingClient {
    fn new() -> Self {
        CountingClient {
            batches: std::sync::Mutex::new(Vec::new()),
        }
    }
    fn batch_sizes(&self) -> Vec<usize> {
        self.batches.lock().unwrap().clone()
    }
}

#[async_trait]
impl GaiseClient for CountingClient {
    async fn instruct_stream(
        &self,
        _request: &GaiseInstructRequest,
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
        Err("instruct_stream is not exercised in these tests".into())
    }

    async fn instruct(
        &self,
        _request: &GaiseInstructRequest,
    ) -> Result<GaiseInstructResponse, Box<dyn Error + Send + Sync>> {
        Err("instruct is not exercised in these tests".into())
    }

    async fn embeddings(
        &self,
        request: &GaiseEmbeddingsRequest,
    ) -> Result<GaiseEmbeddingsResponse, Box<dyn Error + Send + Sync>> {
        let inputs: Vec<String> = match &request.input {
            OneOrMany::One(s) => vec![s.clone()],
            OneOrMany::Many(v) => v.clone(),
        };
        self.batches.lock().unwrap().push(inputs.len());
        Ok(GaiseEmbeddingsResponse {
            external_id: None,
            output: inputs.iter().map(|s| fake_embed(s)).collect(),
            usage: None,
        })
    }
}

fn index_one(idx: &mut Indexer, root: &Path, rel: &str, content: &str) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&p, content).unwrap();
    let spec = lang::detect_language(&p).unwrap();
    idx.index_file(&p, &spec).unwrap();
}

#[tokio::test]
async fn embedding_batches_requests_and_honours_hash_gate() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    // 20 small functions -> 20 chunks, comfortably more than EMBED_BATCH (8), so the
    // embed pass must split into several requests instead of one oversized one.
    let mut src = String::new();
    for i in 0..20 {
        src.push_str(&format!("pub fn f{i}() {{ render(); }}\n"));
    }
    index_one(&mut idx, dir.path(), "many.rs", &src);

    let client = CountingClient::new();
    let stats = idx.embed_index(&client, "fake::embed").await.unwrap();
    let n = stats.embedded;
    assert!(n >= 20, "expected >=20 chunks embedded, got {n}");

    let sizes = client.batch_sizes();
    assert!(sizes.len() >= 3, "expected several batches, got {sizes:?}");
    assert!(
        sizes.iter().all(|&s| s <= 8),
        "a batch exceeded EMBED_BATCH: {sizes:?}"
    );
    assert_eq!(
        sizes.iter().sum::<usize>(),
        n,
        "batches must cover every chunk exactly once"
    );

    // A clean re-run is fully hash-gated: nothing re-embedded, no new requests issued.
    let again = idx.embed_index(&client, "fake::embed").await.unwrap();
    assert_eq!(again.embedded, 0);
    assert_eq!(again.skipped, n);
    assert_eq!(
        client.batch_sizes().len(),
        sizes.len(),
        "a clean re-run must issue no requests"
    );
}

#[tokio::test]
async fn embedding_enables_semantic_search_ranking() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "a.rs",
        "pub fn render_widget() { let widget = 1; }\n",
    );
    index_one(
        &mut idx,
        dir.path(),
        "b.rs",
        "pub fn parse_input() { compute(); }\n",
    );

    // No embeddings yet -> semantic search is unavailable.
    assert!(!idx.has_embeddings());

    let client = fake();
    let stats = idx.embed_index(&client, "fake::embed").await.unwrap();
    assert_eq!(stats.embedded, 2);
    assert!(idx.has_embeddings());

    // A query about rendering ranks the render chunk first.
    let result = idx
        .ask_semantic(&client, "fake::embed", "render widget", 5)
        .await
        .unwrap();
    assert!(!result.chunks.is_empty());
    assert_eq!(
        result.chunks[0].name,
        "render_widget",
        "chunks = {:?}",
        result.chunks.iter().map(|c| &c.name).collect::<Vec<_>>()
    );

    // A query about parsing ranks the parse chunk first.
    let result2 = idx
        .ask_semantic(&client, "fake::embed", "parse compute", 5)
        .await
        .unwrap();
    assert_eq!(result2.chunks[0].name, "parse_input");
}

#[tokio::test]
async fn embedding_is_incremental_and_skips_unchanged() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "a.rs",
        "pub fn alpha_fn() { compute(); }\n",
    );
    let client = fake();

    let first = idx.embed_index(&client, "fake::embed").await.unwrap();
    assert_eq!(first.embedded, 1);
    assert_eq!(first.skipped, 0);

    // Re-running with no source change embeds nothing.
    let second = idx.embed_index(&client, "fake::embed").await.unwrap();
    assert_eq!(second.embedded, 0);
    assert_eq!(second.skipped, 1);
}

#[tokio::test]
async fn semantic_search_falls_back_gracefully_when_unembedded() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(&mut idx, dir.path(), "a.rs", "pub fn alpha_fn() {}\n");
    // No embed_index call -> semantic_search returns nothing (caller falls back).
    let hits = idx.semantic_search(&fake_embed("alpha"), 5);
    assert!(hits.is_empty());
    assert!(!idx.has_embeddings());
}

#[tokio::test]
async fn summaries_are_generated_stored_and_surfaced_in_ask() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "a.rs",
        "pub fn render_widget() { let widget = 1; }\n",
    );
    let client = fake();

    let stats = idx.summarize_index(&client, "fake::chat", 0).await.unwrap();
    assert_eq!(stats.summarized, 1);

    // The stored summary surfaces through a keyword ask.
    let result = idx.ask("render_widget", 5);
    let chunk = result
        .chunks
        .iter()
        .find(|c| c.name == "render_widget")
        .unwrap();
    assert_eq!(
        chunk.summary.as_deref(),
        Some("This is a one-line summary.")
    );

    // Re-running skips the already-summarised chunk.
    let again = idx.summarize_index(&client, "fake::chat", 0).await.unwrap();
    assert_eq!(again.summarized, 0);
    assert_eq!(again.skipped, 1);
}

#[tokio::test]
async fn summarize_respects_the_call_cap() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "a.rs",
        "pub fn one_fn() {}\npub fn two_fn() {}\npub fn three_fn() {}\n",
    );
    let client = fake();
    // Cap at 2 even though three chunks need summaries.
    let stats = idx.summarize_index(&client, "fake::chat", 2).await.unwrap();
    assert_eq!(stats.summarized, 2);
}

#[tokio::test]
async fn default_policy_skips_tests_until_opted_in() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "a.rs",
        "pub fn render_widget() { let w = 1; compute(w); }\n",
    );
    // A `test_*` function is a test chunk (by name), so the default policy skips it.
    index_one(
        &mut idx,
        dir.path(),
        "t.rs",
        "fn test_renders() { let a = render(); use_it(a); }\n",
    );
    let client = fake();

    // include_tests=false: only the non-test function is summarised.
    let stats = idx
        .summarize_index_with(
            &client,
            "fake::chat",
            0,
            SummarizeOptions::default(),
            &mut |_, _| {},
        )
        .await
        .unwrap();
    assert_eq!(
        stats.summarized, 1,
        "the test chunk should be skipped by default"
    );
    assert!(stats.skipped >= 1);

    // Opting tests in summarises the now-only-remaining test chunk.
    let opts_all = SummarizeOptions {
        include_tests: true,
        ..SummarizeOptions::default()
    };
    let again = idx
        .summarize_index_with(&client, "fake::chat", 0, opts_all, &mut |_, _| {})
        .await
        .unwrap();
    assert_eq!(
        again.summarized, 1,
        "opting in should summarise the test chunk"
    );
}

#[tokio::test]
async fn trivial_filter_skips_small_structs_but_keeps_small_functions() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    // A 3-line arg struct (non-callable, under the 4-line threshold) and a 1-line fn.
    index_one(
        &mut idx,
        dir.path(),
        "a.rs",
        "pub struct Args {\n    pub path: String,\n}\npub fn helper() { do_thing(); }\n",
    );
    let client = fake();

    let stats = idx
        .summarize_index_with(
            &client,
            "fake::chat",
            0,
            SummarizeOptions::default(),
            &mut |_, _| {},
        )
        .await
        .unwrap();
    // The struct is skipped as trivial; the tiny function is kept (callables are exempt).
    assert_eq!(
        stats.summarized, 1,
        "small function should still be summarised"
    );
    assert!(idx
        .ask("helper", 5)
        .chunks
        .iter()
        .find(|c| c.name == "helper")
        .unwrap()
        .summary
        .is_some());
    assert!(idx
        .ask("Args", 5)
        .chunks
        .iter()
        .find(|c| c.name == "Args")
        .unwrap()
        .summary
        .is_none());
}

#[tokio::test]
async fn changing_one_chunk_resummarises_only_that_chunk() {
    // Editing one function in a file must NOT resummarise the whole file: summary_hash
    // is per-chunk (kind+name+path+body), and re-indexing upserts node props (preserving
    // each chunk's stored summary_hash), so an unchanged sibling still hash-matches.
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "a.rs",
        "pub fn alpha() { one(); }\npub fn beta() { two(); }\n",
    );
    let client = fake();
    let s1 = idx.summarize_index(&client, "fake::chat", 0).await.unwrap();
    assert_eq!(s1.summarized, 2);

    // Change ONLY beta's body; alpha stays byte-identical.
    let p = dir.path().join("a.rs");
    std::fs::write(
        &p,
        "pub fn alpha() { one(); }\npub fn beta() { two(); three(); }\n",
    )
    .unwrap();
    let spec = lang::detect_language(&p).unwrap();
    idx.index_file(&p, &spec).unwrap();

    let s2 = idx.summarize_index(&client, "fake::chat", 0).await.unwrap();
    assert_eq!(
        s2.summarized, 1,
        "only the changed chunk should resummarise"
    );
    assert_eq!(
        s2.skipped, 1,
        "the unchanged sibling should be skipped via its hash"
    );
}

#[tokio::test]
async fn summaries_run_concurrently_and_cover_every_chunk() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "a.rs",
        "pub fn one_fn() { a(); }\npub fn two_fn() { b(); }\npub fn three_fn() { c(); }\npub fn four_fn() { d(); }\n",
    );
    let client = fake();

    // Four chunks summarised with four calls in flight — buffer_unordered must drain all.
    let opts = SummarizeOptions {
        include_tests: true,
        min_body_lines: 0,
        concurrency: 4,
    };
    let stats = idx
        .summarize_index_with(&client, "fake::chat", 0, opts, &mut |_, _| {})
        .await
        .unwrap();
    assert_eq!(stats.summarized, 4);
}

#[tokio::test]
async fn config_files_are_embedded_and_summarised_like_code() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "Cargo.toml",
        "[package]\nname = \"alpha\"\n",
    );
    let client = fake();

    // A config manifest is a chunk too, so it is embedded...
    let estats = idx.embed_index(&client, "fake::embed").await.unwrap();
    assert_eq!(estats.embedded, 1);
    assert!(idx.has_embeddings());

    // ...and summarised.
    let sstats = idx.summarize_index(&client, "fake::chat", 0).await.unwrap();
    assert_eq!(sstats.summarized, 1);
    let result = idx.ask("alpha", 5);
    assert!(result
        .chunks
        .iter()
        .any(|c| c.path == "Cargo.toml" && c.summary.is_some()));
}

#[tokio::test]
async fn ikignore_prunes_matching_files_before_summary_and_embedding_calls() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());

    index_one(
        &mut idx,
        dir.path(),
        "keep.rs",
        "pub fn keep_for_enrichment() {}\n",
    );
    index_one(
        &mut idx,
        dir.path(),
        "relative/exact.rs",
        "pub fn skip_relative() {}\n",
    );
    index_one(
        &mut idx,
        dir.path(),
        "nested/waste.rs",
        "pub fn skip_by_filename() {}\n",
    );
    index_one(
        &mut idx,
        dir.path(),
        "generated/deep/skip-1.rs",
        "pub fn skip_by_wildcard() {}\n",
    );
    index_one(
        &mut idx,
        dir.path(),
        "absolute/skip.rs",
        "pub fn skip_by_absolute_path() {}\n",
    );

    let absolute = dir
        .path()
        .join("absolute")
        .join("skip.rs")
        .to_string_lossy()
        .replace('\\', "/");
    std::fs::write(
        dir.path().join(".ikignore"),
        format!(
            "# basename, relative path, wildcard, and full path\n\
             waste.rs\n\
             relative/exact.rs\n\
             generated/**/skip-?.rs\n\
             {absolute}\n"
        ),
    )
    .unwrap();

    let indexed = idx.index_all();
    assert_eq!(indexed.ignored, 4);
    assert_eq!(indexed.removed, 4);
    assert_eq!(indexed.ikignore_files, vec![".ikignore"]);
    assert!(idx.outline("relative/exact.rs").is_none());
    assert!(idx.outline("nested/waste.rs").is_none());
    assert!(idx.outline("generated/deep/skip-1.rs").is_none());
    assert!(idx.outline("absolute/skip.rs").is_none());

    let client = fake();
    let summaries = idx.summarize_index(&client, "fake::chat", 0).await.unwrap();
    assert_eq!(summaries.summarized, 1);
    assert_eq!(summaries.ignored, 0);

    let embeddings = idx.embed_index(&client, "fake::embed").await.unwrap();
    assert_eq!(embeddings.embedded, 1);
    assert_eq!(embeddings.ignored, 0);
}

#[tokio::test]
async fn embeddings_and_summaries_persist_across_a_reload() {
    // #3 persistence: the graph (including embedding/summary props) is journalled to
    // the WAL under .ikode/, so a fresh Indexer on the same root replays them.
    let dir = tempdir().unwrap();
    {
        let mut idx = Indexer::new(dir.path().to_path_buf());
        index_one(
            &mut idx,
            dir.path(),
            "a.rs",
            "pub fn render_widget() { let widget = 1; }\n",
        );
        let client = fake();
        // Summarise *before* embedding: the embedding document folds in the stored
        // summary, so doing it in this order (as `/enrich` does) makes the document —
        // and thus the embed_hash — stable across the reload below.
        assert_eq!(
            idx.summarize_index(&client, "fake::chat", 0)
                .await
                .unwrap()
                .summarized,
            1
        );
        assert_eq!(
            idx.embed_index(&client, "fake::embed")
                .await
                .unwrap()
                .embedded,
            1
        );
    } // idx dropped -> WAL file handle released

    // A brand-new Indexer on the same root replays the journal.
    let mut reloaded = Indexer::new(dir.path().to_path_buf());
    reloaded.index_all(); // repopulate in-memory chunk bodies from disk
    assert!(
        reloaded.has_embeddings(),
        "embeddings should survive a reload"
    );

    let client = fake();
    // Nothing needs re-embedding or re-summarising — the stored hashes matched.
    let estats = reloaded.embed_index(&client, "fake::embed").await.unwrap();
    assert_eq!(estats.embedded, 0);
    assert_eq!(estats.skipped, 1);
    let sstats = reloaded
        .summarize_index(&client, "fake::chat", 0)
        .await
        .unwrap();
    assert_eq!(sstats.summarized, 0);
    assert_eq!(sstats.skipped, 1);

    // Semantic search still works against the replayed vectors.
    let result = reloaded
        .ask_semantic(&client, "fake::embed", "render widget", 5)
        .await
        .unwrap();
    assert_eq!(result.chunks[0].name, "render_widget");
    // And the persisted summary surfaces.
    assert_eq!(
        result.chunks[0].summary.as_deref(),
        Some("This is a one-line summary.")
    );
}

#[tokio::test]
async fn relationship_summaries_are_stored_and_survive_relink() {
    // #2 per-edge inference: describe a CALLS edge (caller() invokes callee()) and
    // confirm the description (a) lands on the edge, (b) is incremental, and
    // (c) survives a re-link — the property that depends on link_references not
    // re-Merging existing edges.
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "a.rs",
        "pub fn caller() { callee(); }\npub fn callee() {}\n",
    );
    idx.link_references();
    let client = fake();

    let stats = idx
        .summarize_relationships(&client, "fake::chat", "CALLS", 0)
        .await
        .unwrap();
    assert_eq!(
        stats.summarized, 1,
        "the one caller->callee edge should be described"
    );
    assert_eq!(
        idx.relationship_summary("a.rs::caller", "a.rs::callee", "CALLS")
            .as_deref(),
        Some("This is a one-line summary.")
    );

    // Re-running describes nothing new (endpoints unchanged).
    let again = idx
        .summarize_relationships(&client, "fake::chat", "CALLS", 0)
        .await
        .unwrap();
    assert_eq!(again.summarized, 0);
    assert_eq!(again.skipped, 1);

    // A re-link (as a full /index would do) must NOT wipe the stored edge summary.
    idx.link_references();
    assert_eq!(
        idx.relationship_summary("a.rs::caller", "a.rs::callee", "CALLS")
            .as_deref(),
        Some("This is a one-line summary."),
        "re-linking should preserve the relationship summary on the existing edge"
    );
}

#[tokio::test]
async fn directory_rollups_are_generated_stored_and_incremental() {
    // #5 directory rollups: each Directory node gets a one-line summary composed from
    // its descendant chunks; root "." and nested dirs are all covered.
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(&mut idx, dir.path(), "src/a.rs", "pub fn alpha_fn() {}\n");
    index_one(
        &mut idx,
        dir.path(),
        "src/util/b.rs",
        "pub fn beta_fn() {}\n",
    );
    let client = fake();
    // Compose from real chunk summaries.
    idx.summarize_index(&client, "fake::chat", 0).await.unwrap();

    let stats = idx
        .summarize_directories(&client, "fake::chat", 0)
        .await
        .unwrap();
    assert_eq!(stats.summarized, 3, "., src, src/util");
    assert_eq!(
        idx.directory_summary(".").as_deref(),
        Some("This is a one-line summary.")
    );
    assert_eq!(
        idx.directory_summary("src").as_deref(),
        Some("This is a one-line summary.")
    );
    assert_eq!(
        idx.directory_summary("src/util").as_deref(),
        Some("This is a one-line summary.")
    );

    // Re-running with no structural change summarises nothing.
    let again = idx
        .summarize_directories(&client, "fake::chat", 0)
        .await
        .unwrap();
    assert_eq!(again.summarized, 0);
    assert_eq!(again.skipped, 3);
}

#[tokio::test]
async fn ask_output_surfaces_chunk_and_edge_summaries() {
    // #4 surfacing: the rendered /ask output shows the stored chunk summary and a
    // per-edge relationship description.
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "a.rs",
        "pub fn caller() { callee(); }\npub fn callee() {}\n",
    );
    idx.link_references();
    let client = fake();
    idx.summarize_index(&client, "fake::chat", 0).await.unwrap();
    idx.summarize_relationships(&client, "fake::chat", "CALLS", 0)
        .await
        .unwrap();

    let result = idx.ask("caller callee", 5);
    let rendered = ikode::harness::render_ask_result("caller callee", &result);
    // Chunk summary line and edge summary line both use the "⤷" lead-in.
    assert!(
        rendered.contains("⤷ This is a one-line summary."),
        "rendered = {rendered}"
    );
    // The connectivity section shows the CALLS edge (caller invokes callee()).
    assert!(rendered.contains("CALLS"), "rendered = {rendered}");
}

#[tokio::test]
async fn switching_embedding_model_forces_a_reembed() {
    // B1: the skip decision must factor in the embedding model, not just the body —
    // vectors from a different model aren't comparable, so a model switch re-embeds.
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "a.rs",
        "pub fn alpha_fn() { compute(); }\n",
    );
    let client = fake();

    assert_eq!(
        idx.embed_index(&client, "model-a").await.unwrap().embedded,
        1
    );
    // Same model, unchanged body -> skipped.
    assert_eq!(
        idx.embed_index(&client, "model-a").await.unwrap().skipped,
        1
    );
    // Different model -> must re-embed even though the body is identical.
    let switched = idx.embed_index(&client, "model-b").await.unwrap();
    assert_eq!(switched.embedded, 1);
    assert_eq!(switched.skipped, 0);
}

#[tokio::test]
async fn semantic_search_skips_dimension_mismatched_vectors() {
    // B1: a query of a different dimension than the stored vectors (e.g. mid model
    // switch) must be ignored, not scored against an incompatible space.
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(&mut idx, dir.path(), "a.rs", "pub fn render_widget() {}\n");
    let client = fake();
    idx.embed_index(&client, "fake::embed").await.unwrap();

    // Stored vectors are VOCAB-length; a 3-d query matches nothing.
    assert!(idx.semantic_search(&[1.0, 0.0, 0.0], 5).is_empty());
    // A correctly-sized query still ranks.
    assert!(!idx
        .semantic_search(&fake_embed("render widget"), 5)
        .is_empty());
}

#[tokio::test]
async fn ask_auto_falls_back_to_keyword_then_uses_semantic() {
    // A5: ask_auto is keyword until an embedding index exists, then semantic.
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "a.rs",
        "pub fn render_widget() { let widget = 1; }\n",
    );
    index_one(
        &mut idx,
        dir.path(),
        "b.rs",
        "pub fn parse_input() { compute(); }\n",
    );
    let client = fake();

    // No embeddings -> keyword (matches by name).
    let kw = idx
        .ask_auto(&client, "fake::embed", "render_widget", 5)
        .await;
    assert!(kw.chunks.iter().any(|c| c.name == "render_widget"));

    // After embedding -> semantic ranking puts the render chunk first.
    idx.embed_index(&client, "fake::embed").await.unwrap();
    let sem = idx
        .ask_auto(&client, "fake::embed", "render widget", 5)
        .await;
    assert_eq!(sem.chunks[0].name, "render_widget");
}

#[tokio::test]
async fn relationship_summaries_respect_the_call_cap() {
    // B2: three callers invoke one target (three CALLS edges); a cap of 2
    // describes only two.
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "a.rs",
        "pub fn alpha() { target(); }\npub fn beta() { target(); }\n\
         pub fn gamma() { target(); }\npub fn target() {}\n",
    );
    idx.link_references();
    let client = fake();
    let stats = idx
        .summarize_relationships(&client, "fake::chat", "CALLS", 2)
        .await
        .unwrap();
    assert_eq!(stats.summarized, 2);
}

#[tokio::test]
async fn architecture_summary_returns_prose() {
    let dir = tempdir().unwrap();
    let mut idx = Indexer::new(dir.path().to_path_buf());
    index_one(
        &mut idx,
        dir.path(),
        "a.rs",
        "pub fn caller() { callee(); }\npub fn callee() {}\n",
    );
    idx.link_references();
    let client = fake();
    let text = idx
        .summarize_architecture(&client, "fake::chat")
        .await
        .unwrap();
    assert_eq!(text, "This is a one-line summary.");
}
