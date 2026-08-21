//! The interactive REPL: the welcome banner, the slash-command dispatch loop, and
//! the small command helpers it owns (`/mode` display, `/cls`, `/resume`, and the
//! `… save` config writer). Each branch delegates to the relevant `App` method on
//! a sibling module ([`crate::passes`], [`crate::turn`], [`crate::skills_cmd`]).

use std::collections::HashSet;
use std::process::Command;

use anyhow::Result;
use colored::*;
use dialoguer::Select;
use uuid::Uuid;

use crate::app::App;
use crate::goal::GoalStatus;
use crate::palette;
use crate::session;
use ikode::harness::{self, ConfigScope, ProjectConfig};
use ikode::mcp::parse_add_server;
use ikode::settings::{validate_rule, Effort, Mode};
use ikode::util::{
    human_bytes, normalize_command, parse_byte_size, rel_time, short_id, truncate_preview,
};

mod command_loop;

impl App {
    pub(crate) fn print_mcp_startup_status(&self) {
        if self.settings.mcp_servers.is_empty() {
            return;
        }
        let statuses = self.mcp.statuses(&self.settings.mcp_servers);
        let enabled = statuses.iter().filter(|status| status.enabled).count();
        let connected = statuses.iter().filter(|status| status.connected).count();
        let tools = statuses
            .iter()
            .map(|status| status.tool_count)
            .sum::<usize>();
        println!(
            "{} MCP: {}/{} enabled server(s) connected, {} tool(s) discovered.",
            if connected == enabled {
                "✅"
            } else {
                "⚠️"
            }
            .bright_cyan(),
            connected,
            enabled,
            tools
        );
        for status in statuses.iter().filter(|status| status.error.is_some()) {
            println!(
                "  {} {}: {}",
                "!".bright_yellow(),
                status.name.cyan(),
                status.error.as_deref().unwrap_or("connection failed")
            );
        }
    }

    fn print_mcp_status(&self) {
        let statuses = self.mcp.statuses(&self.settings.mcp_servers);
        if statuses.is_empty() {
            println!(
                "{} No MCP servers registered. Add one with {}.",
                "•".dimmed(),
                "/mcp add <name> -- <command> [args...]".cyan()
            );
            return;
        }
        println!("{}", "MCP servers:".bright_green().bold());
        for status in statuses {
            let (glyph, state) = if !status.enabled {
                ("○".dimmed().to_string(), "disabled".dimmed().to_string())
            } else if status.connected {
                (
                    "✓".bright_green().to_string(),
                    format!("connected · {} tools", status.tool_count)
                        .dimmed()
                        .to_string(),
                )
            } else {
                (
                    "!".bright_yellow().to_string(),
                    status
                        .error
                        .as_deref()
                        .unwrap_or("not connected")
                        .bright_yellow()
                        .to_string(),
                )
            };
            println!(
                "  {} {} [{}] — {}",
                glyph,
                status.name.bright_cyan(),
                status.transport,
                state
            );
        }
        println!(
            "  {}",
            "MCP tools are namespaced as mcp__server__tool and permission-gated by default."
                .dimmed()
        );
    }

    pub(crate) async fn handle_mcp_command(&mut self, argument: &str) {
        let argument = argument.trim();
        if argument.is_empty() || argument == "list" {
            self.print_mcp_status();
            return;
        }
        if argument == "tools" {
            let specs = self.mcp.tool_specs();
            if specs.is_empty() {
                println!("{} No MCP tools are currently connected.", "•".dimmed());
            } else {
                println!(
                    "{}",
                    "MCP tools exposed to the model:".bright_green().bold()
                );
                for tool in specs {
                    let label = self
                        .mcp
                        .tool_label(&tool.name)
                        .unwrap_or_else(|| tool.name.clone());
                    println!("  {} {}", tool.name.cyan(), format!("({label})").dimmed());
                }
            }
            return;
        }
        if argument == "refresh" {
            self.settings.mcp_servers =
                ikode::settings::LocalSettings::load(&self.working_directory).mcp_servers;
            self.reload_mcp().await;
            self.print_mcp_status();
            return;
        }

        let (command, rest) = argument
            .split_once(char::is_whitespace)
            .map(|(command, rest)| (command, rest.trim()))
            .unwrap_or((argument, ""));
        let original = self.settings.clone();
        let message = match command {
            "add" => match parse_add_server(rest) {
                Ok((name, config)) => {
                    let replaced = self
                        .settings
                        .mcp_servers
                        .insert(name.clone(), config)
                        .is_some();
                    if replaced {
                        format!("Updated MCP server '{name}'")
                    } else {
                        format!("Registered MCP server '{name}'")
                    }
                }
                Err(error) => {
                    println!("{} {error}", "⚠️ ".bright_yellow());
                    return;
                }
            },
            "remove" => {
                if rest.is_empty() {
                    println!("{} Usage: /mcp remove <name>", "⚠️ ".bright_yellow());
                    return;
                }
                if self.settings.mcp_servers.remove(rest).is_none() {
                    println!("{} No MCP server named '{}'.", "⚠️ ".bright_yellow(), rest);
                    return;
                }
                format!("Removed MCP server '{rest}'")
            }
            "enable" | "disable" => {
                if rest.is_empty() {
                    println!("{} Usage: /mcp {command} <name>", "⚠️ ".bright_yellow());
                    return;
                }
                let Some(config) = self.settings.mcp_servers.get_mut(rest) else {
                    println!("{} No MCP server named '{}'.", "⚠️ ".bright_yellow(), rest);
                    return;
                };
                let enabled = command == "enable";
                config.set_enabled(enabled);
                format!(
                    "{} MCP server '{rest}'",
                    if enabled { "Enabled" } else { "Disabled" }
                )
            }
            _ => {
                println!(
                    "{} Usage: /mcp [list|tools|refresh|add|remove|enable|disable]",
                    "⚠️ ".bright_yellow()
                );
                return;
            }
        };

        if let Err(error) = self.settings.save(&self.working_directory) {
            self.settings = original;
            println!(
                "{} Could not persist MCP registration; no change was kept: {}",
                "⚠️ ".bright_yellow(),
                error
            );
            return;
        }
        self.reload_mcp().await;
        println!("{} {}.", "✅ ".bright_green(), message);
        self.print_mcp_status();
    }

    pub(crate) fn handle_graph_command(&mut self, argument: &str) {
        let argument = argument.trim();
        let command_parts = argument.split_whitespace().collect::<Vec<_>>();
        if matches!(command_parts.as_slice(), [] | ["status"]) {
            if !self.graph_enabled {
                println!(
                    "{} Graph mode: {} — traditional file/shell harness.",
                    "🕸️ ".bright_blue(),
                    "off".bright_yellow().bold()
                );
            } else if let Some(stats) = self.indexer.graph_stats() {
                println!("{}", harness::render_graph_stats(&stats));
            } else {
                println!(
                    "{} Graph mode is on; persistence is unavailable.",
                    "⚠️ ".bright_yellow()
                );
            }
            return;
        }

        if !matches!(
            command_parts.as_slice(),
            ["on"]
                | ["off"]
                | ["on", "save"]
                | ["off", "save"]
                | ["on", "save", "global"]
                | ["off", "save", "global"]
                | ["save"]
                | ["save", "global"]
        ) {
            println!(
                "Usage: /graph [status] | /graph on|off [save [global]] | /graph save [global]"
            );
            return;
        }

        let mut parts = command_parts.iter().copied();
        match parts.next() {
            Some("on") | Some("off") => {
                let enabled = argument.starts_with("on");
                if self.graph_enabled != enabled {
                    self.graph_enabled = enabled;
                    self.indexer = if enabled {
                        ikode::index::Indexer::new(self.working_directory.clone())
                    } else {
                        ikode::index::Indexer::new_ephemeral(self.working_directory.clone())
                    };
                    self.session_cache_key = Uuid::new_v4().to_string();
                }
                let save = parts.next();
                if save == Some("save") {
                    let scope = if parts.next() == Some("global") {
                        ConfigScope::Global
                    } else {
                        ConfigScope::Project
                    };
                    match harness::save_config_typed(
                        scope,
                        &self.working_directory,
                        "graph_enabled",
                        enabled,
                    ) {
                        Ok(path) => println!(
                            "{} Graph mode set to {} and saved to {}.",
                            "✅ ".bright_green(),
                            if enabled { "on" } else { "off" },
                            path.display()
                        ),
                        Err(error) => println!(
                            "{} Graph mode changed for this session but could not be saved: {}",
                            "⚠️ ".bright_yellow(),
                            error
                        ),
                    }
                } else if save.is_some() {
                    println!(
                        "{} Graph mode changed for this session. Usage: /graph on|off [save [global]]",
                        "⚠️ ".bright_yellow()
                    );
                } else {
                    println!(
                        "{} Graph mode set to {} for this session (add 'save' to persist).",
                        "✅ ".bright_green(),
                        if enabled { "on" } else { "off" }
                    );
                }
            }
            Some("save") => {
                let scope = if parts.next() == Some("global") {
                    ConfigScope::Global
                } else {
                    ConfigScope::Project
                };
                match harness::save_config_typed(
                    scope,
                    &self.working_directory,
                    "graph_enabled",
                    self.graph_enabled,
                ) {
                    Ok(path) => println!(
                        "{} Saved graph_enabled = {} to {}.",
                        "💾".bright_green(),
                        self.graph_enabled,
                        path.display()
                    ),
                    Err(error) => {
                        println!("{} Could not save config: {}", "⚠️ ".bright_yellow(), error)
                    }
                }
            }
            _ => println!(
                "{} Usage: /graph [status|on|off|save] [global]",
                "⚠️ ".bright_yellow()
            ),
        }
    }

    /// `/resume` — pick a saved session from `.ikode/sessions/` and continue it.
    pub(crate) fn run_resume(&mut self) {
        let infos = session::list_sessions(&self.working_directory);
        if infos.is_empty() {
            println!("{} No saved sessions yet.", "•".dimmed());
            return;
        }
        let labels: Vec<String> = infos
            .iter()
            .map(|s| {
                format!(
                    "{}  {}  {}  {}  \"{}\"{}",
                    short_id(&s.id).cyan(),
                    rel_time(s.modified).dimmed(),
                    format!("{} msgs", s.message_count).dimmed(),
                    human_bytes(s.file_bytes).dimmed(),
                    truncate_preview(&s.preview, 60),
                    if s.malformed_lines > 0 || s.read_error.is_some() {
                        "  [damaged]".bright_yellow().to_string()
                    } else {
                        String::new()
                    }
                )
            })
            .collect();
        let current = self.session.id.clone();
        match Select::new()
            .with_prompt("Resume which session?")
            .items(&labels)
            .default(0)
            .interact_opt()
        {
            Ok(Some(idx)) if infos[idx].id != current => {
                if let Err(error) = self.load_session(&infos[idx]) {
                    println!(
                        "{} Could not resume session: {}",
                        "⚠️ ".bright_yellow(),
                        error
                    );
                }
            }
            Ok(Some(_)) => println!("{} Already in that session.", "•".dimmed()),
            _ => println!("{} Resume cancelled.", "•".dimmed()),
        }
    }

    /// `/fork` — clone the active transcript into a fresh saved session and switch
    /// the REPL to that branch. The parent file is left byte-for-byte untouched.
    pub(crate) fn run_fork(&mut self) {
        if self.history.len() <= 1 {
            println!(
                "{} Nothing to fork yet — send a message first.",
                "•".dimmed()
            );
            return;
        }

        let parent_id = self.session.id.clone();
        let branch_id = Uuid::new_v4().to_string();
        let branch =
            match session::Session::fork(&self.working_directory, branch_id.clone(), &self.history)
            {
                Ok(branch) => branch,
                Err(error) => {
                    println!(
                        "{} Could not fork session: {}",
                        "⚠️ ".bright_yellow(),
                        error
                    );
                    return;
                }
            };

        // A fork is a new chat started today. Only the in-memory system message
        // changes; the cloned user/assistant transcript remains exact and the new
        // file still omits system content like every other saved session.
        let today = Self::local_date_string();
        if today != self.system_prompt_date {
            self.system_prompt = self.system_prompt.replace(&self.system_prompt_date, &today);
            self.system_prompt_date = today;
            self.history[0] = self.system_message();
        }

        self.session = branch;
        self.session_cache_key = Uuid::new_v4().to_string();
        self.session_input_tokens = 0;
        self.session_output_tokens = 0;
        self.session_cached_tokens = 0;
        self.last_input_tokens = 0;
        self.current_turn_images.clear();
        self.current_turn_documents.clear();

        println!(
            "{} Forked {} → {}. The parent remains available through /resume.",
            "⑂ ".bright_green(),
            short_id(&parent_id).cyan(),
            short_id(&branch_id).bright_cyan().bold()
        );
    }

    /// Persist a config key to project (default) or global (`rest == "global"`)
    /// config, preserving comments. Used by `/model save` and `/emodel save`.
    fn save_setting(&self, key: &str, value: &str, rest: Option<&str>) {
        let scope = match rest.map(str::trim) {
            Some("global") => ConfigScope::Global,
            _ => ConfigScope::Project,
        };
        match harness::save_config_value(scope, &self.working_directory, key, value) {
            Ok(path) => println!(
                "{} Saved {} = {} to {}",
                "💾".bright_green(),
                key,
                value.bright_magenta(),
                path.display()
            ),
            Err(e) => println!("{} Could not save config: {}", "⚠️ ".bright_yellow(), e),
        }
    }

    fn save_numeric_setting(&self, key: &str, value: usize, rest: Option<&str>) -> bool {
        let scope = match rest.map(str::trim) {
            None => ConfigScope::Project,
            Some("global") => ConfigScope::Global,
            Some(other) => {
                println!(
                    "{} Expected 'global' after save, got '{}'.",
                    "⚠️ ".bright_yellow(),
                    other
                );
                return false;
            }
        };
        let value = match i64::try_from(value) {
            Ok(value) => value,
            Err(_) => {
                println!("{} Value is too large to persist.", "⚠️ ".bright_yellow());
                return false;
            }
        };
        match harness::save_config_typed(scope, &self.working_directory, key, value) {
            Ok(path) => {
                println!(
                    "{} Saved {} = {} to {}",
                    "💾".bright_green(),
                    key,
                    value.to_string().bright_magenta(),
                    path.display()
                );
                true
            }
            Err(error) => {
                println!("{} Could not save config: {}", "⚠️ ".bright_yellow(), error);
                false
            }
        }
    }

    fn handle_permission_command(&mut self, allow: bool, argument: &str) {
        let label = if allow { "allow" } else { "deny" };
        let rules = if allow {
            &self.settings.permissions.allow
        } else {
            &self.settings.permissions.deny
        };
        let argument = argument.trim();
        if argument.is_empty() || argument == "list" {
            if rules.is_empty() {
                println!("{} No {} rules configured.", "•".dimmed(), label);
            } else {
                println!(
                    "{} rules:",
                    label.to_ascii_uppercase().bright_white().bold()
                );
                for (index, rule) in rules.iter().enumerate() {
                    println!("  {}. {}", index + 1, rule.cyan());
                }
            }
            return;
        }

        let original = self.settings.clone();
        let result = if argument == "clear" {
            let count = rules.len();
            if allow {
                self.settings.permissions.allow.clear();
            } else {
                self.settings.permissions.deny.clear();
            }
            format!("Cleared {count} {label} rule(s)")
        } else if let Some(target) = argument.strip_prefix("remove ").map(str::trim) {
            let rules = if allow {
                &mut self.settings.permissions.allow
            } else {
                &mut self.settings.permissions.deny
            };
            let index = target
                .parse::<usize>()
                .ok()
                .and_then(|index| index.checked_sub(1))
                .filter(|index| *index < rules.len())
                .or_else(|| rules.iter().position(|rule| rule == target));
            let Some(index) = index else {
                println!(
                    "{} No {} rule matching '{}'.",
                    "⚠️ ".bright_yellow(),
                    label,
                    target
                );
                return;
            };
            let removed = rules.remove(index);
            format!("Removed {label} rule: {removed}")
        } else {
            if let Err(error) = validate_rule(argument) {
                println!(
                    "{} Invalid {} rule '{}': {}.",
                    "⚠️ ".bright_yellow(),
                    label,
                    argument,
                    error
                );
                return;
            }
            let rules = if allow {
                &mut self.settings.permissions.allow
            } else {
                &mut self.settings.permissions.deny
            };
            if rules.iter().any(|rule| rule == argument) {
                println!("{} Rule already exists: {}", "•".dimmed(), argument.cyan());
                return;
            }
            rules.push(argument.to_string());
            format!("Added {label} rule: {argument}")
        };

        match self.settings.save(&self.working_directory) {
            Ok(()) => println!("{} {}", "✅ ".bright_green(), result),
            Err(error) => {
                self.settings = original;
                println!(
                    "{} Could not persist {} rules; no in-memory change was kept: {}",
                    "⚠️ ".bright_yellow(),
                    label,
                    error
                );
            }
        }
    }

    fn print_storage(&self) {
        let infos = session::list_sessions(&self.working_directory);
        let sessions = session::storage_stats(&infos);
        println!("{} Storage:", "💽".bright_blue());
        if let Some(wal) = self.indexer.wal_stats() {
            println!(
                "  Graph WAL:       {} ({} since checkpoint, {} compressed away)",
                human_bytes(wal.file_bytes).bright_magenta(),
                human_bytes(wal.bytes_since_checkpoint),
                human_bytes(wal.compression_saved_bytes())
            );
        } else {
            println!(
                "  Graph WAL:       {} (persistence unavailable)",
                human_bytes(self.indexer.wal_size_bytes()).bright_yellow()
            );
        }
        println!(
            "  Sessions:        {} across {} files / {} messages (largest {})",
            human_bytes(sessions.bytes).bright_magenta(),
            sessions.files,
            sessions.messages,
            human_bytes(sessions.largest_bytes)
        );
        if sessions.corrupt_files > 0 || sessions.incomplete_files > 0 {
            println!(
                "  Transcript health: {} damaged, {} recoverable incomplete tail(s)",
                sessions.corrupt_files.to_string().bright_yellow(),
                sessions.incomplete_files
            );
        }
    }

    fn warn_session_budget(&self) {
        let Some(limit) = ProjectConfig::resolve(&self.working_directory).session_max_bytes else {
            return;
        };
        let infos = session::list_sessions(&self.working_directory);
        let bytes = session::storage_stats(&infos).bytes;
        if bytes > limit {
            println!(
                "{} Saved sessions use {} (configured budget {}). Run {} for a dry-run cleanup plan.",
                "🗜️ ".bright_yellow(),
                human_bytes(bytes).bright_magenta(),
                human_bytes(limit),
                "/sessions prune".cyan()
            );
        }
    }

    fn handle_sessions(&mut self, argument: &str) {
        let argument = argument.trim();
        if argument.is_empty() {
            self.print_storage();
            println!(
                "  {}",
                "Use /sessions prune ... to preview retention cleanup.".dimmed()
            );
            return;
        }
        let Some(rest) = argument.strip_prefix("prune") else {
            println!(
                "{} Usage: /sessions prune [--keep N] [--max-bytes SIZE] [--apply]",
                "⚠️ ".bright_yellow()
            );
            return;
        };

        let mut keep = None;
        let mut max_bytes = None;
        let mut apply = false;
        let mut tokens = rest.split_whitespace();
        while let Some(token) = tokens.next() {
            match token {
                "--keep" => {
                    let Some(value) = tokens.next() else {
                        println!("{} --keep requires a number.", "⚠️ ".bright_yellow());
                        return;
                    };
                    keep = match value.parse::<usize>() {
                        Ok(value) => Some(value),
                        Err(_) => {
                            println!(
                                "{} Invalid --keep value '{}'.",
                                "⚠️ ".bright_yellow(),
                                value
                            );
                            return;
                        }
                    };
                }
                "--max-bytes" => {
                    let Some(value) = tokens.next() else {
                        println!("{} --max-bytes requires a size.", "⚠️ ".bright_yellow());
                        return;
                    };
                    max_bytes = match parse_byte_size(value) {
                        Ok(value) => Some(value),
                        Err(error) => {
                            println!("{} {}", "⚠️ ".bright_yellow(), error);
                            return;
                        }
                    };
                }
                "--apply" => apply = true,
                unknown => {
                    println!(
                        "{} Unknown prune option '{}'.",
                        "⚠️ ".bright_yellow(),
                        unknown
                    );
                    return;
                }
            }
        }

        let config = ProjectConfig::resolve(&self.working_directory);
        keep = keep.or(config.session_keep);
        max_bytes = max_bytes.or(config.session_max_bytes);
        if keep.is_none() && max_bytes.is_none() {
            println!(
                "{} Supply --keep or --max-bytes, or configure session_keep/session_max_bytes.",
                "⚠️ ".bright_yellow()
            );
            return;
        }

        let infos = session::list_sessions(&self.working_directory);
        let mut protected = HashSet::from([self.session.id.clone()]);
        protected.extend(
            self.goals
                .tasks
                .iter()
                .filter(|task| matches!(task.status, GoalStatus::Active | GoalStatus::Blocked))
                .map(|task| task.session_id.clone()),
        );
        let plan = session::plan_prune(&infos, keep, max_bytes, &protected);
        println!(
            "{} Session prune {}: {} → {}, {} → {}",
            if apply { "🧹" } else { "🔎" }.bright_cyan(),
            if apply { "result" } else { "dry run" },
            plan.before_files,
            plan.after_files,
            human_bytes(plan.before_bytes),
            human_bytes(plan.after_bytes)
        );
        if plan.remove.is_empty() {
            println!("  {}", "Nothing is eligible for removal.".dimmed());
        } else {
            for info in &plan.remove {
                println!(
                    "  {} {}  {}  \"{}\"",
                    if apply { "remove" } else { "would remove" }.bright_yellow(),
                    short_id(&info.id).cyan(),
                    human_bytes(info.file_bytes),
                    truncate_preview(&info.preview, 48)
                );
            }
        }
        if !plan.target_met {
            println!("{} Retention target cannot be met without deleting the current or an active goal session.", "⚠️ ".bright_yellow());
        }
        if !apply && !plan.remove.is_empty() {
            println!(
                "  {}",
                "Re-run with --apply to perform this exact policy.".dimmed()
            );
            return;
        }
        if apply {
            let failures = session::apply_prune(&plan);
            if failures.is_empty() {
                println!(
                    "{} Removed {} session(s).",
                    "✅ ".bright_green(),
                    plan.remove.len()
                );
            } else {
                for (id, error) in failures {
                    println!(
                        "{} Could not remove {}: {}",
                        "⚠️ ".bright_yellow(),
                        short_id(&id),
                        error
                    );
                }
            }
        }
    }

    fn run_doctor(&self) {
        println!("{} iKode diagnostics", "🩺".bright_cyan());
        println!("  Project root:    {}", self.working_directory.display());
        println!("  Chat model:      {}", self.model.bright_magenta());
        println!(
            "  Embedding model: {}",
            self.embedding_model.bright_magenta()
        );
        println!("  Summary model:   {}", self.summary_model.bright_magenta());

        let mut providers = std::collections::BTreeSet::new();
        for model in [&self.model, &self.embedding_model, &self.summary_model] {
            providers.insert(
                model
                    .split_once("::")
                    .map(|(provider, _)| provider)
                    .unwrap_or("unknown"),
            );
        }
        for provider in providers {
            let status = match provider {
                "openai" => credential_status("OPENAI_API_KEY"),
                "anthropic" => credential_status("ANTHROPIC_API_KEY"),
                "gemini" => credential_status("GEMINI_API_KEY"),
                "ollama" => "local provider; no API key required".to_string(),
                "bedrock" => credential_status("AWS_REGION"),
                "vertexai" | "vertex" => vertex_status(),
                _ => "unknown provider prefix".to_string(),
            };
            println!("  Provider {:<9} {}", format!("{provider}:"), status);
        }
        let project_config = harness::project_config_path(&self.working_directory);
        println!(
            "  Project config:  {} ({})",
            project_config.display(),
            if project_config.is_file() {
                "present"
            } else {
                "not created"
            }
        );
        println!(
            "  Local settings:  {} ({})",
            ikode::settings::settings_path(&self.working_directory).display(),
            if ikode::settings::settings_path(&self.working_directory).is_file() {
                "present"
            } else {
                "defaults"
            }
        );
        match (self.graph_enabled, self.indexer.graph_stats()) {
            (false, _) => println!("  Graph mode:      disabled (traditional harness)"),
            (true, Some(graph)) => println!(
                "  Graph:           healthy ({} nodes, {} edges)",
                graph.nodes_total, graph.edges_total
            ),
            (true, None) => println!("  Graph:           unavailable/in-memory only"),
        }
        match &self.web_config {
            Some(web) => println!("  Web research:    enabled — {}", web.describe()),
            None => println!(
                "  Web research:    disabled (set BRAVE_API_KEY/TAVILY_API_KEY or web_search_endpoint)"
            ),
        }
        let mcp_statuses = self.mcp.statuses(&self.settings.mcp_servers);
        println!(
            "  MCP:             {}/{} connected, {} tools",
            mcp_statuses
                .iter()
                .filter(|status| status.connected)
                .count(),
            mcp_statuses.len(),
            mcp_statuses
                .iter()
                .map(|status| status.tool_count)
                .sum::<usize>()
        );
        self.print_storage();
    }

    /// `/goal` — show the active goal: objective, acceptance criteria, step progress.
    fn show_active_goal(&self) {
        match self.goals.active() {
            None => println!(
                "{} No active goal. Start one with {}.",
                "•".dimmed(),
                "/goal <objective>".cyan()
            ),
            Some(t) => {
                println!(
                    "{} Goal {} [{}]: {}",
                    t.status.glyph(),
                    short_id(&t.id).cyan(),
                    t.status.as_str().bright_magenta(),
                    t.objective.bright_white().bold()
                );
                if t.acceptance.is_empty() {
                    println!("  {}", "acceptance: not yet defined".dimmed());
                } else {
                    println!("  acceptance:");
                    for c in &t.acceptance {
                        println!("    - {c}");
                    }
                }
                // Step progress comes from the shared todo list (the in-task tracker).
                if !self.todos.is_empty() {
                    let done = self.todos.iter().filter(|x| x.completed).count();
                    println!("  steps: {}/{} done", done, self.todos.len());
                }
            }
        }
    }

    /// `/goals` — list every goal task with its status (the multi-task view).
    fn list_goals(&self) {
        if self.goals.tasks.is_empty() {
            println!(
                "{} No goals yet. Start one with {}.",
                "•".dimmed(),
                "/goal <objective>".cyan()
            );
            return;
        }
        println!("{}", "Goals:".bright_green().bold());
        for t in &self.goals.tasks {
            let active = self.goals.active_id.as_deref() == Some(t.id.as_str());
            let marker = if active {
                "*".bright_cyan().to_string()
            } else {
                " ".to_string()
            };
            println!(
                "  {}{} {} [{}] {}",
                marker,
                t.status.glyph(),
                short_id(&t.id).cyan(),
                t.status.as_str().dimmed(),
                truncate_preview(&t.objective, 60)
            );
        }
    }

    /// `/goal done` | `/goal abandon` — close the active goal with the given status.
    fn close_active_goal(&mut self, status: GoalStatus) {
        let id = match self.goals.active_id.clone() {
            Some(id) => id,
            None => {
                println!("{} No active goal to close.", "•".dimmed());
                return;
            }
        };
        self.goals.set_status(&id, status);
        self.goals.active_id = None;
        if let Err(e) = self.goals.save(&self.working_directory) {
            eprintln!("{} could not save goals: {e}", "⚠️ ".yellow());
        }
        println!(
            "{} Goal {} marked {}.",
            "✅ ".bright_green(),
            short_id(&id).cyan(),
            status.as_str().bright_magenta()
        );
    }

    fn clear_screen() {
        if cfg!(windows) {
            let _ = Command::new("cmd").args(["/c", "cls"]).status();
        } else {
            let _ = Command::new("clear").status();
        }
    }

    /// Print the current operating mode and any allow/deny rules in effect.
    /// Handle `/image` (and `/img`). With a path, validate and queue the image for
    /// the next message; bare lists what's queued; `clear` discards the queue.
    fn handle_image_attach(&mut self, arg: &str) {
        if arg.is_empty() {
            if self.pending_images.is_empty() {
                println!(
                    "{} No images queued. Use {} to attach one for your next message.",
                    "🖼️ ".bright_blue(),
                    "/image <path>".cyan()
                );
            } else {
                println!(
                    "{} {} image(s) queued for your next message:",
                    "🖼️ ".bright_blue(),
                    self.pending_images.len()
                );
                for img in &self.pending_images {
                    println!("   • {}", img.label);
                }
                println!("{}", "   (/image clear to discard)".dimmed());
            }
            return;
        }
        if arg == "clear" {
            let n = self.pending_images.len();
            self.pending_images.clear();
            println!("{} Cleared {} queued image(s).", "🧹".bright_cyan(), n);
            return;
        }
        match crate::image::load_image(arg, &self.working_directory) {
            Ok(img) => {
                println!(
                    "{} Attached {} — sent with your next message.",
                    "🖼️ ".bright_blue(),
                    img.label.cyan()
                );
                self.pending_images.push(img);
            }
            Err(e) => println!("{} {}", "⚠️ ".bright_yellow(), e),
        }
    }

    /// Handle `/document` (and `/doc`). UTF-8 files work with every provider;
    /// binary formats are checked against the selected provider before queueing.
    fn handle_document_attach(&mut self, arg: &str) {
        if arg.is_empty() {
            if self.pending_documents.is_empty() {
                println!(
                    "No documents queued. Use {} to attach one for your next message.",
                    "/document <path>".cyan()
                );
            } else {
                println!(
                    "{} document(s) queued for your next message:",
                    self.pending_documents.len()
                );
                for document in &self.pending_documents {
                    println!("   - {}", document.label);
                }
                println!("{}", "   (/document clear to discard)".dimmed());
            }
            return;
        }
        if arg == "clear" {
            let count = self.pending_documents.len();
            self.pending_documents.clear();
            println!("Cleared {count} queued document(s).");
            return;
        }

        match crate::document::load_document(arg, &self.working_directory) {
            Ok(document) => {
                if let Some(error) = document.support_error(&self.model) {
                    println!("{} {}", "Warning:".bright_yellow(), error);
                    return;
                }
                println!(
                    "Attached {} - sent with your next message.",
                    document.label.cyan()
                );
                self.pending_documents.push(document);
            }
            Err(error) => println!("{} {}", "Warning:".bright_yellow(), error),
        }
    }

    /// Generic `/attach`: images and documents share one discoverable entry point,
    /// while `/image` and `/document` remain useful explicit controls.
    fn handle_any_attach(&mut self, arg: &str) {
        if arg.is_empty() {
            if self.pending_images.is_empty() && self.pending_documents.is_empty() {
                println!(
                    "Nothing queued. Use {} with an image or document path.",
                    "/attach <path>".cyan()
                );
                return;
            }
            println!(
                "Queued for your next message: {} image(s), {} document(s).",
                self.pending_images.len(),
                self.pending_documents.len()
            );
            for image in &self.pending_images {
                println!("   - image: {}", image.label);
            }
            for document in &self.pending_documents {
                println!("   - document: {}", document.label);
            }
            println!("{}", "   (/attach clear to discard all)".dimmed());
            return;
        }
        if arg == "clear" {
            let images = self.pending_images.len();
            let documents = self.pending_documents.len();
            self.pending_images.clear();
            self.pending_documents.clear();
            println!("Cleared {images} image(s) and {documents} document(s).");
            return;
        }

        match crate::image::load_image(arg, &self.working_directory) {
            Ok(image) => {
                println!(
                    "Attached {} - sent with your next message.",
                    image.label.cyan()
                );
                self.pending_images.push(image);
            }
            Err(image_error) if crate::image::has_supported_extension(arg) => {
                println!("{} {}", "Warning:".bright_yellow(), image_error);
            }
            Err(_) => match crate::document::load_document(arg, &self.working_directory) {
                Ok(document) => {
                    if let Some(error) = document.support_error(&self.model) {
                        println!("{} {}", "Warning:".bright_yellow(), error);
                        return;
                    }
                    println!(
                        "Attached {} - sent with your next message.",
                        document.label.cyan()
                    );
                    self.pending_documents.push(document);
                }
                Err(document_error) => {
                    println!("{} {}", "Warning:".bright_yellow(), document_error);
                }
            },
        }
    }

    /// `/vars` — the environment variables iKode reads, with credentials masked
    /// to `***` + last four characters. Values reflect the live process
    /// environment, so anything loaded from `.env`/`.ikenv` files at startup is
    /// included; full secrets are never printed.
    pub(crate) fn print_vars(&self) {
        // (group, name, is_secret) — grouped in display order. Kept beside the
        // command so adding a provider means touching one table.
        const VARS: &[(&str, &str, bool)] = &[
            ("Providers", "OPENAI_API_KEY", true),
            ("Providers", "OPENAI_API_URL", false),
            ("Providers", "OPENAI_API_TIER", false),
            ("Providers", "ANTHROPIC_API_KEY", true),
            ("Providers", "ANTHROPIC_API_URL", false),
            ("Providers", "GEMINI_API_KEY", true),
            ("Providers", "GEMINI_API_URL", false),
            ("Providers", "OLLAMA_URL", false),
            ("Providers", "AWS_REGION", false),
            ("Providers", "BEDROCK_REGION", false),
            ("Providers", "VERTEXAI_API_URL", false),
            ("Providers", "VERTEXAI_API_TIER", false),
            ("Providers", "VERTEXAI_SA_PATH", false),
            ("Providers", "GOOGLE_ACCOUNT_ID", false),
            ("Providers", "GOOGLE_PRIVATE_KEY", true),
            ("Web research", "BRAVE_API_KEY", true),
            ("Web research", "TAVILY_API_KEY", true),
            ("Web research", "SEARXNG_URL", false),
        ];
        println!("{}", "Environment variables:".bright_green().bold());
        let mut current_group = "";
        for (group, name, secret) in VARS {
            if *group != current_group {
                println!("  {}", group.bright_white().bold());
                current_group = group;
            }
            let value = match std::env::var(name) {
                Ok(value) if !value.trim().is_empty() => {
                    if *secret {
                        ikode::util::mask_secret(&value).bright_magenta().to_string()
                    } else {
                        value.bright_magenta().to_string()
                    }
                }
                _ => "not set".dimmed().to_string(),
            };
            println!("    {:<20} {}", name.cyan(), value);
        }
        println!(
            "  {}",
            "Values include anything loaded from .env / .ikenv / .ikode/.env at startup. Keys are masked; iKode never prints full secrets."
                .dimmed()
        );
    }

    pub(crate) fn print_mode(&self) {
        println!(
            "{} Mode: {} — {}",
            "🛡️ ".bright_blue(),
            self.settings.mode.as_str().bright_magenta().bold(),
            self.settings.mode.describe().dimmed()
        );
        if !self.settings.permissions.allow.is_empty() {
            println!(
                "  allow: {}",
                self.settings.permissions.allow.join(", ").green()
            );
        }
        if !self.settings.permissions.deny.is_empty() {
            println!(
                "  deny:  {}",
                self.settings.permissions.deny.join(", ").red()
            );
        }
        println!(
            "  {}",
            "switch with /mode plan|agentic|yolo · add rules with /allow|/deny".dimmed()
        );
    }
}

fn credential_status(name: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => format!("{name} is set"),
        _ => format!("{name} is missing"),
    }
}

fn vertex_status() -> String {
    let path = match std::env::var("VERTEXAI_SA_PATH") {
        Ok(path) if !path.trim().is_empty() => path,
        _ => return "VERTEXAI_SA_PATH is missing".to_string(),
    };
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) => return format!("cannot read VERTEXAI_SA_PATH: {error}"),
    };
    let account: serde_json::Value = match serde_json::from_str(&contents) {
        Ok(account) => account,
        Err(error) => return format!("service-account JSON is invalid: {error}"),
    };
    if account["private_key"].as_str().is_some() && account["client_email"].as_str().is_some() {
        "service account is readable".to_string()
    } else {
        "service account lacks private_key or client_email".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaise_core::contracts::{GaiseContent, GaiseMessage, OneOrMany};

    fn user(text: &str) -> GaiseMessage {
        GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: text.to_string(),
            })),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
        }
    }

    fn app(root: &std::path::Path) -> App {
        App::new(
            "openai::fake".to_string(),
            "openai::fake-embedding".to_string(),
            "openai::fake-summary".to_string(),
            false,
            None,
            100,
            3,
            root.to_path_buf(),
            true,
            ikode::settings::LocalSettings::default(),
            None,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn fork_switches_to_exact_saved_branch_and_resets_session_counters() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app(root.path());
        app.record(user("parent question"));
        app.session_input_tokens = 11;
        app.session_output_tokens = 7;
        app.session_cached_tokens = 3;
        app.last_input_tokens = 9;
        app.current_turn_images.push(GaiseContent::Text {
            text: "ephemeral".to_string(),
        });
        app.current_turn_documents.push(GaiseContent::Text {
            text: "ephemeral document".to_string(),
        });
        app.pending_images.push(crate::image::LoadedImage {
            label: "queued.png".to_string(),
            media_type: "image/png".to_string(),
            data: vec![1, 2, 3],
        });
        std::fs::write(root.path().join("queued.txt"), "queued document").unwrap();
        app.pending_documents
            .push(crate::document::load_document("queued.txt", root.path()).unwrap());

        let parent_id = app.session.id.clone();
        let parent_path = app.session.path.clone();
        let parent_bytes = std::fs::read(&parent_path).unwrap();
        let old_cache_key = app.session_cache_key.clone();

        app.run_fork();

        assert_ne!(app.session.id, parent_id);
        assert_ne!(app.session_cache_key, old_cache_key);
        assert_eq!(std::fs::read(&parent_path).unwrap(), parent_bytes);
        assert_eq!(std::fs::read(&app.session.path).unwrap(), parent_bytes);
        assert_eq!(session::load_messages(&app.session.path).unwrap().len(), 1);
        assert_eq!(app.history.len(), 2);
        assert_eq!(app.session_input_tokens, 0);
        assert_eq!(app.session_output_tokens, 0);
        assert_eq!(app.session_cached_tokens, 0);
        assert_eq!(app.last_input_tokens, 0);
        assert!(app.current_turn_images.is_empty());
        assert!(app.current_turn_documents.is_empty());
        assert_eq!(
            app.pending_images.len(),
            1,
            "queued next-turn images survive"
        );
        assert_eq!(
            app.pending_documents.len(),
            1,
            "queued next-turn documents survive"
        );
    }

    #[tokio::test]
    async fn fork_without_messages_or_with_write_failure_keeps_parent_active() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app(root.path());
        let empty_id = app.session.id.clone();
        app.run_fork();
        assert_eq!(app.session.id, empty_id);
        assert!(!app.session.path.exists());

        app.record(user("persisted parent"));
        app.session_input_tokens = 17;
        let parent_id = app.session.id.clone();
        let parent_path = app.session.path.clone();
        let parent_bytes = std::fs::read(&parent_path).unwrap();

        let blocked = tempfile::tempdir().unwrap();
        std::fs::write(blocked.path().join(".ikode"), "not a directory").unwrap();
        app.working_directory = blocked.path().to_path_buf();
        app.run_fork();

        assert_eq!(app.session.id, parent_id);
        assert_eq!(app.session.path, parent_path);
        assert_eq!(std::fs::read(&parent_path).unwrap(), parent_bytes);
        assert_eq!(app.session_input_tokens, 17);
    }

    #[tokio::test]
    async fn graph_command_switches_tool_surface_and_persists_config() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app(root.path());
        let old_cache_key = app.session_cache_key.clone();

        app.handle_graph_command("off unexpectedly");
        assert!(
            app.graph_enabled,
            "invalid syntax must not change graph mode"
        );

        app.handle_graph_command("off save");
        assert!(!app.graph_enabled);
        assert_ne!(app.session_cache_key, old_cache_key);
        assert!(!app
            .request_tools(Mode::Agentic, false)
            .iter()
            .any(|tool| tool.name == "ask_codebase"));
        assert_eq!(ProjectConfig::load(root.path()).graph_enabled, Some(false));

        app.handle_graph_command("on");
        assert!(app.graph_enabled);
        assert!(app
            .request_tools(Mode::Agentic, false)
            .iter()
            .any(|tool| tool.name == "ask_codebase"));
    }

    #[tokio::test]
    async fn mcp_command_persists_disabled_registration_without_launching_it() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app(root.path());

        app.handle_mcp_command("add local --disabled -- npx example-mcp")
            .await;
        assert!(matches!(
            app.settings.mcp_servers.get("local"),
            Some(ikode::mcp::McpServerConfig::Stdio { enabled: false, .. })
        ));
        let loaded = ikode::settings::LocalSettings::load(root.path());
        assert!(loaded.mcp_servers.contains_key("local"));

        app.handle_mcp_command("remove local").await;
        assert!(!app.settings.mcp_servers.contains_key("local"));
    }
}
