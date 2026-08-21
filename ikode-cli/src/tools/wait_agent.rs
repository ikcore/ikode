use super::{obj, p};
use crate::tools::ToolHost;
use anyhow::Result;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct WaitAgentArgs {
    pub id: Option<String>,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "wait_agent".to_string(),
        description: Some(
            "Wait for one subagent (or all currently open subagents when id is omitted or 'all') and return completed results to the lead. Model waits poll in bounded 30-second slices, so call again if workers are still running."
                .to_string(),
        ),
        parameters: Some(obj(
            vec![("id", p("string", "Optional agent id/name; omit or use 'all' to wait for all"))],
            vec![],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: WaitAgentArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    host.wait_agent(args.id.as_deref().map(str::trim)).await
}
