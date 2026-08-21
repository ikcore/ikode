use super::{obj, p};
use crate::harness;
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct AskCodebaseArgs {
    pub question: String,
    pub k: Option<usize>,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "ask_codebase".to_string(),
        description: Some("Flagship retrieval: given a natural-language question, returns the most relevant code chunks AND the connectivity subgraph showing how they are wired together (CALLS / REFERENCES / TESTS / IMPLEMENTS / EXTENDS / FOR_TYPE edges among them). Pure-algorithmic — fuses keyword relevance with graph neighbourhood, no whole-file reads. Prefer this as your first move to understand how a feature works: it returns the parts and the wiring, so you rarely need to open files.".to_string()),
        parameters: Some(obj(
            vec![
                ("question", p("string", "Natural-language question or task, e.g. 'how does retry/backoff work?'")),
                ("k", p("integer", "Candidate chunks to retrieve. Defaults to 12.")),
            ],
            vec!["question"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: AskCodebaseArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    if !host.quiet() {
        println!("{} {}", "💬 ".bright_cyan(), args.question.bright_cyan());
    }
    if host.indexer().is_empty() {
        host.indexer().index_all();
    }
    let k = args.k.unwrap_or(12).clamp(1, 50);
    // Same retrieval strategy as `/ask`: semantic (cosine over the per-chunk
    // embedding property) when embeddings exist, keyword otherwise.
    let result = host.ask_codebase(&args.question, k).await;
    Ok(harness::render_ask_result(&args.question, &result))
}
