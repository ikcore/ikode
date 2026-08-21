use super::{arr, obj};
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct TodoCompleteArgs {
    pub ids: Vec<usize>,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "todo_complete".to_string(),
        description: Some("Marks tasks as complete by ID.".to_string()),
        parameters: Some(obj(
            vec![("ids", arr("integer", "Array of task IDs (1-based)"))],
            vec!["ids"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: TodoCompleteArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    for id in args.ids {
        if let Some(todo) = host.todos().iter_mut().find(|t| t.id == id) {
            println!(
                "{} Completed task: {}",
                "✅ ".bright_green(),
                todo.task.bright_green()
            );
            todo.completed = true;
        }
    }
    Ok("Tasks marked as complete.".to_string())
}
