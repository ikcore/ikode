use super::obj;
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "todo_clear".to_string(),
        description: Some(
            "Removes every task from the todo list, completed or pending. Use when the \
             current plan is finished or abandoned and you want a clean slate."
                .to_string(),
        ),
        parameters: Some(obj(vec![], vec![])),
    }
}

pub async fn execute(host: &mut dyn ToolHost, _arguments: Option<&str>) -> Result<String> {
    let count = host.todos().len();
    host.todos().clear();
    if count == 0 {
        println!("{} Todo list already empty.", "•".dimmed());
        Ok("The todo list was already empty.".to_string())
    } else {
        println!(
            "{} Cleared {} task(s) from the todo list.",
            "🧹".bright_green(),
            count.to_string().bright_magenta().bold()
        );
        Ok(format!(
            "Cleared {count} task(s); the todo list is now empty."
        ))
    }
}
