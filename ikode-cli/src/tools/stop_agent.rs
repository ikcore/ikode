use super::{obj, p};
use crate::tools::ToolHost;
use anyhow::Result;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct StopAgentArgs {
    pub id: String,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "stop_agent".to_string(),
        description: Some(
            "Stop one running subagent, or all running subagents with id='all'. Partial output remains inspectable."
                .to_string(),
        ),
        parameters: Some(obj(
            vec![("id", p("string", "Agent id/name, or 'all'"))],
            vec!["id"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: StopAgentArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    host.stop_agent(args.id.trim())
}
