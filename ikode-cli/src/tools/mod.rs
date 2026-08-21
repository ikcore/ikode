//! The tool surface presented to the model. There is one file per tool, each
//! defining that tool's argument struct (if any) and a `spec()` returning its
//! [`GaiseTool`] schema. This module aggregates them and applies the permission
//! mode: in [`crate::settings::Mode::Plan`] mutating tools are withheld entirely.
//! Tool schema, mutability, goal availability, and dispatch identity all come from
//! the registry in this module so those concerns cannot drift apart.
//!
//! All tools are pure-algorithmic (graph traversal, parsing, file IO) and make
//! **zero** model calls — iKode's "minimise inference" principle. The only model
//! calls iKode makes are the opt-in enrichment passes and the chat turn itself;
//! their system prompts live in [`crate::prompts`].

use gaise_core::contracts::{GaiseTool, GaiseToolParameter};

mod host;
pub use host::{render_todos, Todo, ToolHost};

mod ask_codebase;
mod close_agent;
mod create_file;
mod delete_file;
mod edit_chunk;
mod edit_file;
mod execute_command;
mod find_references;
mod graph_overview;
mod graph_query;
mod index_codebase;
mod invoke_skill;
mod list_agents;
mod list_directory;
mod list_skills;
mod outline_file;
mod read_file;
mod search_code;
mod send_agent;
mod spawn_agent;
mod stop_agent;
mod task_block;
mod task_complete;
mod task_plan;
mod todo_add;
mod todo_clear;
mod todo_complete;
mod todo_insert;
mod todo_list;
mod update_settings;
mod wait_agent;
mod web_fetch;
mod web_research;
mod web_search;

// Re-export every argument type so callers keep using `ikode::tools::*`.
pub use ask_codebase::AskCodebaseArgs;
pub use create_file::CreateFileArgs;
pub use delete_file::DeleteFileArgs;
pub use edit_chunk::EditChunkArgs;
pub use edit_file::EditFileArgs;
pub use execute_command::ExecuteCommandArgs;
pub use find_references::FindReferencesArgs;
pub use invoke_skill::InvokeSkillArgs;
pub use list_directory::ListDirectoryArgs;
pub use outline_file::OutlineFileArgs;
pub use read_file::ReadFileArgs;
pub use search_code::SearchCodeArgs;
pub use task_block::TaskBlockArgs;
pub use task_complete::TaskCompleteArgs;
pub use task_plan::TaskPlanArgs;
pub use todo_add::TodoAddArgs;
pub use todo_complete::TodoCompleteArgs;
pub use todo_insert::TodoInsertArgs;
pub use update_settings::UpdateSettingsArgs;
pub use web_fetch::WebFetchArgs;
pub use web_research::WebResearchArgs;
pub use web_search::WebSearchArgs;

#[derive(Clone, Copy)]
enum ToolKind {
    TodoAdd,
    TodoInsert,
    TodoComplete,
    TodoClear,
    TodoList,
    ExecuteCommand,
    ReadFile,
    EditFile,
    EditChunk,
    CreateFile,
    DeleteFile,
    ListDirectory,
    IndexCodebase,
    SearchCode,
    OutlineFile,
    AskCodebase,
    FindReferences,
    GraphOverview,
    GraphQuery,
    UpdateSettings,
    ListSkills,
    InvokeSkill,
    TaskPlan,
    TaskComplete,
    TaskBlock,
    SpawnAgent,
    ListAgents,
    SendAgent,
    WaitAgent,
    StopAgent,
    CloseAgent,
    WebResearch,
    WebSearch,
    WebFetch,
}

/// Where a tool sits in the web-research split: `Lead` tools are offered to the
/// lead only when a search backend is configured; `Leaf` tools exist solely
/// inside a spawned web agent and are never presented to the lead at all.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WebRole {
    None,
    Lead,
    Leaf,
}

struct ToolDefinition {
    kind: ToolKind,
    spec: fn() -> GaiseTool,
    mutating: bool,
    goal_only: bool,
    graph_required: bool,
    web: WebRole,
}

macro_rules! tool {
    ($kind:ident, $module:ident, $mutating:literal, $goal_only:literal) => {
        ToolDefinition {
            kind: ToolKind::$kind,
            spec: $module::spec,
            mutating: $mutating,
            goal_only: $goal_only,
            graph_required: false,
            web: WebRole::None,
        }
    };
    ($kind:ident, $module:ident, $mutating:literal, $goal_only:literal, graph) => {
        ToolDefinition {
            kind: ToolKind::$kind,
            spec: $module::spec,
            mutating: $mutating,
            goal_only: $goal_only,
            graph_required: true,
            web: WebRole::None,
        }
    };
    ($kind:ident, $module:ident, $mutating:literal, $goal_only:literal, web $role:ident) => {
        ToolDefinition {
            kind: ToolKind::$kind,
            spec: $module::spec,
            mutating: $mutating,
            goal_only: $goal_only,
            graph_required: false,
            web: WebRole::$role,
        }
    };
}

/// The single authoritative tool registry. A tool added here automatically gains
/// schema publication, plan-mode filtering, mutability classification, goal
/// availability, and dispatch routing through its [`ToolKind`].
const TOOL_REGISTRY: &[ToolDefinition] = &[
    tool!(TodoAdd, todo_add, false, false),
    tool!(TodoInsert, todo_insert, false, false),
    tool!(TodoComplete, todo_complete, false, false),
    tool!(TodoClear, todo_clear, false, false),
    tool!(TodoList, todo_list, false, false),
    tool!(ExecuteCommand, execute_command, true, false),
    tool!(ReadFile, read_file, false, false),
    tool!(EditFile, edit_file, true, false),
    tool!(EditChunk, edit_chunk, true, false, graph),
    tool!(CreateFile, create_file, true, false),
    tool!(DeleteFile, delete_file, true, false),
    tool!(ListDirectory, list_directory, false, false),
    tool!(IndexCodebase, index_codebase, false, false, graph),
    tool!(SearchCode, search_code, false, false, graph),
    tool!(OutlineFile, outline_file, false, false, graph),
    tool!(AskCodebase, ask_codebase, false, false, graph),
    tool!(FindReferences, find_references, false, false, graph),
    tool!(GraphOverview, graph_overview, false, false, graph),
    tool!(GraphQuery, graph_query, false, false, graph),
    tool!(UpdateSettings, update_settings, true, false),
    tool!(ListSkills, list_skills, false, false),
    tool!(InvokeSkill, invoke_skill, false, false),
    tool!(TaskPlan, task_plan, false, true),
    tool!(TaskComplete, task_complete, false, true),
    tool!(TaskBlock, task_block, false, true),
    tool!(SpawnAgent, spawn_agent, false, false),
    tool!(ListAgents, list_agents, false, false),
    tool!(SendAgent, send_agent, false, false),
    tool!(WaitAgent, wait_agent, false, false),
    tool!(StopAgent, stop_agent, false, false),
    tool!(CloseAgent, close_agent, false, false),
    tool!(WebResearch, web_research, false, false, web Lead),
    tool!(WebSearch, web_search, false, false, web Leaf),
    tool!(WebFetch, web_fetch, false, false, web Leaf),
];

fn definition(name: &str) -> Option<&'static ToolDefinition> {
    TOOL_REGISTRY
        .iter()
        .find(|definition| (definition.spec)().name == name)
}

/// Does invoking `name` change disk or run a shell command?
pub fn is_mutating(name: &str) -> bool {
    definition(name).is_some_and(|definition| definition.mutating)
        || crate::mcp::is_exposed_tool_name(name)
}

/// Whether `name` is present in the model-tool registry.
pub fn is_known(name: &str) -> bool {
    definition(name).is_some() || crate::mcp::is_exposed_tool_name(name)
}

/// Whether a built-in tool depends on the LivingVector graph/index subsystem.
pub fn requires_graph(name: &str) -> bool {
    definition(name).is_some_and(|definition| definition.graph_required)
}

/// Execute a tool call by name against `host`, returning the model-facing result
/// string. Argument parsing lives inside each tool's `execute`, so this is a pure
/// name → module routing table. Unknown names return a message rather than error.
pub async fn dispatch(
    host: &mut dyn ToolHost,
    name: &str,
    arguments: Option<&str>,
) -> anyhow::Result<String> {
    let Some(definition) = definition(name) else {
        return Ok(format!("Unknown tool: {name}"));
    };
    match definition.kind {
        ToolKind::TodoAdd => todo_add::execute(host, arguments).await,
        ToolKind::TodoInsert => todo_insert::execute(host, arguments).await,
        ToolKind::TodoComplete => todo_complete::execute(host, arguments).await,
        ToolKind::TodoClear => todo_clear::execute(host, arguments).await,
        ToolKind::TodoList => todo_list::execute(host, arguments).await,
        ToolKind::ExecuteCommand => execute_command::execute(host, arguments).await,
        ToolKind::ReadFile => read_file::execute(host, arguments).await,
        ToolKind::EditFile => edit_file::execute(host, arguments).await,
        ToolKind::EditChunk => edit_chunk::execute(host, arguments).await,
        ToolKind::CreateFile => create_file::execute(host, arguments).await,
        ToolKind::DeleteFile => delete_file::execute(host, arguments).await,
        ToolKind::ListDirectory => list_directory::execute(host, arguments).await,
        ToolKind::IndexCodebase => index_codebase::execute(host, arguments).await,
        ToolKind::SearchCode => search_code::execute(host, arguments).await,
        ToolKind::OutlineFile => outline_file::execute(host, arguments).await,
        ToolKind::AskCodebase => ask_codebase::execute(host, arguments).await,
        ToolKind::FindReferences => find_references::execute(host, arguments).await,
        ToolKind::GraphOverview => graph_overview::execute(host, arguments).await,
        ToolKind::GraphQuery => graph_query::execute(host, arguments).await,
        ToolKind::UpdateSettings => update_settings::execute(host, arguments).await,
        ToolKind::ListSkills => list_skills::execute(host, arguments).await,
        ToolKind::InvokeSkill => invoke_skill::execute(host, arguments).await,
        // The goal-control tools are normally intercepted by the goal loop in
        // `turn.rs` (which owns the active task). These arms are a safety net for an
        // out-of-goal invocation: they acknowledge without changing any task state.
        ToolKind::TaskPlan => task_plan::execute(host, arguments).await,
        ToolKind::TaskComplete => task_complete::execute(host, arguments).await,
        ToolKind::TaskBlock => task_block::execute(host, arguments).await,
        ToolKind::SpawnAgent => spawn_agent::execute(host, arguments).await,
        ToolKind::ListAgents => list_agents::execute(host, arguments).await,
        ToolKind::SendAgent => send_agent::execute(host, arguments).await,
        ToolKind::WaitAgent => wait_agent::execute(host, arguments).await,
        ToolKind::StopAgent => stop_agent::execute(host, arguments).await,
        ToolKind::CloseAgent => close_agent::execute(host, arguments).await,
        ToolKind::WebResearch => web_research::execute(host, arguments).await,
        ToolKind::WebSearch => web_search::execute(host, arguments).await,
        ToolKind::WebFetch => web_fetch::execute(host, arguments).await,
    }
}

/// Build the tool list presented to the model for the given operating mode. In
/// plan mode the mutating tools (`execute_command`, `edit_file`, `edit_chunk`,
/// `create_file`, `delete_file`) are not offered at all — the capability is removed
/// structurally rather than relying on the model to refrain.
///
/// `in_goal` adds the goal-control tools (`task_plan`/`task_complete`/`task_block`).
/// They are withheld during ordinary interactive chat so the model can't end a
/// non-existent goal — and so the normal tool list (and its cached prefix) is
/// unchanged when no goal is running.
pub fn get_tools(mode: crate::settings::Mode, in_goal: bool) -> Vec<GaiseTool> {
    get_tools_for(mode, in_goal, true)
}

/// Build the model tool list with graph-backed capabilities optionally withheld.
pub fn get_tools_for(
    mode: crate::settings::Mode,
    in_goal: bool,
    graph_enabled: bool,
) -> Vec<GaiseTool> {
    get_tools_with_web(mode, in_goal, graph_enabled, false)
}

/// As [`get_tools_for`], additionally offering `web_research` when a search
/// backend is configured. The web *leaf* tools (`web_search`/`web_fetch`) are
/// never offered to the lead regardless: web access is only reachable through a
/// spawned web agent, keeping untrusted web content away from writing tools.
pub fn get_tools_with_web(
    mode: crate::settings::Mode,
    in_goal: bool,
    graph_enabled: bool,
    web_enabled: bool,
) -> Vec<GaiseTool> {
    TOOL_REGISTRY
        .iter()
        .filter(|definition| in_goal || !definition.goal_only)
        .filter(|definition| mode != crate::settings::Mode::Plan || !definition.mutating)
        .filter(|definition| graph_enabled || !definition.graph_required)
        .filter(|definition| match definition.web {
            WebRole::None => true,
            WebRole::Lead => web_enabled,
            WebRole::Leaf => false,
        })
        .map(|definition| (definition.spec)())
        .collect()
}

/// The web agent's entire tool surface: search and fetch, nothing else. No file
/// or graph access, no orchestration — a page that tries prompt injection has
/// nothing to steer.
pub fn get_web_agent_tools() -> Vec<GaiseTool> {
    get_web_agent_tools_for(true)
}

/// As [`get_web_agent_tools`], with `web_search` withheld in fetch-only mode
/// (no search backend configured) so the researcher structurally cannot try to
/// search — it works from the URLs in its brief.
pub fn get_web_agent_tools_for(can_search: bool) -> Vec<GaiseTool> {
    TOOL_REGISTRY
        .iter()
        .filter(|definition| definition.web == WebRole::Leaf)
        .map(|definition| (definition.spec)())
        .filter(|tool| can_search || tool.name != "web_search")
        .collect()
}

/// Read-only leaf toolset for a spawned subagent. Orchestration controls are not
/// merely discouraged: they are absent, structurally preventing recursive fan-out.
/// Mutating tools are also absent because every worker runs against the shared
/// checkout without a worktree.
pub fn get_subagent_tools() -> Vec<GaiseTool> {
    get_subagent_tools_for(true)
}

pub fn get_subagent_tools_for(graph_enabled: bool) -> Vec<GaiseTool> {
    const ALLOWED: &[&str] = &[
        "read_file",
        "list_directory",
        "index_codebase",
        "search_code",
        "outline_file",
        "ask_codebase",
        "list_skills",
        "invoke_skill",
    ];
    TOOL_REGISTRY
        .iter()
        .filter(|definition| graph_enabled || !definition.graph_required)
        .map(|definition| (definition.spec)())
        .filter(|tool| ALLOWED.contains(&tool.name.as_str()))
        .collect()
}

/// Normalise `text`'s line endings to match the target file: collapse everything to
/// LF first, then expand to CRLF when the file uses it. A model-supplied
/// `old_text`/`new_text` is almost always LF-only, but Windows files are frequently
/// CRLF and `read_file` hides that (it shows LF-stripped lines). Without this, a
/// multi-line `old_text` never matches a CRLF file's raw bytes — the edit silently
/// fails and the model burns turns retrying. Matching the file's style fixes that and
/// keeps the rewritten file's endings consistent.
pub(crate) fn match_line_endings(text: &str, crlf: bool) -> String {
    let lf = text.replace("\r\n", "\n");
    if crlf {
        lf.replace('\n', "\r\n")
    } else {
        lf
    }
}

// --- Shared builders for parameter schemas, used by the per-tool modules. ---

/// An `object` schema from named properties and a required-key list.
pub(crate) fn obj(
    props: Vec<(&str, GaiseToolParameter)>,
    required: Vec<&str>,
) -> GaiseToolParameter {
    GaiseToolParameter {
        r#type: Some("object".to_string()),
        description: None,
        properties: Some(props.into_iter().map(|(k, v)| (k.to_string(), v)).collect()),
        items: None,
        required: if required.is_empty() {
            None
        } else {
            Some(required.into_iter().map(|s| s.to_string()).collect())
        },
    }
}

/// A scalar parameter (`string`, `integer`, ...) with a description.
pub(crate) fn p(ty: &str, desc: &str) -> GaiseToolParameter {
    GaiseToolParameter {
        r#type: Some(ty.to_string()),
        description: Some(desc.to_string()),
        ..Default::default()
    }
}

/// An `array` parameter of `item_ty` items, with a description.
pub(crate) fn arr(item_ty: &str, desc: &str) -> GaiseToolParameter {
    GaiseToolParameter {
        r#type: Some("array".to_string()),
        description: Some(desc.to_string()),
        properties: None,
        items: Some(Box::new(GaiseToolParameter {
            r#type: Some(item_ty.to_string()),
            ..Default::default()
        })),
        required: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{AskResult, Indexer};
    use anyhow::Result;
    use async_trait::async_trait;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    struct AgentToolHost {
        indexer: Indexer,
        todos: Vec<Todo>,
        root: PathBuf,
        calls: Mutex<Vec<String>>,
    }

    impl AgentToolHost {
        fn new(root: PathBuf) -> Self {
            Self {
                indexer: Indexer::new_ephemeral(root.clone()),
                todos: Vec::new(),
                root,
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn push(&self, value: String) {
            self.calls.lock().unwrap().push(value);
        }
    }

    #[async_trait]
    impl ToolHost for AgentToolHost {
        fn indexer(&mut self) -> &mut Indexer {
            &mut self.indexer
        }

        fn todos(&mut self) -> &mut Vec<Todo> {
            &mut self.todos
        }

        fn project_root(&self) -> &Path {
            &self.root
        }

        fn validate_path(&self, path: &str) -> Result<PathBuf> {
            Ok(self.root.join(path))
        }

        fn ask_permission(
            &mut self,
            _tool_key: &str,
            _detail: &str,
            _action: &str,
        ) -> Result<bool> {
            Ok(false)
        }

        fn reindex_path(&mut self, _path: &Path) {}

        fn mark_workspace_changed(&mut self) {}

        async fn ask_codebase(&self, _question: &str, _k: usize) -> AskResult {
            AskResult::default()
        }

        fn spawn_agent(
            &mut self,
            task: &str,
            name: Option<&str>,
            effort: Option<&str>,
        ) -> Result<String> {
            self.push(format!("spawn:{task}:{name:?}:{effort:?}"));
            Ok("spawned".to_string())
        }

        fn list_agents(&self) -> String {
            self.push("list".to_string());
            "listed".to_string()
        }

        fn send_agent(&self, id: &str, message: &str) -> Result<String> {
            self.push(format!("send:{id}:{message}"));
            Ok("sent".to_string())
        }

        async fn wait_agent(&mut self, id: Option<&str>) -> Result<String> {
            self.push(format!("wait:{id:?}"));
            Ok("waited".to_string())
        }

        fn stop_agent(&self, id: &str) -> Result<String> {
            self.push(format!("stop:{id}"));
            Ok("stopped".to_string())
        }

        fn close_agent(&self, id: &str) -> Result<String> {
            self.push(format!("close:{id}"));
            Ok("closed".to_string())
        }
    }

    #[test]
    fn lf_text_matches_a_crlf_file() {
        // A CRLF file's raw content, and a model-supplied LF-only old_text.
        let file = "line1\r\nline2\r\nline3\r\n";
        let old_lf = "line1\nline2";
        // Literal LF match fails against the CRLF file...
        assert_eq!(file.matches(old_lf).count(), 0);
        // ...but after normalising to the file's endings it matches uniquely.
        let normalised = match_line_endings(old_lf, file.contains("\r\n"));
        assert_eq!(normalised, "line1\r\nline2");
        assert_eq!(file.matches(&normalised).count(), 1);
    }

    #[test]
    fn crlf_text_collapses_for_an_lf_file() {
        // The reverse: CRLF input against an LF file collapses to LF.
        let lf_file = "a\nb\nc\n";
        let normalised = match_line_endings("a\r\nb", lf_file.contains("\r\n"));
        assert_eq!(normalised, "a\nb");
        assert_eq!(lf_file.matches(&normalised).count(), 1);
    }

    #[test]
    fn already_matching_endings_are_left_intact() {
        // No double-conversion: CRLF input to a CRLF file stays single CRLF.
        assert_eq!(match_line_endings("x\r\ny", true), "x\r\ny");
        assert_eq!(match_line_endings("x\ny", false), "x\ny");
    }

    #[test]
    fn web_tool_split_keeps_leaf_tools_out_of_every_lead_and_worker_list() {
        // The lead only ever sees web_research, and only when web is enabled.
        let lead_off = get_tools_with_web(crate::settings::Mode::Agentic, false, true, false);
        assert!(!lead_off.iter().any(|tool| tool.name.starts_with("web_")));
        let lead_on = get_tools_with_web(crate::settings::Mode::Agentic, false, true, true);
        assert!(lead_on.iter().any(|tool| tool.name == "web_research"));
        assert!(!lead_on
            .iter()
            .any(|tool| tool.name == "web_search" || tool.name == "web_fetch"));
        // Plan mode keeps web_research: read-only, gated by permission instead.
        let plan = get_tools_with_web(crate::settings::Mode::Plan, false, true, true);
        assert!(plan.iter().any(|tool| tool.name == "web_research"));
        // The web agent's world is exactly search + fetch — fetch alone when no
        // search backend is configured.
        let web_tools: Vec<String> = get_web_agent_tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert_eq!(web_tools, ["web_search", "web_fetch"]);
        let fetch_only: Vec<String> = get_web_agent_tools_for(false)
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert_eq!(fetch_only, ["web_fetch"]);
        // Code subagents gain nothing web-facing.
        assert!(!get_subagent_tools()
            .iter()
            .any(|tool| tool.name.starts_with("web_")));
        // The legacy builder stays web-free for existing callers/tests.
        assert!(!get_tools(crate::settings::Mode::Agentic, false)
            .iter()
            .any(|tool| tool.name.starts_with("web_")));
    }

    #[test]
    fn agent_tool_schemas_have_the_expected_contracts() {
        let specs = [
            (spawn_agent::spec(), vec!["task"]),
            (list_agents::spec(), vec![]),
            (send_agent::spec(), vec!["id", "message"]),
            (wait_agent::spec(), vec![]),
            (stop_agent::spec(), vec!["id"]),
            (close_agent::spec(), vec!["id"]),
        ];
        for (tool, required) in specs {
            assert!(tool
                .description
                .as_deref()
                .is_some_and(|text| !text.is_empty()));
            assert_eq!(
                tool.parameters.unwrap().required.unwrap_or_default(),
                required.into_iter().map(str::to_string).collect::<Vec<_>>(),
                "{}",
                tool.name
            );
        }

        let leaf_names = get_subagent_tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        for orchestration in [
            "spawn_agent",
            "list_agents",
            "send_agent",
            "wait_agent",
            "stop_agent",
            "close_agent",
        ] {
            assert!(!leaf_names.iter().any(|name| name == orchestration));
        }
    }

    #[tokio::test]
    async fn agent_tool_dispatch_routes_and_trims_every_call() {
        let root = tempfile::tempdir().unwrap();
        let mut host = AgentToolHost::new(root.path().to_path_buf());

        assert_eq!(
            dispatch(
                &mut host,
                "spawn_agent",
                Some(r#"{"task":"  inspect auth  ","name":"  review  ","effort":"high"}"#),
            )
            .await
            .unwrap(),
            "spawned"
        );
        assert_eq!(
            dispatch(&mut host, "list_agents", Some("{}"))
                .await
                .unwrap(),
            "listed"
        );
        assert_eq!(
            dispatch(
                &mut host,
                "send_agent",
                Some(r#"{"id":" agent-1 ","message":" check callers "}"#),
            )
            .await
            .unwrap(),
            "sent"
        );
        assert_eq!(
            dispatch(&mut host, "wait_agent", Some(r#"{"id":" all "}"#))
                .await
                .unwrap(),
            "waited"
        );
        assert_eq!(
            dispatch(&mut host, "stop_agent", Some(r#"{"id":" agent-1 "}"#))
                .await
                .unwrap(),
            "stopped"
        );
        assert_eq!(
            dispatch(&mut host, "close_agent", Some(r#"{"id":" all "}"#))
                .await
                .unwrap(),
            "closed"
        );

        assert_eq!(
            host.calls(),
            vec![
                "spawn:inspect auth:Some(\"review\"):Some(\"high\")",
                "list",
                "send:agent-1:check callers",
                "wait:Some(\"all\")",
                "stop:agent-1",
                "close:all",
            ]
        );
    }

    #[tokio::test]
    async fn agent_tool_validation_blocks_bad_calls_before_the_host() {
        let root = tempfile::tempdir().unwrap();
        let mut host = AgentToolHost::new(root.path().to_path_buf());

        let empty = dispatch(
            &mut host,
            "spawn_agent",
            Some(r#"{"task":"   ","effort":"low"}"#),
        )
        .await
        .unwrap();
        assert!(empty.contains("cannot be empty"));
        let effort = dispatch(
            &mut host,
            "spawn_agent",
            Some(r#"{"task":"inspect","effort":"impossible"}"#),
        )
        .await
        .unwrap();
        assert!(effort.contains("unknown effort"));
        assert!(dispatch(&mut host, "send_agent", Some("{}")).await.is_err());
        assert_eq!(
            dispatch(&mut host, "not_a_tool", None).await.unwrap(),
            "Unknown tool: not_a_tool"
        );
        assert!(host.calls().is_empty());
    }
}
