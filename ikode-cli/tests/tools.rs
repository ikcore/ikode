//! Per-tool tests. Each tool has its own file under `tests/tools/`, mirroring the
//! `src/tools/` layout; this crate aggregates them and provides shared helpers.
//! Tools are validated through the public `get_tools` surface (the per-tool
//! modules are private) plus their public argument structs.

use gaise_core::contracts::GaiseTool;
use ikode::settings::Mode;
use ikode::tools;

/// Find a tool by name in the agentic-mode list (where every tool is present).
pub fn tool(name: &str) -> GaiseTool {
    tools::get_tools(Mode::Agentic, false)
        .into_iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("tool `{name}` not found in get_tools"))
}

/// The declared property names of a tool's object schema.
pub fn param_keys(t: &GaiseTool) -> Vec<String> {
    t.parameters
        .as_ref()
        .and_then(|p| p.properties.as_ref())
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// A tool's required-parameter list (empty if none).
pub fn required(t: &GaiseTool) -> Vec<String> {
    t.parameters
        .as_ref()
        .and_then(|p| p.required.clone())
        .unwrap_or_default()
}

/// Does the tool carry a non-empty description (the model relies on these)?
pub fn has_description(t: &GaiseTool) -> bool {
    t.description
        .as_ref()
        .map(|d| !d.is_empty())
        .unwrap_or(false)
}

#[path = "tools/ask_codebase_tests.rs"]
mod ask_codebase_tests;
#[path = "tools/create_file_tests.rs"]
mod create_file_tests;
#[path = "tools/delete_file_tests.rs"]
mod delete_file_tests;
#[path = "tools/edit_chunk_tests.rs"]
mod edit_chunk_tests;
#[path = "tools/edit_file_tests.rs"]
mod edit_file_tests;
#[path = "tools/execute_command_tests.rs"]
mod execute_command_tests;
#[path = "tools/find_references_tests.rs"]
mod find_references_tests;
#[path = "tools/graph_overview_tests.rs"]
mod graph_overview_tests;
#[path = "tools/index_codebase_tests.rs"]
mod index_codebase_tests;
#[path = "tools/list_directory_tests.rs"]
mod list_directory_tests;
#[path = "tools/outline_file_tests.rs"]
mod outline_file_tests;
#[path = "tools/read_file_tests.rs"]
mod read_file_tests;
#[path = "tools/search_code_tests.rs"]
mod search_code_tests;
#[path = "tools/todo_add_tests.rs"]
mod todo_add_tests;
#[path = "tools/todo_clear_tests.rs"]
mod todo_clear_tests;
#[path = "tools/todo_complete_tests.rs"]
mod todo_complete_tests;
#[path = "tools/todo_insert_tests.rs"]
mod todo_insert_tests;
#[path = "tools/todo_list_tests.rs"]
mod todo_list_tests;
#[path = "tools/update_settings_tests.rs"]
mod update_settings_tests;

/// Every tool the model can see must have a non-empty description and an object
/// parameter schema â€” a cross-cutting guard so a new tool can't ship malformed.
#[test]
fn all_tools_are_well_formed() {
    let all = tools::get_tools(Mode::Agentic, false);
    let unique: std::collections::HashSet<&str> = all.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(unique.len(), all.len(), "tool names must be unique");
    assert!(!all.is_empty(), "the registry must expose tools");
    for t in &all {
        assert!(has_description(t), "{} has no description", t.name);
        let schema = t.parameters.as_ref().expect("tool has parameters");
        assert_eq!(
            schema.r#type.as_deref(),
            Some("object"),
            "{} schema",
            t.name
        );
    }
}
