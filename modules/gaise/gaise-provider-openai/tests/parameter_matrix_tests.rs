//! Per-family request-shape matrix for Chat Completions.
//!
//! Sources (audited 2026-08-20): the Chat Completions reference, the
//! latest-model guide ("parameter compatibility" for GPT-5.2/5.4), each model
//! page's effort list, and the images guide for `detail: original`. See
//! `wiki/vendor-openai.md#model-family-rules`.

use gaise_core::contracts::{
    GaiseContent, GaiseGenerationConfig, GaiseInstructRequest, GaiseMessage, GaiseTool, OneOrMany,
};
use gaise_provider_openai::contracts::models::OpenAIChatRequest;
use gaise_provider_openai::openai_client::{normalize_chat_effort, openai_chat_rules};

fn approx(value: &serde_json::Value, expected: f64) -> bool {
    value.as_f64().is_some_and(|v| (v - expected).abs() < 1e-5)
}

fn build(model: &str, config: GaiseGenerationConfig, tools: bool) -> serde_json::Value {
    let request = GaiseInstructRequest {
        model: model.to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".into(),
            content: Some(OneOrMany::Many(vec![
                GaiseContent::Text { text: "hi".into() },
                GaiseContent::Image {
                    data: vec![137, 80, 78, 71],
                    format: Some("png".into()),
                },
            ])),
            ..Default::default()
        }),
        generation_config: Some(config),
        tools: tools.then(|| {
            vec![GaiseTool {
                name: "lookup".into(),
                description: None,
                parameters: None,
            }]
        }),
        ..Default::default()
    };
    serde_json::to_value(OpenAIChatRequest::from(&request)).unwrap()
}

fn sink(effort: Option<&str>) -> GaiseGenerationConfig {
    GaiseGenerationConfig {
        temperature: Some(0.3),
        top_p: Some(0.8),
        max_tokens: Some(2048),
        thinking_effort: effort.map(str::to_string),
        input_image_detail: Some("original".into()),
        ..Default::default()
    }
}

fn image_detail(json: &serde_json::Value) -> serde_json::Value {
    json["messages"][0]["content"][1]["image_url"]["detail"].clone()
}

#[test]
fn max_tokens_always_maps_to_max_completion_tokens() {
    for model in [
        "gpt-5.6",
        "gpt-5.4-mini",
        "gpt-4.1",
        "o3",
        "chat-latest",
        "gpt-audio-1.5",
    ] {
        let json = build(model, sink(None), false);
        assert_eq!(json["max_completion_tokens"], 2048, "{model}");
        assert!(
            json.get("max_tokens").is_none(),
            "{model}: legacy max_tokens is never sent"
        );
    }
}

#[test]
fn gpt5_families_drop_sampling_unless_effort_is_none() {
    // Effort explicitly requested: sampling rejected by the API -> omitted.
    for model in [
        "gpt-5.6",
        "gpt-5.6-terra",
        "gpt-5.5",
        "gpt-5.4",
        "gpt-5.4-mini",
        "gpt-5.2",
        "gpt-5.1",
    ] {
        let json = build(model, sink(Some("high")), false);
        assert!(
            json.get("temperature").is_none(),
            "{model}: temperature requires effort none"
        );
        assert!(json.get("top_p").is_none(), "{model}");
        assert_eq!(json["reasoning_effort"], "high", "{model}");

        let json = build(model, sink(Some("none")), false);
        assert!(
            approx(&json["temperature"], 0.3),
            "{model}: sampling is accepted with effort none"
        );
        assert!(approx(&json["top_p"], 0.8), "{model}");
        assert_eq!(json["reasoning_effort"], "none", "{model}");
    }

    // No effort requested: the family default decides.
    for (model, default_is_none) in [
        ("gpt-5.6", false),
        ("gpt-5.5", false),
        ("gpt-5.4", true),
        ("gpt-5.2", true),
        ("gpt-5.1", true),
    ] {
        let json = build(model, sink(None), false);
        assert_eq!(
            json.get("temperature").is_some(),
            default_is_none,
            "{model}: default effort"
        );
        assert!(
            json.get("reasoning_effort").is_none(),
            "{model}: nothing sent when not configured"
        );
    }

    // GPT-5 / mini / nano and the o-series have no `none`: sampling is never accepted.
    for model in [
        "gpt-5",
        "gpt-5-mini",
        "gpt-5-nano-2025-08-07",
        "o3",
        "o4-mini",
        "gpt-5.3-codex",
    ] {
        for effort in [None, Some("none"), Some("high")] {
            let json = build(model, sink(effort), false);
            assert!(json.get("temperature").is_none(), "{model} {effort:?}");
            assert!(json.get("top_p").is_none(), "{model} {effort:?}");
        }
    }
}

#[test]
fn effort_values_are_clamped_per_family() {
    let effort = |model: &str, level: &str| {
        let rules = openai_chat_rules(model);
        normalize_chat_effort(&rules, level)
    };
    assert_eq!(effort("gpt-5.6", "max").as_deref(), Some("max"));
    assert_eq!(
        effort("gpt-5.5", "max").as_deref(),
        Some("xhigh"),
        "5.5 tops out at xhigh"
    );
    assert_eq!(effort("gpt-5.4", "max").as_deref(), Some("xhigh"));
    assert_eq!(
        effort("gpt-5.1", "xhigh").as_deref(),
        Some("high"),
        "5.1 tops out at high"
    );
    assert_eq!(
        effort("gpt-5.1", "minimal").as_deref(),
        Some("low"),
        "5.1 has no minimal: the smallest non-zero level is low"
    );
    assert_eq!(
        effort("gpt-5.1", "ultra").as_deref(),
        Some("high"),
        "ultra = highest the family offers"
    );
    assert_eq!(effort("gpt-5.6", "ultracode").as_deref(), Some("max"));
    assert_eq!(
        effort("gpt-5.5", "extra_high").as_deref(),
        Some("xhigh"),
        "aliases resolve"
    );
    assert_eq!(
        effort("gpt-5.6", "auto"),
        None,
        "auto lets OpenAI apply the default"
    );
    assert_eq!(
        effort("gpt-7-hypothetical", "ultra").as_deref(),
        Some("max"),
        "unknown family: ultra becomes max"
    );
    assert_eq!(
        effort("gpt-5", "none").as_deref(),
        Some("minimal"),
        "GPT-5 has minimal, not none"
    );
    assert_eq!(effort("gpt-5", "max").as_deref(), Some("high"));
    assert_eq!(effort("o3", "none").as_deref(), Some("low"));
    assert_eq!(effort("o4-mini", "xhigh").as_deref(), Some("high"));
    assert_eq!(effort("gpt-5.3-codex", "none").as_deref(), Some("low"));
    assert_eq!(effort("gpt-5.6", "disabled").as_deref(), Some("none"));
    assert_eq!(
        effort("gpt-5.6", "deep_think_9000").as_deref(),
        Some("deep_think_9000"),
        "unknown values are forwarded"
    );
    assert_eq!(
        effort("gpt-4.1", "high"),
        None,
        "non-reasoning models never get reasoning_effort"
    );
    assert_eq!(
        effort("gpt-7-hypothetical", "weird").as_deref(),
        Some("weird"),
        "unknown models pass through"
    );

    let json = build("gpt-5.5", sink(Some("max")), false);
    assert_eq!(json["reasoning_effort"], "xhigh");
}

#[test]
fn non_reasoning_families_keep_sampling_and_never_receive_reasoning_effort() {
    for model in [
        "gpt-4.1",
        "gpt-4.1-nano",
        "gpt-4o",
        "chat-latest",
        "gpt-5.3-chat-latest",
        "gpt-audio-1.5",
        "ft:gpt-4.1:acme::abc",
    ] {
        let json = build(model, sink(Some("high")), false);
        assert!(approx(&json["temperature"], 0.3), "{model}");
        assert!(approx(&json["top_p"], 0.8), "{model}");
        assert!(json.get("reasoning_effort").is_none(), "{model}");
        assert_eq!(
            image_detail(&json),
            "high",
            "{model}: original degrades to high"
        );
    }
}

#[test]
fn original_image_detail_only_where_supported() {
    for model in ["gpt-5.6", "gpt-5.5", "gpt-5.4"] {
        assert_eq!(
            image_detail(&build(model, sink(None), false)),
            "original",
            "{model}"
        );
    }
    for model in [
        "gpt-5.4-mini",
        "gpt-5.4-nano",
        "gpt-5.2",
        "gpt-5",
        "o3",
        "gpt-4.1",
    ] {
        assert_eq!(
            image_detail(&build(model, sink(None), false)),
            "high",
            "{model}"
        );
    }
    let json = build(
        "gpt-5.4-mini",
        GaiseGenerationConfig {
            input_image_detail: Some("LOW".into()),
            ..Default::default()
        },
        false,
    );
    assert_eq!(image_detail(&json), "low");
}

#[test]
fn gpt56_function_tools_force_none_and_reopen_sampling() {
    let json = build("gpt-5.6-sol", sink(Some("high")), true);
    assert_eq!(
        json["reasoning_effort"], "none",
        "tools on 5.6 Chat force none"
    );
    assert!(
        approx(&json["temperature"], 0.3),
        "with effort none sampling is valid again"
    );
    let json = build("gpt-5.5", sink(Some("high")), true);
    assert_eq!(json["reasoning_effort"], "high", "rule is specific to 5.6");
    assert!(json.get("temperature").is_none());
}

#[test]
fn responses_only_models_are_identified() {
    for model in [
        "gpt-5.5-pro",
        "gpt-5.2-pro",
        "gpt-5-pro-2025-10-06",
        "o3-pro",
        "gpt-5.6-cyber",
        "daybreak-red-latest",
    ] {
        assert!(!openai_chat_rules(model).chat_supported, "{model}");
    }
    for model in [
        "gpt-5.6",
        "gpt-5.4-nano",
        "gpt-4.1",
        "o4-mini",
        "gpt-7-hypothetical",
    ] {
        assert!(openai_chat_rules(model).chat_supported, "{model}");
    }
}

#[test]
fn embedding_dimensions_only_for_text_embedding_3() {
    use gaise_core::contracts::{GaiseEmbeddingTask, GaiseEmbeddingsRequest, OneOrMany};
    use gaise_provider_openai::openai_client::openai_embed_request;
    let req = |model: &str, dims: Option<u32>| GaiseEmbeddingsRequest {
        model: model.into(),
        input: OneOrMany::One("hi".into()),
        task: Some(GaiseEmbeddingTask::Query),
        dimensions: dims,
        ..Default::default()
    };
    let dims = |model: &str, d: Option<u32>| {
        let (wire, resolved) = openai_embed_request(&req(model, d));
        let json = serde_json::to_value(&wire).unwrap();
        assert!(json.get("task").is_none() && json.get("input_type").is_none());
        assert!(
            !resolved.normalize_locally,
            "OpenAI vectors are unit length"
        );
        wire.dimensions
    };
    assert_eq!(dims("text-embedding-3-large", Some(5000)), Some(3072));
    assert_eq!(dims("text-embedding-3-small", Some(5000)), Some(1536));
    assert_eq!(dims("text-embedding-3-small", Some(256)), Some(256));
    assert_eq!(
        dims("text-embedding-ada-002", Some(256)),
        None,
        "ada rejects dimensions"
    );
    assert_eq!(dims("text-embedding-3-large", None), None);
    assert_eq!(
        dims("text-embedding-9-future", Some(256)),
        Some(256),
        "unknown models pass through"
    );
    // Task prefixes never touch OpenAI inputs.
    let (wire, _) = openai_embed_request(&req("text-embedding-3-small", None));
    assert_eq!(serde_json::to_value(&wire).unwrap()["input"], "hi");
}
