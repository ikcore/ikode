use super::{obj, p};
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct OutlineFileArgs {
    pub path: String,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "outline_file".to_string(),
        description: Some("Returns the structural outline of an indexed file (its functions, classes, impl blocks, or markdown sections) with line ranges, without dumping the whole file.".to_string()),
        parameters: Some(obj(
            vec![("path", p("string", "Path to the file to outline"))],
            vec!["path"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: OutlineFileArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    if !host.quiet() {
        println!(
            "{} Outlining: {}",
            "🗂️".bright_cyan(),
            args.path.bright_cyan()
        );
    }
    if host.indexer().is_empty() {
        host.indexer().index_all();
    }
    match host.indexer().outline(&args.path) {
        Some(file) => {
            let mut out = format!(
                "{} ({}) — {} chunks:\n",
                file.path,
                file.language,
                file.chunks.len()
            );
            for ch in &file.chunks {
                out.push_str(&format!(
                    "  {:<12} {}  ({}:{}-{})\n",
                    ch.kind, ch.name, ch.path, ch.line_start, ch.line_end
                ));
            }
            Ok(out)
        }
        None => Ok(format!(
            "File '{}' is not indexed. Run index_codebase or check the path.",
            args.path
        )),
    }
}
