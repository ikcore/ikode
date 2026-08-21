//! Per-family `generationConfig` matrix for the Gemini API.
//!
//! Sources (audited 2026-08-20): Gemini API / Vertex AI thinking guides,
//! the generateContent reference, model pages, and the 2026-07-21 changelog
//! deprecating sampling parameters (see `wiki/vendor-gemini.md#model-family-rules`).

use gaise_core::contracts::{
    GaiseContent, GaiseGenerationConfig, GaiseInstructRequest, GaiseMessage, OneOrMany,
};
use gaise_provider_gemini::contracts::models::{GeminiGenerationConfig, GeminiRequest};

fn build(model: &str, config: GaiseGenerationConfig) -> GeminiGenerationConfig {
    let request = GaiseInstructRequest {
        model: model.to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".into(),
            content: Some(OneOrMany::One(GaiseContent::Text { text: "hi".into() })),
            ..Default::default()
        }),
        generation_config: Some(config),
        ..Default::default()
    };
    GeminiRequest::from(&request)
        .generation_config
        .expect("generation config")
}

fn sink(effort: Option<&str>, tokens: Option<usize>) -> GaiseGenerationConfig {
    GaiseGenerationConfig {
        temperature: Some(0.3),
        top_p: Some(0.8),
        top_k: Some(20),
        max_tokens: Some(2048),
        thinking_effort: effort.map(str::to_string),
        thinking_tokens: tokens,
        ..Default::default()
    }
}

const GEMINI_3_TEXT: &[&str] = &[
    "gemini-3.7-flash",
    "gemini-3.6-flash",
    "gemini-3.5-flash",
    "gemini-3.5-flash-lite",
    "gemini-3.1-flash-lite",
    "gemini-3.1-pro-preview",
    "gemini-3-flash-preview",
];
const GEMINI_3_IMAGE: &[&str] = &[
    "gemini-3.1-flash-image",
    "gemini-3.1-flash-lite-image",
    "gemini-3-pro-image",
];
const GEMINI_25: &[&str] = &[
    "gemini-2.5-pro",
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite",
];

#[test]
fn gemini_3_families_never_receive_sampling_and_use_thinking_level() {
    for model in GEMINI_3_TEXT.iter().chain(GEMINI_3_IMAGE) {
        let cfg = build(model, sink(Some("high"), None));
        assert!(
            cfg.temperature.is_none(),
            "{model}: sampling is deprecated on Gemini 3.x"
        );
        assert!(cfg.top_p.is_none(), "{model}");
        assert!(cfg.top_k.is_none(), "{model}");
        let thinking = cfg.thinking_config.expect("thinking config");
        assert_eq!(thinking.thinking_level.as_deref(), Some("HIGH"), "{model}");
        assert!(
            thinking.thinking_budget.is_none(),
            "{model}: thinkingBudget must not be sent with thinkingLevel"
        );
        assert_eq!(
            thinking.include_thoughts,
            Some(true),
            "{model}: summaries default on"
        );
        assert_eq!(cfg.max_output_tokens, Some(2048), "{model}");
    }
}

#[test]
fn gemini_25_families_keep_sampling_and_use_clamped_budgets() {
    for model in GEMINI_25 {
        let cfg = build(model, sink(None, Some(4096)));
        assert_eq!(cfg.temperature, Some(0.3), "{model}");
        assert_eq!(cfg.top_p, Some(0.8), "{model}");
        assert_eq!(cfg.top_k, Some(20), "{model}");
        let thinking = cfg.thinking_config.expect("thinking config");
        assert_eq!(thinking.thinking_budget, Some(4096), "{model}");
        assert!(
            thinking.thinking_level.is_none(),
            "{model}: thinkingLevel errors on 2.5"
        );
    }
    // Range clamps.
    let budget = |model: &str, effort: Option<&str>, tokens: Option<usize>| {
        build(model, sink(effort, tokens))
            .thinking_config
            .and_then(|t| t.thinking_budget)
    };
    assert_eq!(
        budget("gemini-2.5-pro", None, Some(10)),
        Some(128),
        "Pro minimum is 128"
    );
    assert_eq!(
        budget("gemini-2.5-pro", Some("none"), None),
        Some(128),
        "Pro cannot disable thinking"
    );
    assert_eq!(budget("gemini-2.5-pro", None, Some(100_000)), Some(32_768));
    assert_eq!(
        budget("gemini-2.5-flash", Some("none"), None),
        Some(0),
        "Flash disables with 0"
    );
    assert_eq!(
        budget("gemini-2.5-flash", None, Some(100_000)),
        Some(24_576)
    );
    assert_eq!(
        budget("gemini-2.5-flash-lite", None, Some(100)),
        Some(512),
        "Flash-Lite minimum on-budget is 512"
    );
    assert_eq!(budget("gemini-2.5-flash-lite", Some("off"), None), Some(0));
    assert_eq!(
        budget("gemini-2.5-flash", Some("high"), None),
        Some(24_576),
        "effort approximates a budget on 2.5"
    );
    assert_eq!(budget("gemini-2.5-flash", Some("low"), None), Some(2_048));
    assert_eq!(
        budget("gemini-2.5-flash", Some("mystery"), None),
        None,
        "unknown effort sends nothing"
    );
    assert_eq!(
        budget("gemini-2.5-flash", Some("auto"), None),
        Some(-1),
        "auto is the dynamic budget"
    );
    assert_eq!(
        budget("gemini-2.5-pro", Some("ultra"), None),
        Some(32_768),
        "ultra = family maximum"
    );
    assert!(
        build("gemini-2.5-flash", sink(None, None))
            .thinking_config
            .is_none()
    );
}

#[test]
fn thinking_levels_are_clamped_per_family() {
    let level = |model: &str, effort: &str| {
        build(model, sink(Some(effort), None))
            .thinking_config
            .and_then(|t| t.thinking_level)
            .unwrap()
    };
    // minimal is rejected by 3.7 Flash and the Pro families.
    assert_eq!(level("gemini-3.7-flash", "minimal"), "LOW");
    assert_eq!(level("gemini-3.7-flash", "none"), "LOW");
    assert_eq!(level("gemini-3.1-pro-preview", "minimal"), "LOW");
    assert_eq!(level("gemini-3.6-flash", "minimal"), "MINIMAL");
    assert_eq!(
        level("gemini-3.6-flash", "none"),
        "MINIMAL",
        "thinking cannot be disabled on 3.x"
    );
    assert_eq!(level("gemini-3.5-flash", "max"), "HIGH");
    assert_eq!(level("gemini-3.5-flash", "xhigh"), "HIGH");
    // Image models: minimal or high only.
    assert_eq!(level("gemini-3.1-flash-image", "low"), "MINIMAL");
    assert_eq!(level("gemini-3.1-flash-image", "medium"), "HIGH");
    assert_eq!(level("gemini-3.1-flash-lite-image", "minimal"), "MINIMAL");
    assert_eq!(
        level("gemini-3-pro-image", "minimal"),
        "HIGH",
        "3 Pro Image accepts HIGH only"
    );
    assert_eq!(level("gemini-3-pro-image", "low"), "HIGH");
    // ultra = the family's top level; aliases resolve; auto omits the level; custom values are forwarded upper-cased.
    assert_eq!(level("gemini-3.6-flash", "ultra"), "HIGH");
    assert_eq!(level("gemini-3.1-flash-image", "ultracode"), "HIGH");
    assert_eq!(level("gemini-3.7-flash", "extra_high"), "HIGH");
    assert!(
        build("gemini-3.6-flash", sink(Some("auto"), None))
            .thinking_config
            .unwrap()
            .thinking_level
            .is_none()
    );
    assert_eq!(
        level("gemini-3.6-flash", "deep_think_9000"),
        "DEEP_THINK_9000"
    );
    // Token budgets become levels on 3.x and still respect the family set.
    let from_tokens = |model: &str, tokens: usize| {
        build(model, sink(None, Some(tokens)))
            .thinking_config
            .and_then(|t| t.thinking_level)
            .unwrap()
    };
    assert_eq!(from_tokens("gemini-3.6-flash", 1000), "LOW");
    assert_eq!(from_tokens("gemini-3.6-flash", 8000), "MEDIUM");
    assert_eq!(from_tokens("gemini-3.1-flash-image", 8000), "HIGH");
}

#[test]
fn embedding_requests_map_task_type_and_dimensions_per_model() {
    use gaise_core::contracts::{GaiseEmbeddingTask, GaiseEmbeddingsRequest, OneOrMany};
    use gaise_provider_gemini::contracts::models::gemini_embed_request;
    let req =
        |model: &str, task: Option<GaiseEmbeddingTask>, dims: Option<u32>| GaiseEmbeddingsRequest {
            model: model.into(),
            input: OneOrMany::Many(vec!["alpha".into(), "beta".into()]),
            task,
            dimensions: dims,
            ..Default::default()
        };

    // gemini-embedding-001: taskType field, range-clamped dimensions, raw truncation.
    let (wire, resolved) = gemini_embed_request(&req(
        "gemini-embedding-001",
        Some(GaiseEmbeddingTask::CodeQuery),
        Some(64),
    ));
    let json = serde_json::to_value(&wire).unwrap();
    assert_eq!(json["requests"][0]["taskType"], "CODE_RETRIEVAL_QUERY");
    assert_eq!(json["requests"][0]["outputDimensionality"], 128);
    assert_eq!(json["requests"][1]["content"]["parts"][0]["text"], "beta");
    assert!(
        resolved.normalize_locally,
        "001 leaves truncated vectors raw"
    );

    // gemini-embedding-2: prompt instruction, no taskType, provider normalizes.
    let (wire, resolved) = gemini_embed_request(&req(
        "gemini-embedding-2",
        Some(GaiseEmbeddingTask::Query),
        Some(768),
    ));
    let json = serde_json::to_value(&wire).unwrap();
    assert!(
        json["requests"][0].get("taskType").is_none(),
        "embedding-2 rejects taskType"
    );
    assert_eq!(
        json["requests"][0]["content"]["parts"][0]["text"],
        "task: search result | query: alpha"
    );
    assert_eq!(json["requests"][0]["outputDimensionality"], 768);
    assert!(!resolved.normalize_locally);

    // No task: nothing is added anywhere.
    let (wire, resolved) = gemini_embed_request(&req("gemini-embedding-2", None, None));
    let json = serde_json::to_value(&wire).unwrap();
    assert_eq!(json["requests"][0]["content"]["parts"][0]["text"], "alpha");
    assert!(json["requests"][0].get("outputDimensionality").is_none());
    assert!(!resolved.normalize_locally);

    // Unknown model: taskType default and pass-through dimensions.
    let (wire, _) = gemini_embed_request(&req(
        "gemini-embedding-9",
        Some(GaiseEmbeddingTask::Document),
        Some(5000),
    ));
    let json = serde_json::to_value(&wire).unwrap();
    assert_eq!(json["requests"][0]["taskType"], "RETRIEVAL_DOCUMENT");
    assert_eq!(json["requests"][0]["outputDimensionality"], 5000);
}
