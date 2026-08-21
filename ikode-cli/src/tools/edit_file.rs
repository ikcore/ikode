use super::{match_line_endings, obj, p};
use crate::harness;
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;
use std::io::{self, Write};

#[derive(Deserialize)]
pub struct EditFileArgs {
    pub path: String,
    pub old_text: String,
    pub new_text: String,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "edit_file".to_string(),
        description: Some("Performs a search-and-replace edit on an existing file. The old_text must match exactly (including whitespace and indentation). For multiple edits to the same file, call this tool multiple times.".to_string()),
        parameters: Some(obj(
            vec![
                ("path", p("string", "Path to the file to edit")),
                ("old_text", p("string", "The exact text to find and replace. Must match the file content exactly.")),
                ("new_text", p("string", "The replacement text.")),
            ],
            vec!["path", "old_text", "new_text"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: EditFileArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    println!(
        "{} Editing file: {}",
        "✍️".bright_yellow(),
        args.path.bold().bright_yellow()
    );

    let validated_path = match host.validate_path(&args.path) {
        Ok(p) => p,
        Err(e) => return Ok(format!("Error: {}", e)),
    };
    let content = match std::fs::read_to_string(&validated_path) {
        Ok(c) => c,
        Err(e) => return Ok(format!("Error reading file: {}", e)),
    };

    // Match the file's line endings so a model's LF-only old_text still matches a CRLF
    // file (the common Windows case), and the rewrite keeps the file's ending style.
    let crlf = content.contains("\r\n");
    let old_text = match_line_endings(&args.old_text, crlf);
    let new_text = match_line_endings(&args.new_text, crlf);

    let count = content.matches(&old_text).count();
    if count == 0 {
        // Surface the miss on the console too — otherwise the user just sees "Editing
        // file" with no diff and no idea the edit silently didn't apply.
        println!(
            "{} old_text not found in {} — no change made.",
            "⚠️ ".yellow(),
            args.path.yellow()
        );
        return Ok("Error: old_text not found in file. Make sure it matches exactly, including whitespace and indentation.".to_string());
    }
    if count > 1 {
        println!(
            "{} old_text matches {} places in {} — no change made (needs more context).",
            "⚠️ ".yellow(),
            count,
            args.path.yellow()
        );
        return Ok(format!("Error: old_text matches {} locations in the file. Provide more surrounding context to make the match unique.", count));
    }

    // Show the change as a coloured, line-numbered diff before asking.
    print!(
        "{}",
        harness::render_file_diff(&args.path, &content, &old_text, &new_text)
    );
    let _ = io::stdout().flush();

    let permission_path = host.permission_path(&validated_path);
    if !host.ask_permission(
        "edit_file",
        &permission_path,
        &format!("Edit file {}", permission_path),
    )? {
        return Ok("File edit not applied (denied or cancelled). If in planning mode, switch with /mode agentic.".to_string());
    }

    let new_content = content.replacen(&old_text, &new_text, 1);
    match std::fs::write(&validated_path, &new_content) {
        Ok(_) => {
            host.reindex_path(&validated_path);
            Ok("File updated successfully.".to_string())
        }
        Err(e) => Ok(format!("Error writing file: {}", e)),
    }
}
