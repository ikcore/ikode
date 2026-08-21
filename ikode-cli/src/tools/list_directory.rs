use super::{obj, p};
use crate::harness;
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ListDirectoryArgs {
    pub path: Option<String>,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "list_directory".to_string(),
        description: Some("Lists the immediate contents of a directory (subdirectories first, then files) without recursing. Use this to discover project structure — especially for a fresh or not-yet-indexed project. Defaults to the working-directory root when no path is given.".to_string()),
        parameters: Some(obj(
            vec![("path", p("string", "Directory to list, relative to the working directory. Defaults to the root."))],
            vec![],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: ListDirectoryArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    let rel = args.path.unwrap_or_else(|| ".".to_string());
    if !host.quiet() {
        println!("{} Listing: {}", "📁".bright_cyan(), rel.bright_cyan());
    }

    let validated_path = match host.validate_path(&rel) {
        Ok(p) => p,
        Err(e) => return Ok(format!("Error: {}", e)),
    };
    if !validated_path.is_dir() {
        return Ok(format!("Error: '{}' is not a directory.", rel));
    }
    match harness::list_dir(&validated_path) {
        Ok(entries) if entries.is_empty() => Ok(format!("{} is empty.", rel)),
        Ok(entries) => Ok(format!(
            "{} ({} entries):\n{}",
            rel,
            entries.len(),
            entries.join("\n")
        )),
        Err(e) => Ok(format!("Error listing directory: {}", e)),
    }
}
