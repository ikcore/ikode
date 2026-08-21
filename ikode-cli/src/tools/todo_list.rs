use super::obj;
use crate::tools::{render_todos, ToolHost};
use anyhow::Result;
use gaise_core::contracts::GaiseTool;

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "todo_list".to_string(),
        description: Some("Lists all tasks in the todo list.".to_string()),
        parameters: Some(obj(vec![], vec![])),
    }
}

pub async fn execute(host: &mut dyn ToolHost, _arguments: Option<&str>) -> Result<String> {
    Ok(render_todos(host.todos()))
}
