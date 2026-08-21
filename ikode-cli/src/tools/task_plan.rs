//! `task_plan` — the model records (or revises) the active goal's acceptance
//! criteria and an optional step breakdown. Bridges a goal to the per-step
//! `todo_*` tools, which remain the in-task tracker.
//!
//! The actual state change (writing `acceptance` onto the active task, seeding the
//! steps as todos) is applied by the goal loop in `turn.rs`, which inspects the raw
//! tool call — so this `execute` is only the dispatch-table fallback used if the
//! tool is somehow invoked outside a goal run. Inside a goal it never reaches here.

use super::{arr, obj, ToolHost};
use anyhow::Result;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct TaskPlanArgs {
    /// The definition of done: each entry one concrete, checkable criterion.
    pub acceptance: Vec<String>,
    /// Optional ordered steps to reach it; seeded as the task's todo list.
    #[serde(default)]
    pub steps: Vec<String>,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "task_plan".to_string(),
        description: Some(
            "Set or revise the acceptance criteria (definition of done) for the active goal, \
             and optionally an ordered list of steps. Call this first, before starting work, \
             and again whenever your plan changes."
                .to_string(),
        ),
        parameters: Some(obj(
            vec![
                (
                    "acceptance",
                    arr("string", "Concrete, checkable criteria that define 'done'"),
                ),
                (
                    "steps",
                    arr("string", "Optional ordered steps to reach the goal"),
                ),
            ],
            vec!["acceptance"],
        )),
    }
}

pub async fn execute(_host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: TaskPlanArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    Ok(format!(
        "Plan recorded: {} acceptance criteria, {} steps.",
        args.acceptance.len(),
        args.steps.len()
    ))
}
