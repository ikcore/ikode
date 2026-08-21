//! `task_block` — a clean "stuck, need input" pause. The model calls this when it
//! cannot make progress without a decision, missing information, or access it does
//! not have. The goal loop in `turn.rs` detects it, marks the task `Blocked`, and
//! returns to the prompt with the reason — the task stays persisted and resumable.

use super::{obj, p, ToolHost};
use anyhow::Result;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct TaskBlockArgs {
    /// Why progress is blocked and what input is needed to continue.
    pub reason: String,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "task_block".to_string(),
        description: Some(
            "Pause the active goal because you are blocked and need user input: a decision, \
             missing information, or access you do not have. Explain clearly what you need. \
             Prefer this over guessing when you genuinely cannot proceed."
                .to_string(),
        ),
        parameters: Some(obj(
            vec![(
                "reason",
                p("string", "What blocks progress and what input is needed"),
            )],
            vec!["reason"],
        )),
    }
}

pub async fn execute(_host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: TaskBlockArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    Ok(format!("Goal blocked: {}", args.reason))
}
