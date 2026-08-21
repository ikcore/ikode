//! `task_complete` — the goal loop's stop signal. The model declares the
//! acceptance criteria met and supplies a short summary of what was done. The loop
//! in `turn.rs` detects this call, marks the task `Done`, and stops cleanly — a
//! reliable termination signal instead of guessing completion from prose.

use super::{obj, p, ToolHost};
use anyhow::Result;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct TaskCompleteArgs {
    /// A short summary of what was accomplished, for the user.
    pub summary: String,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "task_complete".to_string(),
        description: Some(
            "Declare the active goal complete: every acceptance criterion is met. \
             Provide a concise summary of what you did. Only call this when the work \
             is genuinely finished — it ends the autonomous loop."
                .to_string(),
        ),
        parameters: Some(obj(
            vec![(
                "summary",
                p("string", "Concise summary of the completed work"),
            )],
            vec!["summary"],
        )),
    }
}

pub async fn execute(_host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: TaskCompleteArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    Ok(format!("Goal marked complete: {}", args.summary))
}
