//! Goal-driven autonomous tasks: a persisted objective the harness pursues across
//! many model turns until its acceptance criteria are met, it blocks on input, or a
//! step budget is exhausted.
//!
//! The unit of work is a [`GoalTask`] carrying its **own** conversation history,
//! mode, and cache lane — never the shared `App::history` / `App::settings.mode`.
//! v1 drives one task at a time; the data model ([`GoalStore`] = `Vec` + an
//! `active_id`) and the per-task isolation are deliberately shaped so concurrency
//! later becomes "add a scheduler over `run_turn`", not a schema rewrite.
//!
//! Persistence mirrors [`crate::settings::LocalSettings`]: a missing `goals.json`
//! yields an empty store silently; a malformed one warns and defaults — never fail
//! startup. Only **metadata** is stored here; the per-task conversation lives in its
//! own `.ikode/sessions/<id>.jsonl` (so a task is "a session with an objective" and
//! reuses the existing transcript persistence), and is rehydrated from there.

use std::path::{Path, PathBuf};

use gaise_core::contracts::GaiseMessage;
use serde::{Deserialize, Serialize};

use ikode::settings::{Effort, Mode};

/// Where a task is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GoalStatus {
    /// Being worked (or paused mid-run, resumable).
    Active,
    /// Acceptance criteria met — the model called `task_complete`.
    Done,
    /// Closed by the user without completing.
    Abandoned,
    /// Stuck pending user input — the model called `task_block`.
    Blocked,
}

impl GoalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            GoalStatus::Active => "active",
            GoalStatus::Done => "done",
            GoalStatus::Abandoned => "abandoned",
            GoalStatus::Blocked => "blocked",
        }
    }

    /// A small glyph for list/status rendering.
    pub fn glyph(&self) -> &'static str {
        match self {
            GoalStatus::Active => "▶",
            GoalStatus::Done => "✅",
            GoalStatus::Abandoned => "✖",
            GoalStatus::Blocked => "⏸",
        }
    }
}

/// One goal task: an objective plus the isolated state needed to pursue it.
///
/// `history`, `mode`, and `cache_key` live here (not on `App`) so two tasks never
/// share a conversation or contend on a global mode — the single decision that
/// makes later concurrency cheap. `worktree` is reserved for the filesystem
/// isolation a concurrent task will need; it is always `None` in v1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalTask {
    /// Short id; also the stem of this task's `.ikode/sessions/<id>.jsonl`.
    pub id: String,
    /// What the user asked for.
    pub objective: String,
    /// Definition of done — model-authored via the `task_plan` tool.
    #[serde(default)]
    pub acceptance: Vec<String>,
    pub status: GoalStatus,
    /// Per-task operating mode (not the global `settings.mode`).
    pub mode: Mode,
    /// Per-task effort snapshot, so resuming a goal does not silently change its
    /// reasoning/delegation contract when the interactive setting changes.
    #[serde(default)]
    pub effort: Effort,
    /// This task's own prompt-cache lane (an opaque key; folded with mode and effort for
    /// the OpenAI `prompt_cache_key`). Keeping it per-task means a long task history
    /// stays warm across the task's many turns, and — under future concurrency —
    /// divergent tasks never evict each other's cached prefix.
    pub cache_key: String,
    /// The transcript file this task appends to (`<id>` of its session).
    pub session_id: String,
    /// Reserved for per-task filesystem isolation (a git worktree). `None` in v1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<PathBuf>,
    /// In-memory conversation for the active run. NOT persisted here — it is the
    /// session transcript on disk, rehydrated via [`crate::session::load_messages`].
    #[serde(skip)]
    pub history: Vec<GaiseMessage>,
}

impl GoalTask {
    /// Begin a fresh task for `objective` in `mode`. `id` doubles as the session id
    /// and `cache_key` is a fresh opaque lane.
    pub fn new(
        id: String,
        objective: String,
        mode: Mode,
        effort: Effort,
        cache_key: String,
    ) -> Self {
        let session_id = id.clone();
        GoalTask {
            id,
            objective,
            acceptance: Vec::new(),
            status: GoalStatus::Active,
            mode,
            effort,
            cache_key,
            session_id,
            worktree: None,
            history: Vec::new(),
        }
    }

    /// The serialisable metadata only (drops the in-memory `history`). Used when
    /// persisting so the store never duplicates the on-disk transcript.
    pub fn metadata(&self) -> GoalTask {
        GoalTask {
            history: Vec::new(),
            ..self.clone()
        }
    }
}

/// The task list. Stored as a `Vec` + `active_id` (rather than a single goal) from
/// v1 so going multi-task is a data change, not a migration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoalStore {
    #[serde(default)]
    pub tasks: Vec<GoalTask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_id: Option<String>,
}

pub enum GoalMatch<'a> {
    None,
    Unique(&'a GoalTask),
    Ambiguous(Vec<&'a GoalTask>),
}

/// Project-relative path to the goals file.
pub fn goals_path(root: &Path) -> PathBuf {
    root.join(".ikode").join("goals.json")
}

impl GoalStore {
    pub fn match_task(&self, query: &str) -> GoalMatch<'_> {
        if let Some(exact) = self.tasks.iter().find(|task| task.id == query) {
            return GoalMatch::Unique(exact);
        }
        let matches: Vec<&GoalTask> = self
            .tasks
            .iter()
            .filter(|task| task.id.starts_with(query))
            .collect();
        match matches.len() {
            0 => GoalMatch::None,
            1 => GoalMatch::Unique(matches[0]),
            _ => GoalMatch::Ambiguous(matches),
        }
    }

    /// Load from `<root>/.ikode/goals.json`. Missing → empty store silently;
    /// malformed → empty store with a warning (never fail startup).
    pub fn load(root: &Path) -> GoalStore {
        let path = goals_path(root);
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("ikode: ignoring malformed {}: {e}", path.display());
                    GoalStore::default()
                }
            },
            Err(_) => GoalStore::default(),
        }
    }

    /// Persist to `<root>/.ikode/goals.json` (pretty JSON). Stores metadata only —
    /// the conversation lives in each task's session transcript.
    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let path = goals_path(root);
        let stripped = GoalStore {
            tasks: self.tasks.iter().map(GoalTask::metadata).collect(),
            active_id: self.active_id.clone(),
        };
        let json = serde_json::to_vec_pretty(&stripped).map_err(std::io::Error::other)?;
        ikode::util::atomic_write(&path, &json)
    }

    /// Insert or replace a task (matched by id) and mark it active.
    pub fn upsert_active(&mut self, task: GoalTask) {
        self.active_id = Some(task.id.clone());
        match self.tasks.iter_mut().find(|t| t.id == task.id) {
            Some(existing) => *existing = task,
            None => self.tasks.push(task),
        }
    }

    /// The currently-active task, if any.
    pub fn active(&self) -> Option<&GoalTask> {
        let id = self.active_id.as_ref()?;
        self.tasks.iter().find(|t| &t.id == id)
    }

    /// Set the status of the task with `id` (no-op if unknown).
    pub fn set_status(&mut self, id: &str, status: GoalStatus) {
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.status = status;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str, objective: &str) -> GoalTask {
        GoalTask::new(
            id.to_string(),
            objective.to_string(),
            Mode::Agentic,
            Effort::Medium,
            format!("ck-{id}"),
        )
    }

    #[test]
    fn round_trips_through_disk_without_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = GoalStore::default();
        let mut task = sample("abc", "ship the feature");
        task.acceptance = vec!["tests pass".into(), "docs updated".into()];
        // History is runtime-only and must never be persisted.
        task.history.push(GaiseMessage {
            role: "user".to_string(),
            content: None,
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
        });
        store.upsert_active(task);
        store.save(dir.path()).unwrap();

        let loaded = GoalStore::load(dir.path());
        assert_eq!(loaded.active_id.as_deref(), Some("abc"));
        let t = loaded.active().unwrap();
        assert_eq!(t.objective, "ship the feature");
        assert_eq!(t.acceptance, vec!["tests pass", "docs updated"]);
        assert_eq!(t.session_id, "abc");
        assert!(
            t.history.is_empty(),
            "history must not be persisted in goals.json"
        );
    }

    #[test]
    fn missing_and_malformed_yield_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        // Missing file → empty, silent.
        assert!(GoalStore::load(dir.path()).tasks.is_empty());
        // Malformed → empty, warned (warning not asserted here).
        std::fs::create_dir_all(dir.path().join(".ikode")).unwrap();
        std::fs::write(goals_path(dir.path()), "{ not json").unwrap();
        assert!(GoalStore::load(dir.path()).tasks.is_empty());
    }

    #[test]
    fn upsert_replaces_by_id_and_tracks_active() {
        let mut store = GoalStore::default();
        store.upsert_active(sample("one", "first"));
        store.upsert_active(sample("two", "second"));
        assert_eq!(store.tasks.len(), 2);
        assert_eq!(store.active_id.as_deref(), Some("two"));
        // Replacing an existing id updates in place rather than appending.
        let mut updated = sample("one", "first revised");
        updated.status = GoalStatus::Done;
        store.upsert_active(updated);
        assert_eq!(store.tasks.len(), 2);
        assert_eq!(store.active_id.as_deref(), Some("one"));
        let t = store.tasks.iter().find(|t| t.id == "one").unwrap();
        assert_eq!(t.objective, "first revised");
        assert_eq!(t.status, GoalStatus::Done);
    }

    #[test]
    fn set_status_updates_named_task() {
        let mut store = GoalStore::default();
        store.upsert_active(sample("x", "do it"));
        store.set_status("x", GoalStatus::Blocked);
        assert_eq!(store.tasks[0].status, GoalStatus::Blocked);
        // Unknown id is a no-op, not a panic.
        store.set_status("nope", GoalStatus::Done);
    }

    #[test]
    fn goal_prefixes_must_be_unique() {
        let mut store = GoalStore::default();
        store.tasks.push(sample("abc-one", "first"));
        store.tasks.push(sample("abc-two", "second"));
        assert!(matches!(store.match_task("abc-one"), GoalMatch::Unique(_)));
        assert!(matches!(
            store.match_task("abc"),
            GoalMatch::Ambiguous(matches) if matches.len() == 2
        ));
        assert!(matches!(store.match_task("nope"), GoalMatch::None));
    }
}
