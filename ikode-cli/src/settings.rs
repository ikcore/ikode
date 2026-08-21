//! Local, machine-specific settings (`.ikode/settings.local.json`): the operating
//! **mode** (plan / agentic / yolo), reasoning/delegation **effort**, per-action
//! **allow/deny** rules, and optional model overrides. Git-ignored by convention so
//! each developer keeps their own runtime posture without affecting committed config.
//!
//! Resolution: this file is the highest *local* layer — CLI flags still win, then
//! these settings, then the committed `.ikode/config.toml`, then global config.
//!
//! The permission engine here is purely algorithmic (string/glob matching, no
//! inference) and is the single source of truth consulted before any mutating
//! tool runs.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Does invoking `tool` change disk or run a shell command?
pub fn is_mutating(tool: &str) -> bool {
    crate::tools::is_mutating(tool) || crate::mcp::is_exposed_tool_name(tool)
}

/// Does invoking `tool` reach the public network (and spend money)? Network
/// tools are read-only — they stay available in plan mode — but they are never
/// auto-allowed the way other read-only tools are: queries leave the machine,
/// so they prompt in plan/agentic (honouring allow rules) and run freely only
/// in yolo. Deny rules always win.
pub fn is_network(tool: &str) -> bool {
    tool == "web_research"
}

/// Fallback context-size threshold (in prompt tokens) at which a conversation
/// auto-compacts when the chat model's context window is **unknown** (neither the
/// bundled GAISe registry nor the provider reports one). The common iKode models
/// (gpt-5.4-mini / -nano) have ~400k-token windows, so 256k leaves comfortable
/// headroom for the reply plus the kept tail.
pub const DEFAULT_AUTO_COMPACT_TOKENS: usize = 256_000;

/// Default share of the chat model's context window at which a conversation
/// auto-compacts. 80% leaves room for the reply, tool results, and the kept tail
/// before the provider starts rejecting requests.
pub const DEFAULT_AUTO_COMPACT_PERCENT: u8 = 80;

/// Where an auto-compact threshold came from — surfaced by `/compact auto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoCompactRule {
    /// Auto-compaction is off (`auto_compact_tokens` or `auto_compact_percent` = 0).
    Disabled,
    /// An explicit absolute `auto_compact_tokens` value.
    Tokens,
    /// `percent` of the model's known context window (`window` tokens).
    Percent { percent: u8, window: u64 },
    /// The window is unknown; [`DEFAULT_AUTO_COMPACT_TOKENS`] applies.
    Fallback,
}

/// The resolved auto-compact policy for one model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoCompactThreshold {
    /// Prompt-token count at or above which the conversation compacts (`0` = never).
    pub tokens: usize,
    pub rule: AutoCompactRule,
}

/// How much reasoning and agentic work the model should spend on a turn.
///
/// `Ultra` is intentionally more than an API scalar: it requests the deepest
/// provider-supported effort and enables proactive use of bounded subagents when
/// independent work would materially help. `Auto` preserves the provider/model
/// default and is the backwards-compatible default for existing settings files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    #[default]
    Auto,
    Low,
    #[serde(alias = "med", alias = "mid")]
    Medium,
    High,
    #[serde(alias = "xhigh", alias = "extra-high", alias = "extra_high")]
    Max,
    Ultra,
}

impl Effort {
    /// Parse console/config spellings. `med` is the short spelling requested by
    /// the console UI; `xhigh` is accepted as a compatibility alias for `max`.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" | "default" | "model" => Some(Self::Auto),
            "low" | "lo" => Some(Self::Low),
            "medium" | "med" | "mid" => Some(Self::Medium),
            "high" | "hi" => Some(Self::High),
            "max" | "xhigh" | "extra-high" | "extra_high" => Some(Self::Max),
            "ultra" => Some(Self::Ultra),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Max => "max",
            Self::Ultra => "ultra",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Self::Auto => "use the selected model's default",
            Self::Low => "fast, economical work for straightforward tasks",
            Self::Medium => "balanced reasoning for normal coding work",
            Self::High => "deeper checking for complex logic and edge cases",
            Self::Max => "maximum model reasoning for especially difficult work",
            Self::Ultra => "maximum reasoning plus proactive parallel subagents",
        }
    }

    /// Provider-facing effort value for the selected model. The console levels are
    /// intentionally stable even though provider vocabularies differ: OpenAI calls
    /// its common deepest API level `xhigh`, Claude uses `max`, and Gemini tops out
    /// at `high`. Ultra is also an orchestration policy, so it uses the same deepest
    /// scalar as Max and adds proactive delegation in the harness.
    pub fn api_value(self, model: &str) -> Option<&'static str> {
        let (provider, model_id) = model.split_once("::").unwrap_or(("", model));
        if self == Self::Auto {
            return None;
        }

        if provider == "openai" {
            // Known non-reasoning OpenAI families reject reasoning_effort instead
            // of ignoring it. Unknown/new reasoning models retain the conservative
            // low/medium/high mapping below.
            if model_id.starts_with("gpt-4")
                || model_id.starts_with("text-")
                || model_id.contains("embedding")
            {
                return None;
            }

            // The original GPT-5 Pro accepts only high. Later Pro models accept
            // medium/high/xhigh, so clamp a requested Low rather than sending a
            // value that the API rejects.
            if model_id == "gpt-5-pro" || model_id.starts_with("gpt-5-pro-") {
                return Some("high");
            }
            let pro_minimum_medium = ["gpt-5.2-pro", "gpt-5.4-pro", "gpt-5.5-pro"]
                .iter()
                .any(|prefix| model_id.starts_with(prefix));
            if self == Self::Low && pro_minimum_medium {
                return Some("medium");
            }

            return match self {
                Self::Low => Some("low"),
                Self::Medium => Some("medium"),
                Self::High => Some("high"),
                Self::Max | Self::Ultra if openai_supports_xhigh(model_id) => Some("xhigh"),
                Self::Max | Self::Ultra => Some("high"),
                Self::Auto => None,
            };
        }

        if provider == "anthropic" {
            // output_config.effort is not accepted by older Claude families. Opus
            // 4.5 supports the scalar but not max; current 4.6+ families support
            // max as their deepest portable level.
            if !anthropic_supports_effort(model_id) {
                return None;
            }
            return match self {
                Self::Low => Some("low"),
                Self::Medium => Some("medium"),
                Self::High => Some("high"),
                Self::Max | Self::Ultra if model_id.contains("opus-4-5") => Some("high"),
                Self::Max | Self::Ultra => Some("max"),
                Self::Auto => None,
            };
        }

        // Ollama has no reasoning-effort request field. Bedrock exposes effort
        // only for particular model-native contracts through
        // additionalModelRequestFields, so omit it for other Bedrock families.
        if provider == "ollama" {
            return None;
        }
        if provider == "bedrock" && !bedrock_supports_effort(model_id) {
            return None;
        }

        match self {
            Self::Auto => None,
            Self::Low => Some("low"),
            Self::Medium => Some("medium"),
            Self::High => Some("high"),
            Self::Max | Self::Ultra => Some("high"),
        }
    }

    pub fn allows_proactive_agents(self) -> bool {
        self == Self::Ultra
    }
}

fn openai_supports_xhigh(model: &str) -> bool {
    ["gpt-5.2", "gpt-5.4", "gpt-5.5", "gpt-5.6"]
        .iter()
        .any(|prefix| model.starts_with(prefix))
}

fn anthropic_supports_effort(model: &str) -> bool {
    [
        "claude-fable-5",
        "claude-mythos-5",
        "claude-mythos-preview",
        "claude-opus-4-5",
        "claude-opus-4-6",
        "claude-opus-4-7",
        "claude-opus-4-8",
        "claude-sonnet-4-6",
        "claude-sonnet-5",
    ]
    .iter()
    .any(|family| model.starts_with(family))
}

fn bedrock_supports_effort(model: &str) -> bool {
    [
        "anthropic.claude-mythos-5",
        "anthropic.claude-fable-5",
        "anthropic.claude-opus-4-7",
        "anthropic.claude-mythos-preview",
        "anthropic.claude-opus-4-6",
        "anthropic.claude-sonnet-4-6",
        "amazon.nova-2",
        "amazon.nova-lite-1-5",
        "amazon.nova-pro-1-5",
    ]
    .iter()
    .any(|family| model.contains(family))
}

/// How freely the agent may act.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Read-only. Mutating tools are withheld entirely (not even presented to the
    /// model), so the agent can explore and plan but cannot change anything.
    Plan,
    /// Default. Mutating tools prompt for confirmation, honouring allow/deny rules.
    #[default]
    Agentic,
    /// No prompts. Every action is auto-allowed except explicit `deny` rules.
    Yolo,
}

impl Mode {
    /// Parse a user-typed mode name, tolerant of common synonyms.
    pub fn parse(s: &str) -> Option<Mode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "plan" | "planning" | "read-only" | "readonly" | "ro" => Some(Mode::Plan),
            "agentic" | "agent" | "default" | "normal" | "ask" => Some(Mode::Agentic),
            "yolo" | "brave" | "auto" | "unsafe" => Some(Mode::Yolo),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Plan => "plan",
            Mode::Agentic => "agentic",
            Mode::Yolo => "yolo",
        }
    }

    /// Next mode for the shift+tab affordance: plan → agentic → yolo → plan.
    pub fn cycle(self) -> Mode {
        match self {
            Mode::Plan => Mode::Agentic,
            Mode::Agentic => Mode::Yolo,
            Mode::Yolo => Mode::Plan,
        }
    }

    pub fn describe(&self) -> &'static str {
        match self {
            Mode::Plan => "read-only — mutating tools are withheld; explore and plan only",
            Mode::Agentic => "mutating tools prompt for confirmation (allow/deny rules honoured)",
            Mode::Yolo => "no prompts — every action auto-allowed except explicit deny rules",
        }
    }
}

/// Allow/deny rule lists. Each rule is either a bare tool name (`"edit_file"`) or a
/// tool name with a glob over the action detail (`"execute_command(cargo *)"`,
/// `"delete_file(*.lock)"`). `deny` always beats `allow`. `*` as a tool name
/// matches any tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Permissions {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Optional model overrides stored locally.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelPrefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<String>,
    /// Model used for summarisation passes (chunks, architecture, directories,
    /// relationships, `/compact`). Defaults to the chat model's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Model web-research agents run on (`/wmodel`). Defaults to the chat model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<String>,
}

/// The on-disk `.ikode/settings.local.json` document.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalSettings {
    #[serde(default)]
    pub mode: Mode,
    /// Session reasoning/delegation level. Missing in older files -> `auto`.
    #[serde(default)]
    pub effort: Effort,
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<ModelPrefs>,
    /// Auto-compact once a request's prompt reaches this share (percent) of the chat
    /// model's context window, as reported by the bundled GAISe model registry or the
    /// provider. Unset → [`DEFAULT_AUTO_COMPACT_PERCENT`]; `0` disables
    /// auto-compaction. Ignored when `auto_compact_tokens` is set explicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_compact_percent: Option<u8>,
    /// Absolute override: auto-compact once a request's prompt reaches this many
    /// tokens regardless of the model's window. Unset → use `auto_compact_percent`
    /// (falling back to [`DEFAULT_AUTO_COMPACT_TOKENS`] when the window is unknown);
    /// `0` disables auto-compaction entirely (manual `/compact` only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_compact_tokens: Option<usize>,
    /// Explicitly registered third-party MCP servers. This file is machine-local
    /// and git-ignored so commands, endpoint headers, and environment references
    /// are never committed as project configuration.
    #[serde(
        default,
        alias = "mcpServers",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub mcp_servers: BTreeMap<String, crate::mcp::McpServerConfig>,
}

impl LocalSettings {
    /// The effective auto-compact percentage (`0` = disabled by percent).
    pub fn auto_compact_percent(&self) -> u8 {
        self.auto_compact_percent
            .unwrap_or(DEFAULT_AUTO_COMPACT_PERCENT)
    }

    /// Resolve the auto-compact threshold for a model whose context window is
    /// `context_window` (in tokens; `None` = unknown).
    ///
    /// Precedence: an explicit `auto_compact_tokens` is an absolute override (so
    /// older settings files keep their exact behaviour); otherwise
    /// `auto_compact_percent` of the window; otherwise, with no window to reason
    /// about, [`DEFAULT_AUTO_COMPACT_TOKENS`]. A `0` in either field disables.
    pub fn auto_compact_threshold(&self, context_window: Option<u64>) -> AutoCompactThreshold {
        if let Some(tokens) = self.auto_compact_tokens {
            return AutoCompactThreshold {
                tokens,
                rule: if tokens == 0 {
                    AutoCompactRule::Disabled
                } else {
                    AutoCompactRule::Tokens
                },
            };
        }
        let percent = self.auto_compact_percent();
        if percent == 0 {
            return AutoCompactThreshold {
                tokens: 0,
                rule: AutoCompactRule::Disabled,
            };
        }
        match context_window {
            Some(window) if window > 0 => AutoCompactThreshold {
                // u128 so a (pathological) huge window can't overflow the multiply.
                tokens: (u128::from(window) * u128::from(percent) / 100) as usize,
                rule: AutoCompactRule::Percent { percent, window },
            },
            _ => AutoCompactThreshold {
                tokens: DEFAULT_AUTO_COMPACT_TOKENS,
                rule: AutoCompactRule::Fallback,
            },
        }
    }
}

/// The outcome of consulting the permission engine for one tool invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Run without prompting.
    Allow,
    /// Refuse outright (deny rule, or a mutating tool in plan mode).
    Deny,
    /// Prompt the user interactively.
    Ask,
}

/// Project-relative path to the local settings file.
pub fn settings_path(root: &Path) -> PathBuf {
    root.join(".ikode").join("settings.local.json")
}

impl LocalSettings {
    /// Load from `<root>/.ikode/settings.local.json`. A missing file yields
    /// defaults silently; a malformed file yields defaults with a warning (we never
    /// fail startup over local settings).
    pub fn load(root: &Path) -> LocalSettings {
        let path = settings_path(root);
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("ikode: ignoring malformed {}: {e}", path.display());
                    LocalSettings::default()
                }
            },
            Err(_) => LocalSettings::default(),
        }
    }

    /// Persist to `<root>/.ikode/settings.local.json` (pretty JSON), creating
    /// `.ikode/` if needed.
    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let path = settings_path(root);
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::util::atomic_write(&path, &json)
    }

    /// Decide whether `tool` (with action `detail`, e.g. the command string or file
    /// path) may run. Precedence: explicit `deny` > non-mutating auto-allow > mode.
    pub fn decide(&self, tool: &str, detail: &str) -> Decision {
        if self
            .permissions
            .deny
            .iter()
            .any(|r| rule_matches(r, tool, detail))
        {
            return Decision::Deny;
        }
        // Network tools: read-only, so plan mode does not withhold them, but
        // every call leaves the machine — prompt unless an allow rule (or yolo)
        // covers it.
        if is_network(tool) {
            return match self.mode {
                Mode::Yolo => Decision::Allow,
                Mode::Plan | Mode::Agentic => {
                    if self
                        .permissions
                        .allow
                        .iter()
                        .any(|r| rule_matches(r, tool, detail))
                    {
                        Decision::Allow
                    } else {
                        Decision::Ask
                    }
                }
            };
        }
        // Read-only tools are never gated (deny rules above can still block them).
        if !is_mutating(tool) {
            return Decision::Allow;
        }
        match self.mode {
            Mode::Plan => Decision::Deny,
            Mode::Yolo => Decision::Allow,
            Mode::Agentic => {
                if self
                    .permissions
                    .allow
                    .iter()
                    .any(|r| rule_matches(r, tool, detail))
                {
                    // A broad prefix rule such as `execute_command(cargo *)` must not
                    // silently authorize a second shell segment appended with `&&`,
                    // `;`, a pipe, or redirection. Compound commands remain available,
                    // but require an explicit per-invocation confirmation.
                    if tool == "execute_command" && command_has_shell_control(detail) {
                        Decision::Ask
                    } else {
                        Decision::Allow
                    }
                } else {
                    Decision::Ask
                }
            }
        }
    }
}

/// Whether a shell command contains an unquoted control operator. This is not a
/// shell parser; it is deliberately conservative and exists only to prevent broad
/// allow-rules from granting additional command segments or redirections.
pub fn command_has_shell_control(command: &str) -> bool {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let chars: Vec<char> = command.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            i += 1;
            continue;
        }
        match quote {
            Some(active) if ch == active => quote = None,
            Some('\'') => {}
            Some('"') => {
                if ch == '`' || (ch == '$' && chars.get(i + 1) == Some(&'(')) {
                    return true;
                }
            }
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if matches!(ch, '&' | '|' | ';' | '<' | '>' | '`' | '\n' | '\r') => {
                return true;
            }
            None if ch == '$' && chars.get(i + 1) == Some(&'(') => return true,
            None => {}
            Some(_) => unreachable!(),
        }
        i += 1;
    }
    false
}

/// Does `rule` (`"tool"` or `"tool(glob)"`, or `"*"`/`"*(glob)"`) match this
/// invocation? The glob, if present, is matched against `detail` with `*` as the
/// only wildcard.
pub fn rule_matches(rule: &str, tool: &str, detail: &str) -> bool {
    let rule = rule.trim();
    let (rule_tool, pattern) = match rule.split_once('(') {
        Some((t, rest)) => {
            let pat = rest.strip_suffix(')').unwrap_or(rest);
            (t.trim(), Some(pat.trim()))
        }
        None => (rule, None),
    };
    if rule_tool != "*" && rule_tool != tool {
        return false;
    }
    match pattern {
        None => true,
        Some(pat) => glob_match(pat, detail),
    }
}

/// Validate the user-facing `tool` / `tool(glob)` permission-rule grammar.
/// Rejecting typos here prevents rules that appear to save successfully but can
/// never match a real tool invocation.
pub fn validate_rule(rule: &str) -> Result<(), String> {
    let rule = rule.trim();
    if rule.is_empty() {
        return Err("rule cannot be empty".to_string());
    }
    let tool = match rule.split_once('(') {
        Some((tool, pattern)) => {
            if !rule.ends_with(')')
                || pattern[..pattern.len().saturating_sub(1)].contains(['(', ')'])
            {
                return Err("expected tool(glob) with one closing ')'".to_string());
            }
            if pattern.len() == 1 {
                return Err("the glob inside tool(...) cannot be empty".to_string());
            }
            tool.trim()
        }
        None => {
            if rule.contains(')') {
                return Err("unexpected ')' in rule".to_string());
            }
            rule
        }
    };
    if tool != "*" && !crate::tools::is_known(tool) && !crate::mcp::is_exposed_tool_name(tool) {
        return Err(format!("unknown tool '{tool}'"));
    }
    Ok(())
}

/// Minimal glob: `*` matches any (possibly empty) run of characters; everything
/// else is literal. Case-sensitive. Anchored at both ends.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text; // no wildcard → exact match
    }
    let mut pos = 0usize;
    // First segment must be a prefix.
    let first = parts[0];
    if !text[pos..].starts_with(first) {
        return false;
    }
    pos += first.len();
    // Middle segments must appear in order.
    for seg in &parts[1..parts.len() - 1] {
        if seg.is_empty() {
            continue;
        }
        match text[pos..].find(seg) {
            Some(i) => pos += i + seg.len(),
            None => return false,
        }
    }
    // Last segment must be a suffix of the remainder.
    let last = parts[parts.len() - 1];
    text[pos..].ends_with(last) && text.len() >= pos
}
