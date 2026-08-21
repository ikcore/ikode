use super::obj;
use crate::skills;
use crate::tools::ToolHost;
use anyhow::Result;
use gaise_core::contracts::GaiseTool;

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "list_skills".to_string(),
        description: Some(
            "List the project's reusable skills (markdown instruction files in \
             .ikode/skills/), returning each skill's name and description. Use \
             invoke_skill to fetch and follow a specific skill's instructions."
                .to_string(),
        ),
        parameters: Some(obj(vec![], vec![])),
    }
}

pub async fn execute(host: &mut dyn ToolHost, _arguments: Option<&str>) -> Result<String> {
    let all = skills::list(host.project_root());
    if all.is_empty() {
        return Ok(
            "No skills defined. Skills are markdown files in .ikode/skills/<name>.md; \
             create one with create_file."
                .to_string(),
        );
    }
    let mut out = String::from("Available skills:\n");
    for s in &all {
        out.push_str(&format!("- {}: {}\n", s.name, s.description));
    }
    out.push_str("\nCall invoke_skill with a skill's name to get its full instructions.");
    Ok(out)
}
