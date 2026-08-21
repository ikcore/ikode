use super::{obj, p};
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ReadFileArgs {
    pub path: String,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "read_file".to_string(),
        description: Some("Reads a file's content with line numbers. Returns at most 2000 lines. Use offset and limit to read specific line ranges of large files.".to_string()),
        parameters: Some(obj(
            vec![
                ("path", p("string", "Path to the file")),
                ("offset", p("integer", "Line number to start reading from (1-based). Defaults to 1.")),
                ("limit", p("integer", "Maximum number of lines to return. Defaults to 2000.")),
            ],
            vec!["path"],
        )),
    }
}

const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: ReadFileArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;

    let validated_path = match host.validate_path(&args.path) {
        Ok(p) => p,
        Err(e) => return Ok(format!("Error: {}", e)),
    };

    let metadata = match std::fs::metadata(&validated_path) {
        Ok(m) => m,
        Err(e) => return Ok(format!("Error reading file: {}", e)),
    };
    if metadata.len() > MAX_FILE_SIZE {
        return Ok(format!(
            "Error: file is too large ({:.1} MB). Maximum supported size is {:.0} MB.",
            metadata.len() as f64 / (1024.0 * 1024.0),
            MAX_FILE_SIZE as f64 / (1024.0 * 1024.0)
        ));
    }

    let content = match std::fs::read_to_string(&validated_path) {
        Ok(c) => c,
        Err(e) => return Ok(format!("Error reading file: {}", e)),
    };

    let total_lines = content.lines().count();
    let offset = args.offset.unwrap_or(1).max(1);
    let limit = args.limit.unwrap_or(2000);

    let selected: Vec<String> = content
        .lines()
        .enumerate()
        .skip(offset - 1)
        .take(limit)
        .map(|(i, line)| format!("{:>6}\t{}", i + 1, line))
        .collect();

    let mut result = selected.join("\n");
    let last_shown = (offset - 1 + selected.len()).min(total_lines);

    // Log the actual slice returned so paged reads of a large file are
    // distinguishable in the transcript (the model issues one call per range).
    if !host.quiet() {
        if args.offset.is_some() || args.limit.is_some() {
            println!(
                "{} Reading file: {} {}",
                "📖 ".bright_cyan(),
                args.path.bold().bright_cyan(),
                format!("(lines {}–{})", offset, last_shown).dimmed()
            );
        } else {
            println!(
                "{} Reading file: {}",
                "📖 ".bright_cyan(),
                args.path.bold().bright_cyan()
            );
        }
    }

    if last_shown < total_lines {
        result.push_str(&format!(
            "\n\n... ({} more lines not shown. Use offset={} to continue reading.)",
            total_lines - last_shown,
            last_shown + 1
        ));
    }
    Ok(result)
}
