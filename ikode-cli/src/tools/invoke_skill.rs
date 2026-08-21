use super::{obj, p};
use crate::skills;
use crate::tools::ToolHost;
use anyhow::Result;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct InvokeSkillArgs {
    pub name: String,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "invoke_skill".to_string(),
        description: Some(
            "Fetch a skill's full instructions by name so you can follow them. This is \
             how you 'use' or 'run' a skill the user refers to — the returned text is a \
             set of instructions you should then carry out. Use list_skills first if you \
             don't know the available names."
                .to_string(),
        ),
        parameters: Some(obj(
            vec![("name", p("string", "Name of the skill to invoke"))],
            vec!["name"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: InvokeSkillArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    match skills::load(host.project_root(), &args.name) {
        Some(s) if !s.body.is_empty() => Ok(format!(
            "Skill '{}' — follow these instructions:\n\n{}",
            s.name, s.body
        )),
        Some(s) => Ok(format!("Skill '{}' has no instructions.", s.name)),
        None => Ok(format!(
            "No skill named '{}'. Call list_skills to see what's available.",
            args.name
        )),
    }
}
