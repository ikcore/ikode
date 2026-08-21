use super::{obj, p};
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct SearchCodeArgs {
    pub query: String,
    pub k: Option<usize>,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "search_code".to_string(),
        description: Some("Searches the indexed codebase for relevant chunks across programming languages and markdown. Returns ranked chunks with chunk_id, path, line range, and a code snippet — so you rarely need to open whole files. Call index_codebase first if nothing is indexed.".to_string()),
        parameters: Some(obj(
            vec![
                ("query", p("string", "Natural-language or keyword query describing what you are looking for")),
                ("k", p("integer", "Maximum number of chunks to return. Defaults to 8.")),
            ],
            vec!["query"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: SearchCodeArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    if !host.quiet() {
        println!(
            "{} Searching: {}",
            "🔎 ".bright_cyan(),
            args.query.bright_cyan()
        );
    }
    if host.indexer().is_empty() {
        let stats = host.indexer().index_all();
        if let Some(error) = &stats.error {
            anyhow::bail!("{error}");
        }
        if !host.quiet() {
            println!(
                "{} Auto-indexed {} files, {} chunks ({} ignored; {} .ikignore file(s)).",
                "📚 ".dimmed(),
                stats.files,
                stats.chunks,
                stats.ignored,
                stats.ikignore_files.len()
            );
        }
    }
    let k = args.k.unwrap_or(8).clamp(1, 50);
    let results = host.indexer().search(&args.query, k);
    if results.is_empty() {
        return Ok("No matching chunks found.".to_string());
    }
    let mut out = format!("Found {} matching chunk(s):\n", results.len());
    for ch in results {
        out.push_str(&format!(
            "\n• {} [{}] {}:{}-{}\n",
            ch.chunk_id, ch.kind, ch.path, ch.line_start, ch.line_end
        ));
        for l in ch.code.lines().take(4) {
            out.push_str(&format!("    {}\n", l));
        }
    }
    Ok(out)
}
