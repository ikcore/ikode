//! Generate the embedding model × task/dimension matrix for `wiki/embeddings.md`.
//!
//! For every embedding model in the bundled registry, the real provider
//! request builder is run for a document and a query with a deliberately
//! awkward `dimensions` request, and what would go on the wire is printed:
//! the task control applied, the text actually sent, the dimension value, and
//! whether GAISe normalizes locally. Nothing contacts a provider.
//!
//! ```powershell
//! cargo run -p gaise-client --example embedding_matrix --all-features > matrix.md
//! ```

use gaise_core::contracts::{
    DimensionRule, EmbeddingTaskControl, GaiseEmbeddingTask, GaiseEmbeddingsRequest, OneOrMany,
    ResolvedEmbedding,
};
use gaise_core::registry::ModelRegistry;

const SAMPLE: &str = "hi";
const AWKWARD_DIMENSIONS: u32 = 300;

fn request(model: &str, task: Option<GaiseEmbeddingTask>) -> GaiseEmbeddingsRequest {
    GaiseEmbeddingsRequest {
        model: model.to_string(),
        input: OneOrMany::One(SAMPLE.to_string()),
        task,
        dimensions: Some(AWKWARD_DIMENSIONS),
        ..Default::default()
    }
}

/// The text the provider receives plus the wire task, through the real builder.
fn wire(
    provider: &str,
    model: &str,
    task: Option<GaiseEmbeddingTask>,
) -> Option<(String, Option<String>, Option<u32>, ResolvedEmbedding)> {
    let req = request(model, task);
    match provider {
        "openai" => {
            let (w, r) = gaise_provider_openai::openai_client::openai_embed_request(&req);
            let json = serde_json::to_value(&w).unwrap();
            Some((
                json["input"].as_str().unwrap_or("?").to_string(),
                None,
                w.dimensions,
                r,
            ))
        }
        "gemini" => {
            let (w, r) = gaise_provider_gemini::contracts::models::gemini_embed_request(&req);
            let json = serde_json::to_value(&w).unwrap();
            let first = &json["requests"][0];
            Some((
                first["content"]["parts"][0]["text"]
                    .as_str()
                    .unwrap_or("?")
                    .to_string(),
                first
                    .get("taskType")
                    .and_then(|v| v.as_str())
                    .map(|v| format!("`taskType: {v}`")),
                first
                    .get("outputDimensionality")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
                r,
            ))
        }
        "vertexai" => {
            let (w, r) = gaise_provider_vertexai::contracts::models::vertex_embed_request(&req);
            let json = serde_json::to_value(&w).unwrap();
            let first = &json["instances"][0];
            Some((
                first["content"].as_str().unwrap_or("?").to_string(),
                first
                    .get("task_type")
                    .and_then(|v| v.as_str())
                    .map(|v| format!("`task_type: {v}`")),
                json["parameters"]
                    .get("outputDimensionality")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
                r,
            ))
        }
        "bedrock" => {
            let (bodies, r) =
                gaise_provider_bedrock::bedrock_client::GaiseClientBedrock::embedding_bodies(&req)
                    .ok()?;
            let body = bodies.into_iter().next()?;
            let nova = &body["singleEmbeddingParams"];
            let text = body
                .get("inputText")
                .and_then(|v| v.as_str())
                .or_else(|| body["texts"][0].as_str())
                .or_else(|| nova["text"]["value"].as_str())
                .unwrap_or("?")
                .to_string();
            let task = body
                .get("input_type")
                .and_then(|v| v.as_str())
                .map(|v| format!("`input_type: {v}`"))
                .or_else(|| {
                    nova.get("embeddingPurpose")
                        .and_then(|v| v.as_str())
                        .map(|v| format!("`embeddingPurpose: {v}`"))
                });
            let dims = body
                .get("dimensions")
                .or_else(|| body.get("output_dimension"))
                .or_else(|| body["embeddingConfig"].get("outputEmbeddingLength"))
                .or_else(|| nova.get("embeddingDimension"))
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);
            Some((text, task, dims, r))
        }
        "ollama" => {
            let (w, r) = gaise_provider_ollama::ollama_client::ollama_embed_request(&req);
            Some((w.input[0].clone(), None, w.dimensions, r))
        }
        _ => None,
    }
}

fn describe_control(control: &EmbeddingTaskControl) -> String {
    match control {
        EmbeddingTaskControl::None => "none (task ignored)".into(),
        EmbeddingTaskControl::TaskType => "`taskType` field".into(),
        EmbeddingTaskControl::InputType => "`input_type` field (required)".into(),
        EmbeddingTaskControl::EmbeddingPurpose => "`embeddingPurpose` field (required)".into(),
        EmbeddingTaskControl::PromptInstruction => "prompt instruction".into(),
        EmbeddingTaskControl::Prefix { .. } => "text prefix per side".into(),
        EmbeddingTaskControl::QueryInstruction(_) => "query instruction".into(),
    }
}

fn describe_rule(rule: &DimensionRule, default: Option<u32>) -> String {
    let d = default
        .map(|d| d.to_string())
        .unwrap_or_else(|| "per tag".into());
    match rule {
        DimensionRule::Fixed => format!("{d} (fixed)"),
        DimensionRule::Range { min, max } => format!("{d} ({min}–{max})"),
        DimensionRule::Set(set) => format!(
            "{d} ({})",
            set.iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(" / ")
        ),
    }
}

fn show_text(sent: &str) -> String {
    if sent == SAMPLE {
        "unchanged".into()
    } else {
        format!("`{}`", sent.replace('\n', "\\n").replace('|', "\\|"))
    }
}

fn main() {
    let registry = ModelRegistry::bundled();
    println!(
        "<!-- Generated by `cargo run -p gaise-client --example embedding_matrix --all-features`; registry audited {} -->",
        registry.audited_on
    );
    println!();
    println!(
        "Each row runs the provider's real request builder for `task: document` and `task: query` with `dimensions: {AWKWARD_DIMENSIONS}` (an awkward value on purpose) on the sample text `{SAMPLE}`. **Sent** shows the text after any prefix or instruction; **dims** shows the wire value after snapping/clamping (— = omitted); **local L2** marks where GAISe normalizes the result itself because the provider would not."
    );
    println!();
    for provider in ["openai", "gemini", "vertexai", "bedrock", "ollama"] {
        let models: Vec<_> = registry
            .models_for(provider)
            .filter(|m| m.embedding.is_some())
            .collect();
        if models.is_empty() {
            continue;
        }
        println!("### {provider}");
        println!();
        println!(
            "| Model | Dimensions (default / options) | Task control | Document → sent | Query → sent | dims sent | local L2 (doc / query) |"
        );
        println!("|---|---|---|---|---|---|---|");
        for m in models {
            let profile = m.embedding.as_ref().unwrap();
            let drivable = m
                .classified()
                .map(|c| {
                    c.operations
                        .contains(&gaise_core::contracts::GaiseOperation::Embeddings)
                })
                .unwrap_or(false);
            // Use a concrete id for glob families so the builder sees a real tag.
            let id = m.model.replace('*', "latest");
            let name = format!("`{}`", m.model);
            if !drivable {
                println!(
                    "| {name} | {} | {} | — (not drivable: {}) | — | — | — |",
                    describe_rule(&profile.dimension_rule, profile.default_dimensions),
                    describe_control(&profile.task_control),
                    m.gaise_support.as_deref().unwrap_or("see notes")
                );
                continue;
            }
            let doc = wire(provider, &id, Some(GaiseEmbeddingTask::Document));
            let query = wire(provider, &id, Some(GaiseEmbeddingTask::Query));
            let (Some(doc), Some(query)) = (doc, query) else {
                println!(
                    "| {name} | {} | {} | — (builder rejected) | — | — | — |",
                    describe_rule(&profile.dimension_rule, profile.default_dimensions),
                    describe_control(&profile.task_control),
                );
                continue;
            };
            let doc_cell = match &doc.1 {
                Some(t) => format!("{} · {}", t, show_text(&doc.0)),
                None => show_text(&doc.0),
            };
            let query_cell = match &query.1 {
                Some(t) => format!("{} · {}", t, show_text(&query.0)),
                None => show_text(&query.0),
            };
            let dims = query.2.map(|d| d.to_string()).unwrap_or_else(|| "—".into());
            let l2 = format!(
                "{} / {}",
                if doc.3.normalize_locally { "yes" } else { "no" },
                if query.3.normalize_locally {
                    "yes"
                } else {
                    "no"
                }
            );
            println!(
                "| {name} | {} | {} | {doc_cell} | {query_cell} | {dims} | {l2} |",
                describe_rule(&profile.dimension_rule, profile.default_dimensions),
                describe_control(&profile.task_control),
            );
        }
        println!();
    }
}
