use super::{obj, p};
use crate::settings::Effort;
use crate::tools::ToolHost;
use anyhow::Result;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SpawnAgentArgs {
    pub task: String,
    pub name: Option<String>,
    pub effort: Option<String>,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "spawn_agent".to_string(),
        description: Some(
            "Spawn a bounded read-only subagent thread for an independent exploration, review, or analysis task. The worker has its own context and tools, cannot edit files or spawn children, and returns a result to the lead. Prefer parallel agents for independent read-heavy work; keep sequential and write-heavy work with the lead."
                .to_string(),
        ),
        parameters: Some(obj(
            vec![
                ("task", p("string", "Concrete bounded task and the evidence/result to return")),
                ("name", p("string", "Optional short display name, such as security or tests")),
                ("effort", p("string", "Optional worker effort: auto, low, medium, high, max, or ultra")),
            ],
            vec!["task"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: SpawnAgentArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    if args.task.trim().is_empty() {
        return Ok("Error: agent task cannot be empty.".to_string());
    }
    if let Some(raw) = args.effort.as_deref() {
        if Effort::parse(raw).is_none() {
            return Ok(format!(
                "Error: unknown effort '{raw}'; use auto, low, medium, high, max, or ultra."
            ));
        }
    }
    host.spawn_agent(
        args.task.trim(),
        args.name.as_deref().map(str::trim),
        args.effort.as_deref(),
    )
}
