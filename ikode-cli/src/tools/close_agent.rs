use super::{obj, p};
use crate::tools::ToolHost;
use anyhow::Result;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CloseAgentArgs {
    pub id: String,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "close_agent".to_string(),
        description: Some(
            "Close and remove a finished/stopped subagent thread, or all terminal threads with id='all'. Running agents must be stopped first."
                .to_string(),
        ),
        parameters: Some(obj(
            vec![("id", p("string", "Agent id/name, or 'all'"))],
            vec!["id"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: CloseAgentArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    host.close_agent(args.id.trim())
}
