use super::{obj, p};
use crate::harness;
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;
use std::io::{self, Write};

#[derive(Deserialize)]
pub struct CreateFileArgs {
    pub path: String,
    pub content: String,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "create_file".to_string(),
        description: Some(
            "Creates a new file with the given content. Fails if the file already exists."
                .to_string(),
        ),
        parameters: Some(obj(
            vec![
                ("path", p("string", "Path for the new file")),
                ("content", p("string", "Content of the new file")),
            ],
            vec!["path", "content"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: CreateFileArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    println!(
        "{} Creating file: {}",
        "📄".bright_green(),
        args.path.bold().bright_green()
    );

    let validated_path = match host.validate_path(&args.path) {
        Ok(p) => p,
        Err(e) => return Ok(format!("Error: {}", e)),
    };
    if validated_path.exists() {
        println!(
            "{} {} already exists — no file created (use edit_file to modify it).",
            "⚠️ ".yellow(),
            args.path.yellow()
        );
        return Ok(format!(
            "Error: file '{}' already exists. Use edit_file to modify existing files.",
            args.path
        ));
    }

    // Preview the new file as a coloured, line-numbered diff (capped).
    print!(
        "{}",
        harness::render_new_file(&args.path, &args.content, 40)
    );
    let _ = io::stdout().flush();

    let permission_path = host.permission_path(&validated_path);
    if !host.ask_permission(
        "create_file",
        &permission_path,
        &format!("Create file {}", permission_path),
    )? {
        return Ok("File not created (denied or cancelled). If in planning mode, switch with /mode agentic.".to_string());
    }

    if let Some(parent) = validated_path.parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Ok(format!("Error creating directories: {}", e));
            }
        }
    }

    // Directory creation can change which path components are canonicalisable.
    // Validate again before opening the file so a newly-visible traversal/symlink
    // cannot turn the approved target into an out-of-project write.
    let validated_path = match host.validate_path(&args.path) {
        Ok(p) => p,
        Err(e) => return Ok(format!("Error: {}", e)),
    };

    match std::fs::write(&validated_path, &args.content) {
        Ok(_) => {
            host.reindex_path(&validated_path);
            Ok("File created successfully.".to_string())
        }
        Err(e) => Ok(format!("Error creating file: {}", e)),
    }
}
