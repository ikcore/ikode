use super::obj;
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "index_codebase".to_string(),
        description: Some("Builds or refreshes the in-memory code+markdown index for the working directory, honoring root and nested .ikignore files. Run this before search_code/outline_file if the index is empty. References programming languages (Rust, Python, JS/TS, Go, Java, C/C++, C#) and Markdown, chunking them into functions, classes, impls, and sections.".to_string()),
        parameters: Some(obj(vec![], vec![])),
    }
}

pub async fn execute(host: &mut dyn ToolHost, _arguments: Option<&str>) -> Result<String> {
    let stats = host.indexer().index_all();
    if let Some(error) = &stats.error {
        anyhow::bail!("{error}");
    }
    if !host.quiet() {
        println!(
            "{} Indexed {} files, {} chunks, {} references ({} ignored; {} .ikignore file(s)).",
            "📚 ".bright_green(),
            stats.files,
            stats.chunks,
            stats.links,
            stats.ignored,
            stats.ikignore_files.len()
        );
    }
    let langs = host
        .indexer()
        .language_breakdown()
        .iter()
        .map(|(l, n)| format!("{} ({})", l, n))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "Indexed {} files, {} chunks, {} call/reference edges ({} skipped, {} ignored). \
         Found {} .ikignore file(s): {}. Languages: {}.",
        stats.files,
        stats.chunks,
        stats.links,
        stats.skipped,
        stats.ignored,
        stats.ikignore_files.len(),
        if stats.ikignore_files.is_empty() {
            "none".to_string()
        } else {
            stats.ikignore_files.join(", ")
        },
        langs
    ))
}
