use super::{obj, p};
use crate::tools::{render_todos, Todo, ToolHost};
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct TodoInsertArgs {
    pub before_id: usize,
    pub task: String,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "todo_insert".to_string(),
        description: Some(
            "Insert a task before another task in the todo list. Returns the updated list."
                .to_string(),
        ),
        parameters: Some(obj(
            vec![
                (
                    "before_id",
                    p(
                        "integer",
                        "The ID of the task before which to insert the new task.",
                    ),
                ),
                ("task", p("string", "The task to insert.")),
            ],
            vec!["before_id", "task"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: TodoInsertArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    let todos = host.todos();
    let index = todos
        .iter()
        .position(|t| t.id == args.before_id)
        .unwrap_or(todos.len());
    println!(
        "{} Inserting task: {} before ID {}",
        "📝 ".bright_blue(),
        args.task.bright_blue(),
        args.before_id
    );
    todos.insert(
        index,
        Todo {
            id: 0,
            task: args.task,
            completed: false,
        },
    );
    for (i, todo) in todos.iter_mut().enumerate() {
        todo.id = i + 1;
    }
    Ok(render_todos(host.todos()))
}
