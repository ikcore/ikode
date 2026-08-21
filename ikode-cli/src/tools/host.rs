//! The seam between tools and the running session. Tools live in the library and
//! execute against the [`ToolHost`] trait; the binary's `App` implements it. This
//! keeps each tool's execution code beside its definition (one file per tool)
//! while session-only state (chat history, prompts, REPL, model wiring) stays in
//! the binary.

use std::path::{Path, PathBuf};

use anyhow::Result;
use async_trait::async_trait;

use crate::index::{AskResult, Indexer};

/// A planner todo item. Lives in the library so the todo tools can own their
/// logic; the session holds the actual list.
pub struct Todo {
    pub id: usize,
    pub task: String,
    pub completed: bool,
}

/// Render the todo list the way the planner tools report it.
pub fn render_todos(todos: &[Todo]) -> String {
    let mut list = String::new();
    for todo in todos {
        let status = if todo.completed {
            "✅  completed"
        } else {
            "⏳  pending"
        };
        list.push_str(&format!("{}) {} ({})\n", todo.id, todo.task, status));
    }
    if list.is_empty() {
        "No tasks.".to_string()
    } else {
        list
    }
}

/// The capabilities a tool needs from its host.
#[async_trait]
pub trait ToolHost: Send {
    /// Mutable access to the code index/graph.
    fn indexer(&mut self) -> &mut Indexer;
    /// The session's planner todo list.
    fn todos(&mut self) -> &mut Vec<Todo>;
    /// Suppress terminal UI emitted by tools. Background subagents buffer their
    /// own activity and must not corrupt the lead REPL's live prompt rendering.
    fn quiet(&self) -> bool {
        false
    }
    /// The project root (working directory) — where `.ikode/` lives. Used by tools
    /// that read or write project configuration.
    fn project_root(&self) -> &Path;
    /// Resolve a relative path against the working directory, rejecting traversal
    /// or absolute paths that escape it. `Err` carries a human-readable reason.
    fn validate_path(&self, path: &str) -> Result<PathBuf>;
    /// Stable, project-relative spelling used by allow/deny rules. File tools call
    /// this only after validation, so aliases such as `./src/x.rs` and
    /// `src/dir/../x.rs` cannot bypass a rule written for `src/x.rs`.
    fn permission_path(&self, path: &Path) -> String {
        let root = self
            .project_root()
            .canonicalize()
            .unwrap_or_else(|_| self.project_root().to_path_buf());
        let relative = path.strip_prefix(&root).unwrap_or(path);
        let detail = relative.to_string_lossy().replace('\\', "/");
        if detail.is_empty() {
            ".".to_string()
        } else {
            detail
        }
    }
    /// Permission gate for a mutating tool (see [`crate::settings`]). `detail` is
    /// the matchable action value (command string / file path) used by allow/deny
    /// rules; `action` is the human-readable prompt. Returns `true` to proceed.
    fn ask_permission(&mut self, tool_key: &str, detail: &str, action: &str) -> Result<bool>;
    /// Re-mirror a single path into the index/graph after a write.
    fn reindex_path(&mut self, path: &Path);
    /// Record that a tool may have changed workspace files in a way the targeted
    /// re-index cannot fully describe (notably an arbitrary shell command).
    fn mark_workspace_changed(&mut self);
    /// Retrieve relevant chunks + their connectivity for a question — semantic when
    /// an embedding index exists, keyword otherwise. Async because it may embed.
    async fn ask_codebase(&self, question: &str, k: usize) -> AskResult;

    /// Subagent orchestration hooks. The default implementations keep library
    /// tools usable in hosts that do not provide an agent runtime (including leaf
    /// subagents themselves, which are structurally unable to recurse).
    fn spawn_agent(
        &mut self,
        _task: &str,
        _name: Option<&str>,
        _effort: Option<&str>,
    ) -> Result<String> {
        Ok("Subagent orchestration is unavailable in this host.".to_string())
    }

    fn list_agents(&self) -> String {
        "Subagent orchestration is unavailable in this host.".to_string()
    }

    fn send_agent(&self, _id: &str, _message: &str) -> Result<String> {
        Ok("Subagent orchestration is unavailable in this host.".to_string())
    }

    async fn wait_agent(&mut self, _id: Option<&str>) -> Result<String> {
        Ok("Subagent orchestration is unavailable in this host.".to_string())
    }

    fn stop_agent(&self, _id: &str) -> Result<String> {
        Ok("Subagent orchestration is unavailable in this host.".to_string())
    }

    fn close_agent(&self, _id: &str) -> Result<String> {
        Ok("Subagent orchestration is unavailable in this host.".to_string())
    }

    /// The web-research seam. `web_session` is `Some` only inside a spawned web
    /// agent's host (where the `web_search`/`web_fetch` tools run against its
    /// budgeted session); `spawn_web_agent` is implemented only by the lead's
    /// `App`. Both default to unavailable, so a code subagent can neither reach
    /// the network nor spawn researchers — the same structural no-recursion
    /// boundary the other orchestration hooks use.
    fn web_session(&mut self) -> Option<&mut crate::web::WebSession> {
        None
    }

    async fn spawn_web_agent(&mut self, _question: &str, _depth: Option<usize>) -> Result<String> {
        Ok("Web research is unavailable in this host.".to_string())
    }
}
