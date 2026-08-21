use super::{obj, p};
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct DeleteFileArgs {
    pub path: String,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "delete_file".to_string(),
        description: Some("Deletes an existing file from the working directory and prunes it from the code graph. Prompts for confirmation. Use this instead of an `rm`/`del` shell command so the index stays consistent.".to_string()),
        parameters: Some(obj(
            vec![("path", p("string", "Path to the file to delete"))],
            vec!["path"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: DeleteFileArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    println!(
        "{} Deleting file: {}",
        "🗑️ ".bright_red(),
        args.path.bold().bright_red()
    );

    let validated_path = match host.validate_path(&args.path) {
        Ok(p) => p,
        Err(e) => return Ok(format!("Error: {}", e)),
    };
    if !validated_path.exists() {
        return Ok(format!("Error: file '{}' does not exist.", args.path));
    }
    if validated_path.is_dir() {
        return Ok(format!(
            "Error: '{}' is a directory; delete_file only removes files.",
            args.path
        ));
    }
    let permission_path = host.permission_path(&validated_path);
    if !host.ask_permission(
        "delete_file",
        &permission_path,
        &format!("Delete file {}", permission_path),
    )? {
        return Ok("File not deleted (denied or cancelled). If in planning mode, switch with /mode agentic.".to_string());
    }
    match std::fs::remove_file(&validated_path) {
        Ok(_) => {
            // Keep the graph/index consistent with disk.
            host.indexer().remove_path(&validated_path);
            host.mark_workspace_changed();
            Ok("File deleted successfully.".to_string())
        }
        Err(e) => Ok(format!("Error deleting file: {}", e)),
    }
}
