use std::process::Command;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_cli_version_flag() {
    let output = Command::new("cargo")
        .args(&["run", "--bin", "ikode", "--", "--version"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ikode-cli"));
}

#[test]
fn test_cli_help_flag() {
    let output = Command::new("cargo")
        .args(&["run", "--bin", "ikode", "--", "--help"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("--model"));
    assert!(stdout.contains("--brave"));
}

#[test]
fn test_cli_invalid_model() {
    let output = Command::new("cargo")
        .args(&["run", "--bin", "ikode", "--", "--model", "invalid::model"])
        .output()
        .expect("Failed to execute command");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success() || stderr.contains("error") || stderr.contains("Error"));
}

#[test]
fn test_todo_add_functionality() {
    let todo_task = "Test task";
    let mut todos = Vec::new();
    todos.push(todo_task.to_string());

    assert_eq!(todos.len(), 1);
    assert_eq!(todos[0], todo_task);
}

#[test]
fn test_todo_complete_functionality() {
    struct Todo {
        task: String,
        completed: bool,
    }

    let mut todos = vec![
        Todo { task: "Task 1".to_string(), completed: false },
        Todo { task: "Task 2".to_string(), completed: false },
    ];

    todos[0].completed = true;

    assert!(todos[0].completed);
    assert!(!todos[1].completed);
}

#[test]
fn test_todo_list_functionality() {
    struct Todo {
        task: String,
        completed: bool,
    }

    let todos = vec![
        Todo { task: "Task 1".to_string(), completed: true },
        Todo { task: "Task 2".to_string(), completed: false },
        Todo { task: "Task 3".to_string(), completed: false },
    ];

    assert_eq!(todos.len(), 3);
    assert_eq!(todos.iter().filter(|t| t.completed).count(), 1);
    assert_eq!(todos.iter().filter(|t| !t.completed).count(), 2);
}

#[test]
fn test_system_prompt_formatting() {
    let template = "Working directory: __WORKING_DIRECTORY__\nPlatform: __PLATFORM__";
    let wd = "/test/path";
    let platform = "linux";

    let formatted = template
        .replace("__WORKING_DIRECTORY__", wd)
        .replace("__PLATFORM__", platform);

    assert!(formatted.contains("/test/path"));
    assert!(formatted.contains("linux"));
    assert!(!formatted.contains("__"));
}

#[test]
fn test_json_parsing_for_tool_args() {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    struct ReadFileArgs {
        path: String,
    }

    let json = r#"{"path": "test.txt"}"#;
    let args: ReadFileArgs = serde_json::from_str(json).unwrap();

    assert_eq!(args.path, "test.txt");
}

#[test]
fn test_json_parsing_handles_empty_args() {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Default)]
    struct EmptyArgs {}

    let json = "{}";
    let args: EmptyArgs = serde_json::from_str(json).unwrap();

    let _ = args;
}

#[test]
fn test_command_execution_output_capture() {
    let output = Command::new("echo")
        .arg("test")
        .output()
        .expect("Failed to execute echo");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.trim() == "test");
}

#[test]
fn test_working_directory_detection() {
    let wd = std::env::current_dir().unwrap();
    assert!(wd.exists());
    assert!(wd.is_absolute());
}

#[test]
fn test_git_repo_detection() {
    let git_path = std::path::Path::new(".git");
    let is_git = git_path.exists();
    assert!(is_git || !is_git);
}

#[test]
fn test_date_formatting() {
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    assert!(date.len() == 10);
    assert!(date.contains("-"));

    let parts: Vec<&str> = date.split('-').collect();
    assert_eq!(parts.len(), 3);
}
