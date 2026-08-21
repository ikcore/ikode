//! The `App` aggregate and its core plumbing: construction (provider config,
//! system prompt, Ctrl-C listener), history/session bookkeeping, the
//! working-directory path guard, the permission gate, and the [`ToolHost`]
//! implementation that lets library tools act against the running session.
//!
//! The larger behavioural groups live in sibling modules that each add an
//! `impl App` block: the REPL ([`crate::repl`]), the agent turn
//! ([`crate::turn`]), the index/enrich passes ([`crate::passes`]), and skill
//! management ([`crate::skills_cmd`]). `App`'s fields are `pub(crate)` so those
//! modules can reach them.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use colored::*;
use dialoguer::Select;
use gaise_client::ServiceAccount;
use gaise_client::{GaiseClientConfig, GaiseClientService};
use gaise_core::contracts::{GaiseContent, GaiseGenerationConfig, GaiseMessage, OneOrMany};
use gaise_core::GaiseClient;
use uuid::Uuid;

use crate::agent::{AgentLaunch, AgentManager};
use crate::goal::{GoalStore, GoalTask};
use crate::session;
use ikode::index::Indexer;
use ikode::mcp::McpManager;
use ikode::settings::{Decision, Effort, LocalSettings, Mode};
use ikode::tools::{Todo, ToolHost};
use ikode::util::{
    attach_content_to_last_user, effort_status_line, is_sendable_message, mode_status_line,
    resolve_within, session_start_date, short_id,
};

const MODEL_AGENT_WAIT_SLICE: Duration = Duration::from_secs(30);

pub(crate) struct App {
    /// Shared (`Arc`) so the `/visualize` server can hold its own handle and embed
    /// search queries while the REPL still owns the session.
    pub(crate) client: Arc<dyn GaiseClient>,
    pub(crate) history: Vec<GaiseMessage>,
    pub(crate) todos: Vec<Todo>,
    pub(crate) model: String,
    pub(crate) embedding_model: String,
    pub(crate) summary_model: String,
    pub(crate) brave: bool,
    pub(crate) system_prompt: String,
    /// The `YYYY-MM-DD` currently substituted into `system_prompt`. Tracked so a
    /// resumed session can re-anchor the prompt to its original date.
    pub(crate) system_prompt_date: String,
    pub(crate) session_cache_key: String,
    pub(crate) working_directory: PathBuf,
    pub(crate) max_history: usize,
    pub(crate) prefix_keep: usize,
    pub(crate) indexer: Indexer,
    /// Whether graph/index-backed retrieval is active for this session. When false,
    /// graph tools are structurally withheld and file changes do not touch the WAL.
    pub(crate) graph_enabled: bool,
    /// Session-scoped connections to explicitly registered third-party MCP servers.
    pub(crate) mcp: McpManager,
    /// Monotonic signal advanced only after a tool actually changes (or may have
    /// changed) workspace files. The turn loop compares it around dispatch so a
    /// denied/failed mutating tool does not trigger post-turn indexing.
    pub(crate) workspace_revision: u64,
    pub(crate) always_allow: HashSet<String>,
    /// File extensions (e.g. `rs`, `toml`) the user has chosen to auto-allow all
    /// further edits/creates/deletes for, for the remainder of this session. Set by
    /// the "Allow all further changes to this file type" prompt choice.
    pub(crate) always_allow_ext: HashSet<String>,
    pub(crate) settings: LocalSettings,
    pub(crate) interrupt: tokio::sync::watch::Receiver<u64>,
    /// The on-disk transcript this conversation is being appended to.
    pub(crate) session: session::Session,
    /// Persisted goal tasks (`.ikode/goals.json`). Each task carries its own
    /// isolated history, mode, and cache lane; v1 runs one at a time.
    pub(crate) goals: GoalStore,
    /// Session-scoped bounded read-only subagent threads.
    pub(crate) agents: AgentManager,
    /// Resolved web-research capability (search backend + budgets + optional
    /// headless browser), or `None` when no backend is configured — in which
    /// case `web_research` is structurally withheld from the tool list.
    pub(crate) web_config: Option<ikode::web::WebConfig>,
    /// Explicit web-agent model override (`/wmodel`); `None` falls back to the
    /// chat model at spawn time.
    pub(crate) web_model: Option<String>,
    /// Model-turn budget for one web agent.
    pub(crate) web_agent_turns: usize,
    /// Running token totals across every model turn in this session.
    pub(crate) session_input_tokens: usize,
    pub(crate) session_output_tokens: usize,
    pub(crate) session_cached_tokens: usize,
    /// Prompt (input) token count of the most recent model request — i.e. the
    /// current context size. Drives token-threshold auto-compaction.
    pub(crate) last_input_tokens: usize,
    /// Resolved context windows per `provider::model` (`None` = looked up, unknown).
    /// Filled lazily by [`Self::context_window`] so the registry/provider lookup
    /// happens once per model, not after every turn.
    pub(crate) context_windows: HashMap<String, Option<u64>>,
    /// Images attached via `/image` and queued for the *next* user message. Drained
    /// into [`Self::current_turn_images`] when that message is sent.
    pub(crate) pending_images: Vec<crate::image::LoadedImage>,
    /// Image content blocks for the turn currently in flight. Injected into the
    /// request (onto the latest user message) by [`Self::build_request_history`],
    /// but never recorded to `history` or the session file — image attachments are
    /// ephemeral, scoped to the single turn they're sent on.
    pub(crate) current_turn_images: Vec<GaiseContent>,
    /// Documents queued via `/document` or generic `/attach` for the next user
    /// message. Text documents become ordinary text blocks; supported binary
    /// documents become provider-native file blocks.
    pub(crate) pending_documents: Vec<crate::document::LoadedDocument>,
    /// Request-only document blocks for the turn currently in flight.
    pub(crate) current_turn_documents: Vec<GaiseContent>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        model: String,
        embedding_model: String,
        summary_model: String,
        brave: bool,
        guide_path: Option<String>,
        max_history: usize,
        prefix_keep: usize,
        project_root: PathBuf,
        graph_enabled: bool,
        settings: LocalSettings,
        web_config: Option<ikode::web::WebConfig>,
    ) -> Result<Self> {
        let mut config = GaiseClientConfig::default();

        if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
            config.openai_api_key = Some(api_key);
        }
        if let Ok(api_url) = std::env::var("OPENAI_API_URL") {
            config.openai_api_url = Some(api_url);
        }
        if let Ok(ollama_url) = std::env::var("OLLAMA_URL") {
            config.ollama_url = Some(ollama_url);
        }
        if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
            config.anthropic_api_key = Some(api_key);
        }
        if let Ok(api_url) = std::env::var("ANTHROPIC_API_URL") {
            config.anthropic_api_url = Some(api_url);
        }
        if let Ok(api_key) = std::env::var("GEMINI_API_KEY") {
            config.gemini_api_key = Some(api_key);
        }
        if let Ok(api_url) = std::env::var("GEMINI_API_URL") {
            config.gemini_api_url = Some(api_url);
        }
        if let Ok(region) = std::env::var("AWS_REGION") {
            config.bedrock_region = Some(region);
        }
        if let Ok(api_url) = std::env::var("VERTEXAI_API_URL") {
            config.vertexai_api_url = Some(api_url);
        }
        if let Ok(sa_path) = std::env::var("VERTEXAI_SA_PATH") {
            match load_vertex_service_account(&sa_path) {
                Ok(account) => config.vertexai_sa = Some(account),
                Err(error) => eprintln!(
                    "{} Could not load VERTEXAI_SA_PATH '{}': {:#}",
                    "⚠️ ".bright_yellow(),
                    sa_path,
                    error
                ),
            }
        }

        let client = GaiseClientService::new(config);

        let system_prompt_raw = include_str!("sys-prompt.md");
        let system_prompt_date = Self::local_date_string();
        let mut system_prompt =
            Self::format_system_prompt(system_prompt_raw, &project_root, &system_prompt_date);

        // Project instructions: prefer the docs-canonical .ikode/ikode.md, then a
        // root-level ikode.md (the CLAUDE.md equivalent).
        let instr_path = project_root.join(".ikode").join("ikode.md");
        let legacy_path = project_root.join("ikode.md");
        let chosen = if instr_path.exists() {
            Some(instr_path)
        } else if legacy_path.exists() {
            Some(legacy_path)
        } else {
            None
        };
        if let Some(p) = chosen {
            if let Ok(content) = fs::read_to_string(&p) {
                system_prompt.push_str(&format!(
                    "\n\nUser Project Guidelines (from {}):\n",
                    p.display()
                ));
                system_prompt.push_str(&content);
            }
        }

        if let Some(path) = guide_path {
            match fs::read_to_string(&path) {
                Ok(content) => {
                    system_prompt.push_str(&format!("\n\nUser Guidelines (from {}):\n", path));
                    system_prompt.push_str(&content);
                }
                Err(e) => eprintln!(
                    "{} Warning: Could not read guide file {}: {}",
                    "⚠️ ".yellow(),
                    path,
                    e
                ),
            }
        }

        let working_directory = project_root;
        let indexer = if graph_enabled {
            Indexer::new(working_directory.clone())
        } else {
            Indexer::new_ephemeral(working_directory.clone())
        };
        let mcp = McpManager::new(working_directory.clone());
        let session = session::Session::new(&working_directory, Uuid::new_v4().to_string());
        let goals = GoalStore::load(&working_directory);
        let agent_config = ikode::harness::ProjectConfig::resolve(&working_directory);
        let agents = AgentManager::new(
            agent_config.agent_max_threads.unwrap_or(4),
            agent_config.agent_max_turns.unwrap_or(12),
        )
        .with_web_limit(
            agent_config
                .web_max_agents
                .unwrap_or(ikode::web::DEFAULT_MAX_WEB_AGENTS),
        );
        // Local settings (session posture) beat committed project config for the
        // web model, mirroring the chat/embedding model layering in main.rs.
        let web_model = settings
            .models
            .as_ref()
            .and_then(|models| models.web.clone())
            .or(agent_config.web_model.clone());
        // The chat `model` argument arrives fully resolved by the caller
        // (CLI flag > local settings > project config > global > default); it
        // must not be re-derived here or the CLI flag and config.toml are lost.
        let web_agent_turns = agent_config
            .web_agent_max_turns
            .unwrap_or(ikode::web::DEFAULT_WEB_AGENT_TURNS);

        // Ctrl-C listener: each press bumps a counter on a watch channel; an
        // in-flight turn selects on this to cancel cleanly (without exiting).
        let (tx, interrupt) = tokio::sync::watch::channel(0u64);
        tokio::spawn(async move {
            let mut count = 0u64;
            loop {
                if tokio::signal::ctrl_c().await.is_err() {
                    break;
                }
                count += 1;
                if tx.send(count).is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            client: Arc::new(client),
            history: vec![GaiseMessage {
                role: "system".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text {
                    text: system_prompt.clone(),
                })),
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
            }],
            todos: Vec::new(),
            model,
            embedding_model,
            summary_model,
            brave,
            system_prompt,
            system_prompt_date,
            session_cache_key: Uuid::new_v4().to_string(),
            working_directory,
            max_history,
            prefix_keep,
            indexer,
            graph_enabled,
            mcp,
            workspace_revision: 0,
            always_allow: HashSet::new(),
            always_allow_ext: HashSet::new(),
            settings,
            interrupt,
            session,
            goals,
            agents,
            web_config,
            web_model,
            web_agent_turns,
            session_input_tokens: 0,
            session_output_tokens: 0,
            session_cached_tokens: 0,
            last_input_tokens: 0,
            context_windows: HashMap::new(),
            pending_images: Vec::new(),
            current_turn_images: Vec::new(),
            pending_documents: Vec::new(),
            current_turn_documents: Vec::new(),
        })
    }

    /// Push a message into history and append it to the on-disk session transcript.
    /// Persistence failures are non-fatal — they warn but never abort the turn.
    pub(crate) fn record(&mut self, msg: GaiseMessage) {
        if let Err(e) = self.session.append(&msg) {
            eprintln!("{} could not write session: {e}", "⚠️ ".yellow());
        }
        self.history.push(msg);
    }

    /// Record a message into a goal task's isolated history and append it to that
    /// task's own session transcript (`<id>.jsonl`). The task owns its history, so
    /// this never touches `self.history` — keeping goal conversations isolated from
    /// the interactive one (and from each other, under future concurrency).
    /// Persistence failures warn but never abort the turn.
    pub(crate) fn record_task(&self, task: &mut GoalTask, msg: GaiseMessage) {
        let session = session::Session::new(&self.working_directory, task.session_id.clone());
        if let Err(e) = session.append(&msg) {
            eprintln!("{} could not write goal session: {e}", "⚠️ ".yellow());
        }
        task.history.push(msg);
    }

    /// Replace the current conversation with one loaded from disk (grafted onto the
    /// freshly-built system prompt) and continue appending to that session's file.
    pub(crate) fn load_session(&mut self, info: &session::SessionInfo) -> Result<()> {
        let msgs = session::load_messages(&info.path)?;
        // Re-anchor the prompt's "today's date" to when this conversation started, so a
        // session resumed on a later day keeps a consistent prefix (and, within cache
        // TTL, a warm one) instead of silently switching to the launch day's date.
        if let Some(started) = session_start_date(&info.path) {
            if started != self.system_prompt_date {
                self.system_prompt = self
                    .system_prompt
                    .replace(&self.system_prompt_date, &started);
                self.system_prompt_date = started;
            }
        }
        let mut history = vec![self.system_message()];
        history.extend(msgs);
        self.history = history;
        self.session = session::Session {
            id: info.id.clone(),
            path: info.path.clone(),
        };
        self.session_cache_key = Uuid::new_v4().to_string();
        println!(
            "{} Resumed session {} ({} messages).",
            "📂 ".bright_green(),
            short_id(&info.id).cyan(),
            (self.history.len() - 1).to_string().bright_magenta().bold()
        );
        Ok(())
    }

    /// The system-prompt message that always heads the history.
    pub(crate) fn system_message(&self) -> GaiseMessage {
        GaiseMessage {
            role: "system".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: self.system_prompt.clone(),
            })),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
        }
    }

    /// Substitute the environment placeholders in the raw prompt. `today` is passed
    /// in (not read from the clock) so a resumed session can re-anchor the prompt to
    /// the date the conversation actually started — keeping the cached prefix and the
    /// narrative consistent rather than silently jumping to the launch day.
    pub(crate) fn format_system_prompt(raw: &str, root: &Path, today: &str) -> String {
        let wd = root.to_string_lossy().to_string();
        let platform = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        let os_version = "Unknown";
        // The shell `execute_command` actually spawns commands through — the model must
        // write commands for THIS shell (it is cmd.exe on Windows, not PowerShell).
        let shell = if cfg!(target_os = "windows") {
            "cmd.exe (Windows command prompt)"
        } else {
            "sh (POSIX shell)"
        };
        let is_git = root.join(".git").exists();

        raw.replace("__WORKING_DIRECTORY__", &wd)
            .replace("__PLATFORM__", platform)
            .replace("__ARCH__", arch)
            .replace("__OS_VERSION__", os_version)
            .replace("__SHELL__", shell)
            .replace("__TODAY_DATE__", today)
            .replace("__IS_GIT_REPO__", if is_git { "Yes" } else { "No" })
    }

    /// Today's date as `YYYY-MM-DD` in local time — the format embedded in the prompt.
    pub(crate) fn local_date_string() -> String {
        chrono::Local::now().format("%Y-%m-%d").to_string()
    }

    /// Build the request history for the interactive prompt path (the shared
    /// `self.history`, current global mode, no goal block).
    pub(crate) fn build_request_history(&self) -> Vec<GaiseMessage> {
        let mut msgs = self.assemble_history(
            &self.history,
            self.settings.mode,
            self.settings.effort,
            None,
        );
        // Splice this turn's attachments onto the latest user message. They
        // live only in the request — never in `history` or the transcript — so they
        // inform the current turn (and every tool-loop iteration within it) without
        // being re-sent on later turns or persisted across a resume.
        if !self.current_turn_images.is_empty() {
            attach_content_to_last_user(&mut msgs, &self.current_turn_images);
        }
        if !self.current_turn_documents.is_empty() {
            attach_content_to_last_user(&mut msgs, &self.current_turn_documents);
        }
        msgs
    }

    /// Assemble the messages sent to the model for one turn from `history`: apply the
    /// context window (keep the cacheable head + a recent tail), drop empty turns,
    /// and append two high-authority blocks to the system message — the optional
    /// **goal** block (objective + acceptance), then the live **mode** line.
    ///
    /// `goal` is `Some((objective, acceptance))` on the autonomous goal path and
    /// `None` interactively. The goal block is stable across a task's life, so it
    /// sits in the cached prefix; the mode line changes only on a mode switch (which
    /// is also folded into the cache key — see [`generation_config`]). Anything that
    /// varies per turn (e.g. the continuation nudge) is appended by the caller at the
    /// *tail*, never here at the head, so the warm prefix is never disturbed.
    pub(crate) fn assemble_history(
        &self,
        history: &[GaiseMessage],
        mode: Mode,
        effort: Effort,
        goal: Option<(&str, &[String])>,
    ) -> Vec<GaiseMessage> {
        let mut result = if self.max_history == 0 || history.len() <= self.max_history {
            history.to_vec()
        } else {
            let total = history.len();
            let mut result = Vec::with_capacity(self.max_history);

            let prefix_end = (1 + self.prefix_keep).min(total);
            result.extend_from_slice(&history[..prefix_end]);

            let tail_count = self.max_history.saturating_sub(prefix_end);
            let tail_start = total.saturating_sub(tail_count);

            if tail_start > prefix_end {
                // Deliberately omit the dropped-message count: a changing number here
                // would alter this early, otherwise-stable note every turn and break the
                // cached prefix for everything after it. A static phrasing keeps the
                // head (system + kept prefix + this note) cacheable.
                result.push(GaiseMessage {
                    role: "system".to_string(),
                    content: Some(OneOrMany::One(GaiseContent::Text {
                        text: "[Note: some earlier messages were truncated to save context. \
                               The conversation continues below.]"
                            .to_string(),
                    })),
                    tool_calls: None,
                    tool_call_id: None,
                    tool_name: None,
                });
            }

            let actual_tail_start = tail_start.max(prefix_end);
            result.extend_from_slice(&history[actual_tail_start..]);

            result
        };

        // Drop empty turns: a model turn that streamed neither text nor a tool call ends
        // up as content=None + no tool_calls. Providers (e.g. OpenAI) reject `content:
        // null` on a message without tool_calls, and the turn carries no information, so
        // it must not be sent back.
        result.retain(is_sendable_message);

        // Append the goal block then the live mode line to the system message. The
        // goal block (when present) is stable for the task's life so it stays in the
        // cached prefix; the mode line gives the model a fresh, high-authority signal
        // of the active mode each turn (without it the model infers mode from tool
        // availability and anchors on its own earlier refusals).
        if let Some(first) = result.first_mut() {
            if first.role == "system" {
                if let Some(OneOrMany::One(GaiseContent::Text { text })) = first.content.as_mut() {
                    if let Some((objective, acceptance)) = goal {
                        text.push_str(&goal_block(objective, acceptance));
                    }
                    text.push_str("\n\n");
                    text.push_str(mode_status_line(mode));
                    text.push_str("\n\n");
                    text.push_str(effort_status_line(effort));
                    text.push_str("\n\n");
                    text.push_str(if self.graph_enabled {
                        "ACTIVE GRAPH MODE: enabled — graph/index retrieval tools may be used."
                    } else {
                        "ACTIVE GRAPH MODE: disabled — work as a traditional file/shell harness; graph/index tools are unavailable."
                    });
                    text.push_str("\n\n");
                    text.push_str(&self.web_status_line());
                }
            }
        }

        result
    }

    /// The prompt-cache lane for this turn, or `None` for providers without an
    /// explicit cache key. `base` is the per-conversation lane (the session id for
    /// interactive turns, or the task's `cache_key` for a goal), folded with mode and
    /// effort so each runtime contract has its own lane: switching either costs one
    /// fresh prompt, while switching back can re-hit the still-warm prior lane.
    ///
    /// Only OpenAI exposes an explicit `prompt_cache_key` (a routing hint over its
    /// automatic prefix caching). Vertex/Gemini and others cache **implicitly** on a
    /// stable prefix, so they need no key here — the prefix discipline in
    /// [`assemble_history`] (stable system + goal block at the head, per-turn nudges
    /// at the tail) is what earns their cache hits. Setting the key is harmless for
    /// them (it is simply ignored), but we only bother for OpenAI.
    pub(crate) fn generation_config(
        &self,
        base: &str,
        mode: Mode,
        effort: Effort,
    ) -> Option<GaiseGenerationConfig> {
        generation_config_for(&self.model, base, mode, effort)
    }

    pub(crate) fn validate_path(&self, path: &str) -> Result<PathBuf> {
        resolve_within(&self.working_directory, path)
    }

    pub(crate) fn reindex_path(&mut self, path: &Path) {
        if !self.graph_enabled {
            return;
        }
        if let Some(spec) = ikode::lang::detect_language(path) {
            let _ = self.indexer.index_file(path, &spec);
        }
    }

    /// Model tools for this request: built-ins filtered by mode/graph state, then
    /// namespaced MCP tools when external side effects are permitted.
    pub(crate) fn request_tools(
        &self,
        mode: Mode,
        in_goal: bool,
    ) -> Vec<gaise_core::contracts::GaiseTool> {
        let mut tools = ikode::tools::get_tools_with_web(
            mode,
            in_goal,
            self.graph_enabled,
            self.web_config.is_some(),
        );
        if mode != Mode::Plan {
            tools.extend(self.mcp.tool_specs());
        }
        tools
    }

    /// The model web agents run on: the `/wmodel` override, else the chat model.
    pub(crate) fn effective_web_model(&self) -> String {
        self.web_model.clone().unwrap_or_else(|| self.model.clone())
    }

    /// Live, authoritative web-research status appended to the system message —
    /// mirrors the ACTIVE GRAPH MODE line, so the model knows whether
    /// `web_research` can search, only fetch, or is absent (and can tell the
    /// user exactly how to enable it rather than improvising).
    fn web_status_line(&self) -> String {
        match &self.web_config {
            Some(web) if web.backend.is_some() => format!(
                "WEB RESEARCH: enabled — web_research delegates to a sandboxed web agent ({}).",
                web.describe()
            ),
            Some(web) => format!(
                "WEB RESEARCH: fetch-only — web_research can read pages at explicit URLs (JS rendering: {}), but no search backend is configured, so it cannot discover pages. If the user needs searching, tell them to set BRAVE_API_KEY or TAVILY_API_KEY in .ikode/.ikenv (or web_search_endpoint in config) and restart.",
                if web.browser.is_some() { "available" } else { "unavailable" }
            ),
            None => "WEB RESEARCH: unavailable in this session — do not attempt web access; tell the user web research is not configured.".to_string(),
        }
    }

    pub(crate) async fn reload_mcp(&mut self) {
        self.mcp.reload(&self.settings.mcp_servers).await;
    }

    /// Permission gate for mutating tools. Returns true if the action may proceed.
    /// Decide whether a mutating tool may run. Explicit denies and plan mode are
    /// evaluated before session grants, so an earlier "always allow" cannot override
    /// the current safety policy. Only `Decision::Ask` reaches the interactive
    /// prompt. `detail` is the matchable action value (command string / file path)
    /// used by allow/deny glob rules; `action` is the human-readable prompt.
    pub(crate) fn ask_permission(
        &mut self,
        tool_key: &str,
        detail: &str,
        action: &str,
    ) -> Result<bool> {
        let decision = self.settings.decide(tool_key, detail);
        if decision == Decision::Deny {
            let reason =
                if self.settings.mode == Mode::Plan && ikode::settings::is_mutating(tool_key) {
                    format!(
                        "blocked: planning mode is read-only. Switch with {} to make changes.",
                        "/mode agentic".cyan()
                    )
                } else {
                    "blocked by a deny rule in .ikode/settings.local.json".to_string()
                };
            println!("{} {} {}", "🚫".red(), tool_key.bold(), reason);
            return Ok(false);
        }
        if decision == Decision::Allow {
            return Ok(true);
        }

        let compound_command =
            tool_key == "execute_command" && ikode::settings::command_has_shell_control(detail);
        if !compound_command && self.always_allow.contains(tool_key) {
            return Ok(true);
        }
        // For file-targeting tools, `detail` is the path — derive its extension so we can
        // honour (and offer) a per-file-type session grant.
        let ext = file_type_ext(tool_key, detail);
        if let Some(e) = &ext {
            if self.always_allow_ext.contains(e) {
                return Ok(true);
            }
        }
        // Build the choice list. The file-type option only appears when we have an
        // extension to scope it to; each label maps to a `Choice` so the indices stay
        // correct whether or not that option is present.
        let mut labels: Vec<String> = vec!["Allow".to_string()];
        let mut choices: Vec<Choice> = vec![Choice::AllowOnce];
        if let Some(e) = &ext {
            labels.push(format!(
                "Allow all further changes to .{} files (this session)",
                e
            ));
            choices.push(Choice::AllowExt(e.clone()));
        }
        labels.push("Always allow this tool (this session)".to_string());
        choices.push(Choice::AllowToolSession);
        labels.push("Always allow this tool (save to settings.local.json)".to_string());
        choices.push(Choice::AllowToolSaved);
        labels.push("Reject".to_string());
        choices.push(Choice::Reject);

        let prompt = format!("{} {}?", "❓ ".bright_yellow(), action.cyan());
        let selection = Select::new()
            .with_prompt(prompt)
            .items(&labels)
            .default(0)
            .interact()?;
        match &choices[selection] {
            Choice::AllowOnce => Ok(true),
            Choice::AllowExt(e) => {
                self.always_allow_ext.insert(e.clone());
                Ok(true)
            }
            Choice::AllowToolSession => {
                self.always_allow.insert(tool_key.to_string());
                Ok(true)
            }
            Choice::AllowToolSaved => {
                if !self
                    .settings
                    .permissions
                    .allow
                    .iter()
                    .any(|r| r == tool_key)
                {
                    self.settings.permissions.allow.push(tool_key.to_string());
                }
                if let Err(e) = self.settings.save(&self.working_directory) {
                    eprintln!("{} could not save settings: {e}", "⚠️ ".yellow());
                }
                Ok(true)
            }
            Choice::Reject => Ok(false),
        }
    }

    /// A yes/no confirmation that doesn't record an "always allow" key (used by
    /// destructive one-off actions like /rebuild). Auto-confirms in yolo mode.
    pub(crate) fn ask_permission_sync(&self, action: &str, default_yes: bool) -> bool {
        if self.settings.mode == Mode::Yolo {
            return true;
        }
        let items = ["Yes", "No (cancel)"];
        let prompt = format!("{} {}?", "❓ ".bright_yellow(), action.cyan());
        Select::new()
            .with_prompt(prompt)
            .items(&items)
            .default(if default_yes { 0 } else { 1 })
            .interact()
            .map(|s| s == 0)
            .unwrap_or(false)
    }

    /// `!<cmd>` shell escape: run a command in-session, print it, and feed the
    /// output back into history so the model has it as context next turn.
    pub(crate) fn run_shell_escape(&mut self, cmd: &str) {
        println!("{} {}", "🐚".bright_magenta(), cmd.bright_magenta());
        let output = if cfg!(target_os = "windows") {
            Command::new("cmd").args(["/C", cmd]).output()
        } else {
            Command::new("sh").args(["-c", cmd]).output()
        };
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                if !stdout.is_empty() {
                    print!("{}", stdout);
                }
                if !stderr.is_empty() {
                    eprint!("{}", stderr);
                }
                let _ = io::stdout().flush();
                self.history.push(GaiseMessage {
                    role: "system".to_string(),
                    content: Some(OneOrMany::One(GaiseContent::Text {
                        text: format!(
                            "[shell] $ {}\nSTDOUT:\n{}\nSTDERR:\n{}",
                            cmd, stdout, stderr
                        ),
                    })),
                    tool_calls: None,
                    tool_call_id: None,
                    tool_name: None,
                });
            }
            Err(e) => println!("{} shell error: {}", "⚠️ ".bright_yellow(), e),
        }
    }
}

fn load_vertex_service_account(path: &str) -> Result<ServiceAccount> {
    let contents = std::fs::read_to_string(path).with_context(|| "read service-account file")?;
    let account: serde_json::Value =
        serde_json::from_str(&contents).with_context(|| "parse service-account JSON")?;
    let private_key = account["private_key"]
        .as_str()
        .filter(|value| !value.is_empty())
        .context("missing private_key")?;
    let client_email = account["client_email"]
        .as_str()
        .filter(|value| !value.is_empty())
        .context("missing client_email")?;
    Ok(ServiceAccount {
        private_key: private_key.to_string(),
        client_email: client_email.to_string(),
    })
}

/// The tool/session seam: tools (in the library) execute against this; here the
/// binary's `App` supplies the concrete capabilities. The delegations call the
/// inherent `App` methods of the same name (inherent methods take precedence in
/// method resolution, so there is no recursion).
#[async_trait::async_trait]
impl ToolHost for App {
    fn indexer(&mut self) -> &mut Indexer {
        &mut self.indexer
    }

    fn todos(&mut self) -> &mut Vec<Todo> {
        &mut self.todos
    }

    fn project_root(&self) -> &Path {
        &self.working_directory
    }

    fn validate_path(&self, path: &str) -> Result<PathBuf> {
        self.validate_path(path)
    }

    fn ask_permission(&mut self, tool_key: &str, detail: &str, action: &str) -> Result<bool> {
        self.ask_permission(tool_key, detail, action)
    }

    fn reindex_path(&mut self, path: &Path) {
        self.reindex_path(path);
        self.workspace_revision = self.workspace_revision.wrapping_add(1);
    }

    fn mark_workspace_changed(&mut self) {
        self.workspace_revision = self.workspace_revision.wrapping_add(1);
    }

    async fn ask_codebase(&self, question: &str, k: usize) -> ikode::index::AskResult {
        self.indexer
            .ask_auto(self.client.as_ref(), &self.embedding_model, question, k)
            .await
    }

    fn spawn_agent(
        &mut self,
        task: &str,
        name: Option<&str>,
        effort: Option<&str>,
    ) -> Result<String> {
        // Ultra makes the lead proactive. Workers default to high so parallel
        // fan-out does not multiply maximum-effort cost; callers can explicitly
        // request a different leaf effort when the task warrants it.
        let inherited = match self.settings.effort {
            Effort::Ultra => Effort::High,
            other => other,
        };
        let worker_effort = effort.and_then(Effort::parse).unwrap_or(inherited);
        let snapshot = self.agents.spawn(AgentLaunch {
            task: task.to_string(),
            name: name.map(str::to_string),
            model: self.model.clone(),
            effort: worker_effort,
            system_prompt: self.system_prompt.clone(),
            project_root: self.working_directory.clone(),
            embedding_model: self.embedding_model.clone(),
            client: self.client.clone(),
            graph_enabled: self.graph_enabled,
            flavor: crate::agent::AgentFlavor::Code,
        })?;
        Ok(format!(
            "Spawned subagent {} ({}) at {} effort for: {}. Use wait_agent to collect its result.",
            snapshot.name,
            snapshot.id,
            snapshot.effort.as_str(),
            snapshot.task
        ))
    }

    async fn spawn_web_agent(&mut self, question: &str, depth: Option<usize>) -> Result<String> {
        let Some(web) = self.web_config.clone() else {
            return Ok(
                "Web research is not configured: set BRAVE_API_KEY or TAVILY_API_KEY in .ikode/.ikenv (or a SearXNG web_search_endpoint) and restart iKode."
                    .to_string(),
            );
        };
        let depth = depth.unwrap_or(1).min(2);
        let description = web.describe();
        // Research is bounded summarisation work — never inherit ultra.
        let effort = match self.settings.effort {
            Effort::Ultra => Effort::High,
            other => other,
        };
        let snapshot = self.agents.spawn(AgentLaunch {
            task: question.to_string(),
            name: Some("web".to_string()),
            model: self.effective_web_model(),
            effort,
            // Web workers get a dedicated research prompt in the worker loop;
            // the lead's coding system prompt is deliberately not inherited.
            system_prompt: String::new(),
            project_root: self.working_directory.clone(),
            embedding_model: self.embedding_model.clone(),
            client: self.client.clone(),
            graph_enabled: false,
            flavor: crate::agent::AgentFlavor::Web {
                config: web,
                depth,
                max_turns: self.web_agent_turns,
            },
        })?;
        println!(
            "{} Web research {} ({}) started on {}: {}",
            "🌐".bright_cyan(),
            snapshot.name.bright_cyan(),
            snapshot.id,
            snapshot.model.bright_magenta(),
            ikode::util::truncate_preview(question, 100)
        );
        Ok(format!(
            "Spawned web agent {} ({}) on {} (depth {depth}; {description}) researching: {}. Continue other work; collect with wait_agent, or the result will be auto-surfaced into the conversation when it finishes.",
            snapshot.name, snapshot.id, snapshot.model, snapshot.task
        ))
    }

    fn list_agents(&self) -> String {
        self.agents.list()
    }

    fn send_agent(&self, id: &str, message: &str) -> Result<String> {
        self.agents.send(id, message)
    }

    async fn wait_agent(&mut self, id: Option<&str>) -> Result<String> {
        match tokio::time::timeout(MODEL_AGENT_WAIT_SLICE, self.agents.wait(id)).await {
            Ok(result) => result,
            Err(_) => Ok(format!(
                "Subagent wait reached its 30-second polling boundary; workers are still running. Call wait_agent again when their results are needed.\n\n{}",
                self.agents.list()
            )),
        }
    }

    fn stop_agent(&self, id: &str) -> Result<String> {
        self.agents.stop(id)
    }

    fn close_agent(&self, id: &str) -> Result<String> {
        self.agents.close(id)
    }
}

/// The outcome of the interactive permission prompt, decoupled from the menu index
/// (which shifts depending on whether the file-type option is offered).
enum Choice {
    /// Allow this single invocation.
    AllowOnce,
    /// Auto-allow every further change to files with this extension, this session.
    AllowExt(String),
    /// Auto-allow this tool for the rest of the session.
    AllowToolSession,
    /// Auto-allow this tool and persist an allow rule to `settings.local.json`.
    AllowToolSaved,
    /// Decline this invocation.
    Reject,
}

/// The high-authority goal block appended to the system message during an
/// autonomous goal. Stable across the task's life (so it stays in the cached
/// prefix): the objective never changes and acceptance only changes on an explicit
/// `task_plan` re-plan.
fn goal_block(objective: &str, acceptance: &[String]) -> String {
    let mut block = String::from("\n\n## Active goal\n");
    block.push_str("You are working autonomously toward this objective:\n");
    block.push_str(objective);
    block.push('\n');
    if acceptance.is_empty() {
        block.push_str(
            "\nAcceptance criteria: not yet defined — call task_plan to set them before working.",
        );
    } else {
        block.push_str("\nAcceptance criteria (all must hold to call task_complete):\n");
        for c in acceptance {
            block.push_str("- ");
            block.push_str(c);
            block.push('\n');
        }
    }
    block.push_str(
        "\nUse task_complete when every criterion is met, or task_block if you need user input.",
    );
    block
}

/// The file extension that a "this file type" grant would key on, or `None` when the
/// tool doesn't target a file path (so the option isn't offered). Only the file
/// mutating tools carry a path in `detail`; `execute_command`/`update_settings` don't.
fn file_type_ext(tool_key: &str, detail: &str) -> Option<String> {
    if !matches!(
        tool_key,
        "edit_file" | "edit_chunk" | "create_file" | "delete_file"
    ) {
        return None;
    }
    Path::new(detail)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
}

fn generation_config_for(
    model: &str,
    base: &str,
    mode: Mode,
    effort: Effort,
) -> Option<GaiseGenerationConfig> {
    let cache_key = model
        .starts_with("openai::")
        .then(|| format!("{}-{}-{}", base, mode.as_str(), effort.as_str()));
    let thinking_effort = effort.api_value(model).map(str::to_string);
    if cache_key.is_none() && thinking_effort.is_none() {
        None
    } else {
        Some(GaiseGenerationConfig {
            thinking_effort,
            cache_key,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_config_maps_effort_and_separates_cache_lanes() {
        let openai =
            generation_config_for("openai::gpt-5.4-mini", "chat-1", Mode::Agentic, Effort::Max)
                .unwrap();
        assert_eq!(openai.thinking_effort.as_deref(), Some("xhigh"));
        assert_eq!(openai.cache_key.as_deref(), Some("chat-1-agentic-max"));

        let auto =
            generation_config_for("openai::gpt-5.4-mini", "chat-1", Mode::Plan, Effort::Auto)
                .unwrap();
        assert_eq!(auto.thinking_effort, None);
        assert_eq!(auto.cache_key.as_deref(), Some("chat-1-plan-auto"));

        let claude = generation_config_for(
            "anthropic::claude-sonnet-4-6",
            "ignored",
            Mode::Agentic,
            Effort::Ultra,
        )
        .unwrap();
        assert_eq!(claude.thinking_effort.as_deref(), Some("max"));
        assert_eq!(claude.cache_key, None);

        let gemini = generation_config_for(
            "gemini::gemini-3.1-pro-preview",
            "ignored",
            Mode::Yolo,
            Effort::Max,
        )
        .unwrap();
        assert_eq!(gemini.thinking_effort.as_deref(), Some("high"));
        assert_eq!(gemini.cache_key, None);
    }

    #[test]
    fn generation_config_omits_unsupported_non_openai_effort() {
        assert!(generation_config_for(
            "anthropic::claude-sonnet-4-5-20250929",
            "chat",
            Mode::Plan,
            Effort::High,
        )
        .is_none());
        assert!(generation_config_for(
            "bedrock::amazon.titan-text-express-v1",
            "chat",
            Mode::Plan,
            Effort::High,
        )
        .is_none());
        assert!(
            generation_config_for("ollama::qwen3", "chat", Mode::Plan, Effort::High,).is_none()
        );

        let bedrock = generation_config_for(
            "bedrock::us.amazon.nova-2-lite-v1:0",
            "chat",
            Mode::Agentic,
            Effort::Max,
        )
        .unwrap();
        assert_eq!(bedrock.thinking_effort.as_deref(), Some("high"));
        assert_eq!(bedrock.cache_key, None);

        // OpenAI still receives its prompt-cache lane even when this model family
        // must not receive a reasoning_effort field.
        let non_reasoning =
            generation_config_for("openai::gpt-4.1", "chat", Mode::Plan, Effort::High).unwrap();
        assert_eq!(non_reasoning.thinking_effort, None);
        assert_eq!(non_reasoning.cache_key.as_deref(), Some("chat-plan-high"));
    }
}
