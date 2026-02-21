use gaise_core::contracts::{GaiseTool, GaiseToolParameter};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct TodoAddArgs {
    pub tasks: Vec<String>,
}

#[derive(Deserialize)]
pub struct TodoInsertArgs {
    pub before_id: usize,
    pub task: String,
}

#[derive(Deserialize)]
pub struct TodoCompleteArgs {
    pub ids: Vec<usize>,
}

#[derive(Deserialize)]
pub struct ExecuteCommandArgs {
    pub command: String,
}

#[derive(Deserialize)]
pub struct ReadFileArgs {
    pub path: String,
}

#[derive(Deserialize)]
pub struct EditFileArgs {
    pub path: String,
    pub content: String,
}

pub fn get_tools() -> Vec<GaiseTool> {
    vec![
        GaiseTool {
            name: "todo_add".to_string(),
            description: Some("Adds items to the todo list.".to_string()),
            parameters: Some(GaiseToolParameter {
                r#type: Some("object".to_string()),
                description: None,
                properties: Some({
                    let mut p = HashMap::new();
                    p.insert("tasks".to_string(), GaiseToolParameter {
                        r#type: Some("array".to_string()),
                        description: Some("Array of task descriptions".to_string()),
                        properties: None,
                        items: Some(Box::new(GaiseToolParameter {
                            r#type: Some("string".to_string()),
                            ..Default::default()
                        })),
                        required: None,
                        ..Default::default()
                    });
                    p
                }),
                required: Some(vec!["tasks".to_string()]),
                ..Default::default()
            }),
        },
        GaiseTool {
            name: "todo_insert".to_string(),
            description: Some("Insert a task before another task in the todo list. Returns the updated list.".to_string()),
            parameters: Some(GaiseToolParameter {
                r#type: Some("object".to_string()),
                description: None,
                properties: Some({
                    let mut p = HashMap::new();
                    p.insert("before_id".to_string(), GaiseToolParameter {
                        r#type: Some("integer".to_string()),
                        description: Some("The ID of the task before which to insert the new task.".to_string()),
                        ..Default::default()
                    });
                    p.insert("task".to_string(), GaiseToolParameter {
                        r#type: Some("string".to_string()),
                        description: Some("The task to insert.".to_string()),
                        ..Default::default()
                    });
                    p
                }),
                required: Some(vec!["before_id".to_string(), "task".to_string()]),
                ..Default::default()
            }),
        },
        GaiseTool {
            name: "todo_complete".to_string(),
            description: Some("Marks tasks as complete by ID.".to_string()),
            parameters: Some(GaiseToolParameter {
                r#type: Some("object".to_string()),
                description: None,
                properties: Some({
                    let mut p = HashMap::new();
                    p.insert("ids".to_string(), GaiseToolParameter {
                        r#type: Some("array".to_string()),
                        description: Some("Array of task IDs (1-based)".to_string()),
                        properties: None,
                        items: Some(Box::new(GaiseToolParameter {
                            r#type: Some("integer".to_string()),
                            ..Default::default()
                        })),
                        required: None,
                        ..Default::default()
                    });
                    p
                }),
                required: Some(vec!["ids".to_string()]),
                ..Default::default()
            }),
        },
        GaiseTool {
            name: "todo_list".to_string(),
            description: Some("Lists all tasks in the todo list.".to_string()),
            parameters: Some(GaiseToolParameter {
                r#type: Some("object".to_string()),
                description: None,
                properties: Some(HashMap::new()),
                required: None,
                ..Default::default()
            }),
        },
        GaiseTool {
            name: "execute_command".to_string(),
            description: Some("Executes a shell command.".to_string()),
            parameters: Some(GaiseToolParameter {
                r#type: Some("object".to_string()),
                description: None,
                properties: Some({
                    let mut p = HashMap::new();
                    p.insert("command".to_string(), GaiseToolParameter {
                        r#type: Some("string".to_string()),
                        description: Some("The command to execute".to_string()),
                        properties: None,
                        required: None,
                        ..Default::default()
                    });
                    p
                }),
                required: Some(vec!["command".to_string()]),
                ..Default::default()
            }),
        },
        GaiseTool {
            name: "read_file".to_string(),
            description: Some("Reads a file's content.".to_string()),
            parameters: Some(GaiseToolParameter {
                r#type: Some("object".to_string()),
                description: None,
                properties: Some({
                    let mut p = HashMap::new();
                    p.insert("path".to_string(), GaiseToolParameter {
                        r#type: Some("string".to_string()),
                        description: Some("Path to the file".to_string()),
                        properties: None,
                        required: None,
                        ..Default::default()
                    });
                    p
                }),
                required: Some(vec!["path".to_string()]),
                ..Default::default()
            }),
        },
        GaiseTool {
            name: "edit_file".to_string(),
            description: Some("Edits a file with new content.".to_string()),
            parameters: Some(GaiseToolParameter {
                r#type: Some("object".to_string()),
                description: None,
                properties: Some({
                    let mut p = HashMap::new();
                    p.insert("path".to_string(), GaiseToolParameter {
                        r#type: Some("string".to_string()),
                        description: Some("Path to the file".to_string()),
                        properties: None,
                        required: None,
                        ..Default::default()
                    });
                    p.insert("content".to_string(), GaiseToolParameter {
                        r#type: Some("string".to_string()),
                        description: Some("New full content of the file".to_string()),
                        properties: None,
                        required: None,
                        ..Default::default()
                    });
                    p
                }),
                required: Some(vec!["path".to_string(), "content".to_string()]),
                ..Default::default()
            }),
        },
    ]
}
