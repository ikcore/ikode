//! `think` mapping per model family through the canonical reasoning vocabulary.

use gaise_core::contracts::GaiseGenerationConfig;
use gaise_provider_ollama::contracts::models::OllamaThink;
use gaise_provider_ollama::ollama_client::ollama_think;

fn think(model: &str, effort: Option<&str>, tokens: Option<usize>) -> Option<serde_json::Value> {
    let config = GaiseGenerationConfig {
        thinking_effort: effort.map(str::to_string),
        thinking_tokens: tokens,
        ..Default::default()
    };
    ollama_think(model, &config).map(|t: OllamaThink| serde_json::to_value(t).unwrap())
}

#[test]
fn gpt_oss_takes_levels_and_other_models_take_booleans() {
    assert_eq!(think("gpt-oss:20b", Some("low"), None), Some("low".into()));
    assert_eq!(
        think("gpt-oss:20b", Some("minimal"), None),
        Some("low".into())
    );
    assert_eq!(
        think("gpt-oss:20b", Some("xhigh"), None),
        Some("high".into())
    );
    assert_eq!(
        think("gpt-oss:20b", Some("ultra"), None),
        Some("high".into())
    );
    assert_eq!(
        think("gpt-oss:20b", Some("maximum"), None),
        Some("high".into())
    );
    assert_eq!(think("gpt-oss:20b", Some("none"), None), Some(false.into()));
    assert_eq!(think("gpt-oss:20b", Some("auto"), None), Some(true.into()));
    assert_eq!(
        think("gpt-oss:20b", Some("deep"), None),
        Some("deep".into()),
        "custom forwarded"
    );
    assert_eq!(
        think("gpt-oss:20b", None, Some(2048)),
        Some("medium".into())
    );
    assert_eq!(think("gpt-oss:20b", None, Some(0)), Some(false.into()));

    assert_eq!(think("qwen3:8b", Some("high"), None), Some(true.into()));
    assert_eq!(think("qwen3:8b", Some("off"), None), Some(false.into()));
    assert_eq!(think("qwen3:8b", Some("auto"), None), Some(true.into()));
    assert_eq!(think("qwen3:8b", None, Some(512)), Some(true.into()));
    assert_eq!(think("qwen3:8b", None, None), None);
}

#[test]
fn embedding_requests_apply_family_prefixes_and_dimension_rules() {
    use gaise_core::contracts::{GaiseEmbeddingTask, GaiseEmbeddingsRequest, OneOrMany};
    use gaise_provider_ollama::ollama_client::ollama_embed_request;
    let req =
        |model: &str, task: Option<GaiseEmbeddingTask>, dims: Option<u32>| GaiseEmbeddingsRequest {
            model: model.into(),
            input: OneOrMany::Many(vec!["alpha".into(), "beta".into()]),
            task,
            dimensions: dims,
            ..Default::default()
        };
    let body = |r: &GaiseEmbeddingsRequest| {
        let (wire, resolved) = ollama_embed_request(r);
        (serde_json::to_value(wire).unwrap(), resolved)
    };

    // EmbeddingGemma: Google instruction convention, discrete Matryoshka sizes.
    let (json, resolved) = body(&req(
        "embeddinggemma:latest",
        Some(GaiseEmbeddingTask::Query),
        Some(300),
    ));
    assert_eq!(json["input"][0], "task: search result | query: alpha");
    assert_eq!(json["dimensions"], 256);
    assert_eq!(json["truncate"], true);
    assert!(!resolved.normalize_locally, "Ollama normalizes its output");
    let (json, _) = body(&req(
        "embeddinggemma",
        Some(GaiseEmbeddingTask::Document),
        None,
    ));
    assert_eq!(
        json["input"][1], "title: none | text: beta",
        "untagged name resolves"
    );

    // nomic: side prefixes, 64-768 range.
    let (json, _) = body(&req(
        "nomic-embed-text:v1.5",
        Some(GaiseEmbeddingTask::Query),
        Some(32),
    ));
    assert_eq!(json["input"][0], "search_query: alpha");
    assert_eq!(json["dimensions"], 64);
    let (json, _) = body(&req(
        "nomic-embed-text:latest",
        Some(GaiseEmbeddingTask::Clustering),
        None,
    ));
    assert_eq!(json["input"][0], "clustering: alpha");

    // mxbai: instruction on queries only; fixed size drops dimensions.
    let (json, _) = body(&req(
        "mxbai-embed-large:latest",
        Some(GaiseEmbeddingTask::Query),
        Some(256),
    ));
    assert_eq!(
        json["input"][0],
        "Represent this sentence for searching relevant passages: alpha"
    );
    assert!(json.get("dimensions").is_none(), "fixed-size model");
    let (json, _) = body(&req(
        "mxbai-embed-large:latest",
        Some(GaiseEmbeddingTask::Document),
        None,
    ));
    assert_eq!(json["input"][0], "alpha");

    // Qwen3 / Arctic 2 query instructions.
    let (json, _) = body(&req(
        "qwen3-embedding:4b",
        Some(GaiseEmbeddingTask::Query),
        Some(8000),
    ));
    assert!(
        json["input"][0]
            .as_str()
            .unwrap()
            .starts_with("Instruct: Given a web search query")
    );
    assert!(
        json["input"][0]
            .as_str()
            .unwrap()
            .ends_with("\nQuery:alpha")
    );
    assert_eq!(json["dimensions"], 4096);
    let (json, _) = body(&req(
        "snowflake-arctic-embed2:latest",
        Some(GaiseEmbeddingTask::Query),
        Some(300),
    ));
    assert_eq!(json["input"][0], "query: alpha");
    assert_eq!(json["dimensions"], 256);

    // No task, no prefix; families without a convention are untouched.
    let (json, _) = body(&req("nomic-embed-text:latest", None, None));
    assert_eq!(json["input"][0], "alpha");
    let (json, _) = body(&req(
        "bge-m3:latest",
        Some(GaiseEmbeddingTask::Query),
        Some(512),
    ));
    assert_eq!(json["input"][0], "alpha");
    assert!(json.get("dimensions").is_none());

    // Unknown tag: pass-through.
    let (json, resolved) = body(&req(
        "my-custom-embed:latest",
        Some(GaiseEmbeddingTask::Query),
        Some(123),
    ));
    assert_eq!(json["input"][0], "alpha");
    assert_eq!(json["dimensions"], 123);
    assert!(!resolved.normalize_locally);
}
