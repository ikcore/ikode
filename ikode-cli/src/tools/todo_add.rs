use super::{arr, obj};
use crate::tools::{Todo, ToolHost};
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct TodoAddArgs {
    pub tasks: Vec<String>,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "todo_add".to_string(),
        description: Some("Adds items to the todo list.".to_string()),
        parameters: Some(obj(
            vec![("tasks", arr("string", "Array of task descriptions"))],
            vec!["tasks"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: TodoAddArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    for task in args.tasks {
        println!(
            "{} Adding task: {}",
            "📝 ".bright_blue(),
            task.bright_blue()
        );
        let id = host.todos().len() + 1;
        host.todos().push(Todo {
            id,
            task,
            completed: false,
        });
    }
    Ok("Tasks added.".to_string())
}
