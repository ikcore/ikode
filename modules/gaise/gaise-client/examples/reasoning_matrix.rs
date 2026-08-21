//! Generate the vendor model × reasoning-level matrix for `wiki/reasoning.md`.
//!
//! For every reasoning-capable model in the bundled registry, every canonical
//! effort level is run through the real provider request builder and the wire
//! value that would be sent is printed. Nothing contacts a provider.
//!
//! ```powershell
//! cargo run -p gaise-client --example reasoning_matrix --all-features > matrix.md
//! ```

use gaise_core::contracts::{
    GaiseContent, GaiseGenerationConfig, GaiseInstructRequest, GaiseMessage, GaiseReasoningEffort,
    OneOrMany,
};
use gaise_core::registry::ModelRegistry;

const LEVELS: [&str; 9] = [
    "none", "auto", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];

fn request(model: &str, effort: &str) -> GaiseInstructRequest {
    GaiseInstructRequest {
        model: model.to_string(),
        input: OneOrMany::One(GaiseMessage {
            role: "user".into(),
            content: Some(OneOrMany::One(GaiseContent::Text { text: "hi".into() })),
            ..Default::default()
        }),
        generation_config: Some(GaiseGenerationConfig {
            thinking_effort: Some(effort.to_string()),
            max_tokens: Some(8192),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// What the provider would receive for one (model, effort) pair.
fn wire_value(provider: &str, model: &str, effort: &str) -> String {
    let req = request(model, effort);
    match provider {
        "openai" => {
            let rules = gaise_provider_openai::openai_client::openai_chat_rules(model);
            if !rules.chat_supported {
                return "— (Responses-only)".into();
            }
            let json = serde_json::to_value(
                gaise_provider_openai::contracts::models::OpenAIChatRequest::from(&req),
            )
            .unwrap();
            match json.get("reasoning_effort") {
                Some(v) => format!("`reasoning_effort: {}`", v.as_str().unwrap_or("?")),
                None if rules.reasoning => "omitted (model default)".into(),
                None => "omitted (non-reasoning)".into(),
            }
        }
        "anthropic" => {
            let json = serde_json::to_value(
                gaise_provider_anthropic::contracts::models::AnthropicRequest::from(&req),
            )
            .unwrap();
            describe_claude(&json["thinking"], &json["output_config"])
        }
        "bedrock" => {
            if !model.to_ascii_lowercase().contains("claude") && !model.contains("nova") {
                return "n/a".into();
            }
            match gaise_provider_bedrock::bedrock_client::GaiseClientBedrock::reasoning_request_fields(&req) {
                Some(fields) => {
                    if let Some(cfg) = fields.get("reasoningConfig") {
                        format!(
                            "`reasoningConfig.maxReasoningEffort: {}`",
                            cfg["maxReasoningEffort"].as_str().unwrap_or("?")
                        )
                    } else {
                        describe_claude(&fields["thinking"], &fields["output_config"])
                    }
                }
                None => "omitted".into(),
            }
        }
        "gemini" => {
            let json = serde_json::to_value(
                gaise_provider_gemini::contracts::models::GeminiRequest::from(&req),
            )
            .unwrap();
            describe_google(&json["generationConfig"]["thinkingConfig"])
        }
        "vertexai" => {
            let json = serde_json::to_value(
                gaise_provider_vertexai::contracts::models::GoogleInstructRequest::from(&req),
            )
            .unwrap();
            // Vertex serializes the parameters block under `generationConfig`
            // (snake_case struct field names, camelCase thinking keys).
            let cfg = json
                .get("generationConfig")
                .or_else(|| json.get("generation_config"))
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let thinking = cfg
                .get("thinkingConfig")
                .or_else(|| cfg.get("thinking_config"))
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            describe_google(&thinking)
        }
        "ollama" => {
            let cfg = req.generation_config.as_ref().unwrap();
            match gaise_provider_ollama::ollama_client::ollama_think(model, cfg) {
                Some(think) => format!("`think: {}`", serde_json::to_value(think).unwrap()),
                None => "omitted".into(),
            }
        }
        _ => "n/a".into(),
    }
}

fn describe_claude(thinking: &serde_json::Value, output_config: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    if let Some(t) = thinking.get("type").and_then(|v| v.as_str()) {
        match thinking.get("budget_tokens").and_then(|v| v.as_u64()) {
            Some(budget) => parts.push(format!("`thinking: {t} ({budget})`")),
            None => parts.push(format!("`thinking: {t}`")),
        }
    }
    if let Some(e) = output_config.get("effort").and_then(|v| v.as_str()) {
        parts.push(format!("`effort: {e}`"));
    }
    if parts.is_empty() {
        "omitted".into()
    } else {
        parts.join(" ")
    }
}

fn describe_google(thinking: &serde_json::Value) -> String {
    if thinking.is_null() {
        return "omitted".into();
    }
    if let Some(level) = thinking.get("thinkingLevel").and_then(|v| v.as_str()) {
        return format!("`thinkingLevel: {level}`");
    }
    if let Some(budget) = thinking.get("thinkingBudget").and_then(|v| v.as_i64()) {
        return if budget < 0 {
            "`thinkingBudget: -1` (dynamic)".into()
        } else {
            format!("`thinkingBudget: {budget}`")
        };
    }
    "thinking config without level (model default)".into()
}

fn main() {
    let registry = ModelRegistry::bundled();
    let providers = [
        ("openai", "OpenAI (Chat Completions)"),
        ("anthropic", "Anthropic (Messages)"),
        (
            "bedrock",
            "Amazon Bedrock (Converse `additionalModelRequestFields`)",
        ),
        (
            "gemini",
            "Google Gemini API (`generationConfig.thinkingConfig`)",
        ),
        (
            "vertexai",
            "Google Vertex AI (`generationConfig.thinkingConfig`)",
        ),
        ("ollama", "Ollama (`think`)"),
    ];
    println!(
        "<!-- Generated by `cargo run -p gaise-client --example reasoning_matrix --all-features`; registry audited {} -->",
        registry.audited_on
    );
    println!();
    for (key, title) in providers {
        let models: Vec<_> = registry
            .models_for(key)
            .filter(|m| {
                let c = m.classified().ok();
                c.as_ref()
                    .is_some_and(|c| c.reasoning == gaise_core::contracts::GaiseSupport::Supported)
                    && (!m.model.contains('*') || key == "ollama")
                    && m.status != "retired"
            })
            .collect();
        if models.is_empty() {
            continue;
        }
        println!("### {title}");
        println!();
        print!("| Model |");
        for level in LEVELS {
            print!(" `{level}` |");
        }
        println!();
        print!("|---|");
        for _ in LEVELS {
            print!("---|");
        }
        println!();
        for m in models {
            // Ollama entries are family globs; show a representative tag.
            let model = m.model.replace('*', "latest");
            print!("| `{}` |", m.model);
            for level in LEVELS {
                print!(" {} |", wire_value(key, &model, level));
            }
            println!();
        }
        println!();
    }
    // Sanity: the canonical ladder parses back to itself.
    for level in LEVELS {
        assert_eq!(GaiseReasoningEffort::parse(level).as_str(), level);
    }
}
