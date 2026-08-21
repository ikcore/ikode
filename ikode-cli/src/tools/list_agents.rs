use super::obj;
use crate::tools::ToolHost;
use anyhow::Result;
use gaise_core::contracts::GaiseTool;

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "list_agents".to_string(),
        description: Some(
            "List every subagent thread with its id, name, task, state, model, and effort."
                .to_string(),
        ),
        parameters: Some(obj(vec![], vec![])),
    }
}

pub async fn execute(host: &mut dyn ToolHost, _arguments: Option<&str>) -> Result<String> {
    Ok(host.list_agents())
}
