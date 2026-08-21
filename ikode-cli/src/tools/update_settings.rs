use super::{obj, p};
use crate::harness::{self, ConfigScope};
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct UpdateSettingsArgs {
    pub key: String,
    pub value: String,
}

/// The value type a settable key expects, so the tool can validate before writing.
enum Kind {
    Bool,
    Uint,
}

/// The config keys this tool is permitted to write, with their value type. Kept
/// deliberately narrow: the chat/embedding model have their own dedicated flows
/// (`/model`, `/emodel`) because changing the embedding model invalidates stored
/// vectors, so they are intentionally *not* settable here.
fn settable(key: &str) -> Option<Kind> {
    match key {
        "summarize_tests" | "auto_index" | "auto_enrich" | "brave" => Some(Kind::Bool),
        "summarize_min_lines"
        | "summarize_concurrency"
        | "max_history"
        | "prefix_keep"
        | "session_keep"
        | "session_max_bytes"
        | "web_max_searches"
        | "web_max_fetches"
        | "web_max_agents"
        | "web_agent_max_turns" => Some(Kind::Uint),
        _ => None,
    }
}

const SETTABLE_KEYS: &str =
    "summarize_tests, summarize_min_lines, summarize_concurrency, auto_index, auto_enrich, brave, max_history, prefix_keep, session_keep, session_max_bytes, web_max_searches, web_max_fetches, web_max_agents, web_agent_max_turns";

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "update_settings".to_string(),
        description: Some(format!(
            "Persist a single project setting to .ikode/config.toml (comment-preserving). \
             Use this when the user asks to change enrichment/indexing behaviour, e.g. \
             'also summarise tests' or 'set summary concurrency to 8'. Settable keys: {}. \
             Booleans accept true/false; the rest are non-negative integers. The change \
             takes effect on the next index/enrich pass.",
            SETTABLE_KEYS
        )),
        parameters: Some(obj(
            vec![
                (
                    "key",
                    p(
                        "string",
                        "The config key to set (see the description for the allowed list).",
                    ),
                ),
                (
                    "value",
                    p(
                        "string",
                        "The new value: true/false for booleans, a non-negative integer otherwise.",
                    ),
                ),
            ],
            vec!["key", "value"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: UpdateSettingsArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    let key = args.key.trim();
    let raw = args.value.trim();

    let kind = match settable(key) {
        Some(k) => k,
        None => {
            return Ok(format!(
                "Error: '{}' is not a settable key. Allowed: {}.",
                key, SETTABLE_KEYS
            ))
        }
    };

    println!(
        "{} Update setting: {} = {}",
        "⚙️".bright_cyan(),
        key.bold().bright_cyan(),
        raw.bright_cyan()
    );

    let action = format!("Set {} = {} in .ikode/config.toml", key, raw);
    if !host.ask_permission("update_settings", key, &action)? {
        return Ok(
            "Setting not changed (denied or cancelled). If in planning mode, switch with /mode agentic."
                .to_string(),
        );
    }

    let root = host.project_root().to_path_buf();
    let path = match kind {
        Kind::Bool => {
            let b = match raw.to_ascii_lowercase().as_str() {
                "true" | "yes" | "on" | "1" => true,
                "false" | "no" | "off" | "0" => false,
                _ => {
                    return Ok(format!(
                        "Error: '{}' expects true/false, got '{}'.",
                        key, raw
                    ))
                }
            };
            harness::save_config_typed(ConfigScope::Project, &root, key, b)?
        }
        Kind::Uint => {
            let n = match raw.parse::<u64>().ok().and_then(|n| i64::try_from(n).ok()) {
                Some(n) => n,
                None => {
                    return Ok(format!(
                        "Error: '{}' expects a non-negative integer, got '{}'.",
                        key, raw
                    ))
                }
            };
            harness::save_config_typed(ConfigScope::Project, &root, key, n)?
        }
    };

    Ok(format!(
        "Updated {} = {} in {}. Takes effect on the next index/enrich pass.",
        key,
        raw,
        path.display()
    ))
}
