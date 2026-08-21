//! Per-family request-shape matrix for the Messages API.
//!
//! Every current Claude family is given the same "kitchen sink" generation
//! config and the serialized request is checked against the parameters that
//! family accepts. Sources: the Claude model overview, thinking-troubleshooting
//! ("configurations each model rejects"), and effort pages, audited 2026-08-20
//! (see `wiki/vendor-anthropic.md#model-family-rules`).

use gaise_core::contracts::{
    GaiseContent, GaiseGenerationConfig, GaiseInstructRequest, GaiseMessage, OneOrMany,
};
use gaise_provider_anthropic::contracts::models::AnthropicRequest;

fn approx(value: &serde_json::Value, expected: f64) -> bool {
    value.as_f64().is_some_and(|v| (v - expected).abs() < 1e-5)
}

fn request(model: &str, config: GaiseGenerationConfig) -> serde_json::Value {
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
    serde_json::to_value(AnthropicRequest::from(&request)).unwrap()
}

fn sink() -> GaiseGenerationConfig {
    GaiseGenerationConfig {
        temperature: Some(0.3),
        top_p: Some(0.8),
        top_k: Some(20),
        max_tokens: Some(2048),
        thinking_effort: Some("high".into()),
        thinking_tokens: Some(4096),
        include_thoughts: Some(true),
        ..Default::default()
    }
}

fn sampling_only() -> GaiseGenerationConfig {
    GaiseGenerationConfig {
        temperature: Some(0.3),
        top_p: Some(0.8),
        top_k: Some(20),
        max_tokens: Some(1024),
        ..Default::default()
    }
}

const ADAPTIVE_ONLY: &[&str] = &[
    "claude-opus-5",
    "claude-fable-5",
    "claude-mythos-5",
    "claude-opus-4-8",
    "claude-opus-4-7",
    "claude-sonnet-5",
];
const ADAPTIVE_OR_MANUAL: &[&str] = &["claude-opus-4-6", "claude-sonnet-4-6"];
const MANUAL: &[&str] = &[
    "claude-opus-4-5-20251101",
    "claude-sonnet-4-5-20250929",
    "claude-haiku-4-5-20251001",
];

#[test]
fn adaptive_only_families_never_receive_sampling_or_budgets() {
    for model in ADAPTIVE_ONLY {
        let json = request(model, sink());
        assert!(
            json.get("temperature").is_none(),
            "{model}: temperature must be omitted"
        );
        assert!(
            json.get("top_p").is_none(),
            "{model}: top_p must be omitted"
        );
        assert!(
            json.get("top_k").is_none(),
            "{model}: top_k must be omitted"
        );
        assert_eq!(json["thinking"]["type"], "adaptive", "{model}");
        assert!(
            json["thinking"].get("budget_tokens").is_none(),
            "{model}: budget_tokens is rejected"
        );
        assert_eq!(json["thinking"]["display"], "summarized", "{model}");
        assert_eq!(json["output_config"]["effort"], "high", "{model}");
        assert_eq!(json["max_tokens"], 2048, "{model}");

        // Sampling-only requests still drop sampling on fixed-sampling families.
        let json = request(model, sampling_only());
        assert!(json.get("temperature").is_none(), "{model}");
        assert!(json.get("top_p").is_none(), "{model}");
        assert!(json.get("top_k").is_none(), "{model}");
        assert!(
            json.get("thinking").is_none(),
            "{model}: no thinking block without reasoning controls"
        );
    }
}

#[test]
fn four_six_families_use_adaptive_thinking_and_keep_sampling_when_thinking_is_off() {
    for model in ADAPTIVE_OR_MANUAL {
        let json = request(model, sink());
        assert_eq!(json["thinking"]["type"], "adaptive", "{model}");
        assert!(
            json["thinking"].get("budget_tokens").is_none(),
            "{model}: adaptive ignores budgets"
        );
        assert_eq!(json["output_config"]["effort"], "high", "{model}");
        // Thinking enabled: temperature and top_k are dropped; top_p only survives in 0.95..=1.0.
        assert!(json.get("temperature").is_none(), "{model}");
        assert!(json.get("top_k").is_none(), "{model}");
        assert!(
            json.get("top_p").is_none(),
            "{model}: 0.8 is outside the thinking-compatible range"
        );

        let json = request(model, sampling_only());
        assert!(approx(&json["temperature"], 0.3), "{model}");
        assert!(approx(&json["top_p"], 0.8), "{model}");
        assert_eq!(json["top_k"], 20, "{model}");
        assert!(json.get("thinking").is_none(), "{model}");
        assert!(json.get("output_config").is_none(), "{model}");
    }
}

#[test]
fn four_five_families_use_manual_budgets_and_exclusive_sampling() {
    for model in MANUAL {
        let json = request(model, sink());
        assert_eq!(json["thinking"]["type"], "enabled", "{model}");
        assert_eq!(json["thinking"]["budget_tokens"], 4096, "{model}");
        assert_eq!(
            json["max_tokens"], 5120,
            "{model}: max_tokens is raised above the budget (budget_tokens < max_tokens)"
        );
        assert_eq!(json["thinking"]["display"], "summarized", "{model}");
        assert!(
            json.get("temperature").is_none(),
            "{model}: thinking forces default temperature"
        );
        assert!(json.get("top_k").is_none(), "{model}");
        if model.contains("opus-4-5") {
            assert_eq!(
                json["output_config"]["effort"], "high",
                "{model}: Opus 4.5 accepts effort"
            );
        } else {
            assert!(
                json.get("output_config").is_none(),
                "{model}: effort is not supported"
            );
        }

        // Without thinking, temperature and top_p are mutually exclusive: temperature wins.
        let json = request(model, sampling_only());
        assert!(approx(&json["temperature"], 0.3), "{model}");
        assert!(
            json.get("top_p").is_none(),
            "{model}: only one of temperature/top_p may be sent"
        );
        assert_eq!(json["top_k"], 20, "{model}");
        assert!(json.get("thinking").is_none(), "{model}");

        let json = request(
            model,
            GaiseGenerationConfig {
                top_p: Some(0.9),
                ..Default::default()
            },
        );
        assert!(approx(&json["top_p"], 0.9), "{model}: top_p alone is fine");
    }
}

#[test]
fn effort_is_forwarded_lowercase_and_max_tokens_defaults() {
    let json = request(
        "claude-opus-5",
        GaiseGenerationConfig {
            thinking_effort: Some("XHIGH".into()),
            ..Default::default()
        },
    );
    assert_eq!(json["output_config"]["effort"], "xhigh");
    assert_eq!(
        json["max_tokens"], 4096,
        "max_tokens is required by the API; the adapter defaults it"
    );
}

#[test]
fn effort_none_disables_thinking_where_allowed_and_is_dropped_on_always_on_families() {
    for model in [
        "claude-opus-5",
        "claude-opus-4-8",
        "claude-sonnet-5",
        "claude-sonnet-4-6",
        "claude-haiku-4-5-20251001",
    ] {
        let json = request(
            model,
            GaiseGenerationConfig {
                thinking_effort: Some("none".into()),
                include_thoughts: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(json["thinking"]["type"], "disabled", "{model}");
        assert!(
            json["thinking"].get("display").is_none(),
            "{model}: display is invalid with disabled"
        );
        assert!(
            json.get("output_config").is_none(),
            "{model}: no effort alongside disabled"
        );
    }
    for model in ["claude-fable-5", "claude-mythos-5", "claude-mythos-preview"] {
        let json = request(
            model,
            GaiseGenerationConfig {
                thinking_effort: Some("off".into()),
                ..Default::default()
            },
        );
        assert!(
            json.get("thinking").is_none(),
            "{model}: always-on families cannot send disabled"
        );
        assert!(json.get("output_config").is_none(), "{model}");
    }
}

#[test]
fn effort_levels_are_clamped_to_what_each_family_accepts() {
    let effort = |model: &str, level: &str| {
        request(
            model,
            GaiseGenerationConfig {
                thinking_effort: Some(level.into()),
                ..Default::default()
            },
        )["output_config"]["effort"]
            .clone()
    };
    assert_eq!(effort("claude-opus-5", "minimal"), "low");
    assert_eq!(effort("claude-opus-5", "xhigh"), "xhigh");
    assert_eq!(
        effort("claude-opus-4-6", "xhigh"),
        "max",
        "4.6 has no xhigh; max is the nearest"
    );
    assert_eq!(effort("claude-sonnet-4-6", "max"), "max");
    assert_eq!(effort("claude-opus-4-5-20251101", "xhigh"), "high");
    assert_eq!(effort("claude-opus-4-5-20251101", "max"), "high");
    assert_eq!(
        effort("claude-opus-4-6", "ultra"),
        "max",
        "ultra = highest the family offers"
    );
    assert_eq!(effort("claude-opus-4-5-20251101", "ultracode"), "high");
    assert_eq!(
        effort("claude-opus-5", "extra_high"),
        "xhigh",
        "aliases resolve"
    );
    assert_eq!(
        effort("claude-opus-4-6", "deep_think_9000"),
        "deep_think_9000",
        "unknown future values are forwarded"
    );
    assert!(
        effort("claude-sonnet-4-5-20250929", "high").is_null(),
        "Sonnet 4.5 has no effort control"
    );
}

#[test]
fn manual_budgets_respect_minimum_and_ceiling() {
    let json = request(
        "claude-haiku-4-5-20251001",
        GaiseGenerationConfig {
            thinking_tokens: Some(100),
            max_tokens: Some(512),
            ..Default::default()
        },
    );
    assert_eq!(
        json["thinking"]["budget_tokens"], 1024,
        "budget is raised to the API minimum"
    );
    assert_eq!(
        json["max_tokens"], 2048,
        "max_tokens is raised above the budget"
    );

    let json = request(
        "claude-opus-4-5-20251101",
        GaiseGenerationConfig {
            max_tokens: Some(200_000),
            ..Default::default()
        },
    );
    assert_eq!(json["max_tokens"], 64_000, "Claude 4.5 caps output at 64k");

    let json = request(
        "claude-opus-4-5-20251101",
        GaiseGenerationConfig {
            max_tokens: Some(64_000),
            thinking_tokens: Some(64_000),
            ..Default::default()
        },
    );
    assert_eq!(json["max_tokens"], 64_000);
    assert_eq!(
        json["thinking"]["budget_tokens"],
        64_000 - 1024,
        "budget shrinks when the ceiling is hit"
    );

    let json = request(
        "claude-opus-5",
        GaiseGenerationConfig {
            max_tokens: Some(200_000),
            ..Default::default()
        },
    );
    assert_eq!(json["max_tokens"], 128_000);
}

#[test]
fn auto_enables_thinking_with_provider_defaults() {
    let auto = |model: &str| {
        request(
            model,
            GaiseGenerationConfig {
                thinking_effort: Some("auto".into()),
                max_tokens: Some(2048),
                ..Default::default()
            },
        )
    };
    // Adaptive families: adaptive block, no effort (provider default depth).
    for model in ["claude-opus-5", "claude-sonnet-4-6", "claude-fable-5"] {
        let json = auto(model);
        assert_eq!(json["thinking"]["type"], "adaptive", "{model}");
        assert!(json.get("output_config").is_none(), "{model}");
    }
    // Manual families: enabled with the default budget and max_tokens raised above it.
    let json = auto("claude-haiku-4-5-20251001");
    assert_eq!(json["thinking"]["type"], "enabled");
    assert_eq!(json["thinking"]["budget_tokens"], 4096);
    assert_eq!(json["max_tokens"], 5120);
    assert!(json.get("output_config").is_none());
    assert_eq!(
        auto("claude-sonnet-4-6")["max_tokens"],
        2048,
        "adaptive families keep max_tokens"
    );
}
