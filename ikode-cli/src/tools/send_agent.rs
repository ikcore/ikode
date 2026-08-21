use super::{obj, p};
use crate::tools::ToolHost;
use anyhow::Result;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SendAgentArgs {
    pub id: String,
    pub message: String,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "send_agent".to_string(),
        description: Some(
            "Steer a running subagent with additional instructions. The message is delivered at the next safe model boundary without discarding completed work."
                .to_string(),
        ),
        parameters: Some(obj(
            vec![
                ("id", p("string", "Agent id, unique id prefix, or exact name")),
                ("message", p("string", "Follow-up instruction or correction")),
            ],
            vec!["id", "message"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: SendAgentArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    host.send_agent(args.id.trim(), args.message.trim())
}
