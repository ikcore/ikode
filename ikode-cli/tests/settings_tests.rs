//! Tests for the local settings / permission engine: modes, allow/deny rules,
//! glob matching, plan-mode tool withholding, and JSON load/save round-trips.

use ikode::mcp::McpServerConfig;
use ikode::settings::{
    command_has_shell_control, glob_match, is_mutating, rule_matches, validate_rule, Decision,
    Effort, LocalSettings, Mode, ModelPrefs,
};
use ikode::tools;
use std::collections::BTreeMap;
use tempfile::tempdir;

#[test]
fn mode_parse_accepts_synonyms() {
    assert_eq!(Mode::parse("plan"), Some(Mode::Plan));
    assert_eq!(Mode::parse("planning"), Some(Mode::Plan));
    assert_eq!(Mode::parse("READONLY"), Some(Mode::Plan));
    assert_eq!(Mode::parse("agentic"), Some(Mode::Agentic));
    assert_eq!(Mode::parse("agent"), Some(Mode::Agentic));
    assert_eq!(Mode::parse("yolo"), Some(Mode::Yolo));
    assert_eq!(Mode::parse("brave"), Some(Mode::Yolo));
    assert_eq!(Mode::parse("nonsense"), None);
}

#[test]
fn default_mode_is_agentic() {
    assert_eq!(LocalSettings::default().mode, Mode::Agentic);
    assert_eq!(LocalSettings::default().effort, Effort::Auto);
}

#[test]
fn effort_parses_console_levels_and_provider_fallbacks() {
    assert_eq!(Effort::parse("low"), Some(Effort::Low));
    assert_eq!(Effort::parse("med"), Some(Effort::Medium));
    assert_eq!(Effort::parse("HIGH"), Some(Effort::High));
    assert_eq!(Effort::parse("xhigh"), Some(Effort::Max));
    assert_eq!(Effort::parse("ultra"), Some(Effort::Ultra));
    assert_eq!(Effort::parse("nonsense"), None);
    assert_eq!(
        serde_json::from_str::<Effort>("\"med\"").unwrap(),
        Effort::Medium
    );
    assert_eq!(
        serde_json::from_str::<Effort>("\"xhigh\"").unwrap(),
        Effort::Max
    );
    assert_eq!(
        Effort::Ultra.api_value("openai::gpt-5.4-mini"),
        Some("xhigh")
    );
    assert_eq!(Effort::Max.api_value("openai::gpt-5"), Some("high"));
    assert_eq!(Effort::Low.api_value("openai::gpt-5.4-pro"), Some("medium"));
    assert_eq!(Effort::High.api_value("openai::gpt-4.1"), None);
    assert_eq!(
        Effort::Ultra.api_value("anthropic::claude-sonnet-4-6"),
        Some("max")
    );
    assert_eq!(
        Effort::Max.api_value("anthropic::claude-opus-4-5-20251101"),
        Some("high")
    );
    assert_eq!(
        Effort::High.api_value("anthropic::claude-sonnet-4-5-20250929"),
        None
    );
    assert_eq!(Effort::Max.api_value("gemini::gemini-test"), Some("high"));
    assert!(Effort::Ultra.allows_proactive_agents());
}

#[test]
fn effort_aliases_labels_and_delegation_policy_cover_every_level() {
    let aliases = [
        ("auto", Effort::Auto),
        ("default", Effort::Auto),
        ("model", Effort::Auto),
        ("low", Effort::Low),
        ("lo", Effort::Low),
        ("medium", Effort::Medium),
        ("med", Effort::Medium),
        ("mid", Effort::Medium),
        ("high", Effort::High),
        ("hi", Effort::High),
        ("max", Effort::Max),
        ("xhigh", Effort::Max),
        ("extra-high", Effort::Max),
        ("extra_high", Effort::Max),
        ("ultra", Effort::Ultra),
    ];
    for (input, expected) in aliases {
        assert_eq!(Effort::parse(input), Some(expected), "{input}");
        assert_eq!(Effort::parse(&input.to_ascii_uppercase()), Some(expected));
    }

    let levels = [
        Effort::Auto,
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::Max,
        Effort::Ultra,
    ];
    for effort in levels {
        assert!(!effort.as_str().is_empty());
        assert!(!effort.describe().is_empty());
        assert_eq!(effort.allows_proactive_agents(), effort == Effort::Ultra);
    }
}

#[test]
fn effort_provider_mapping_covers_every_clamp_and_fallback() {
    let cases = [
        (Effort::Auto, "openai::gpt-5.4-mini", None),
        (Effort::Low, "openai::gpt-5.4-mini", Some("low")),
        (Effort::Medium, "openai::gpt-5.4-mini", Some("medium")),
        (Effort::High, "openai::gpt-5.4-mini", Some("high")),
        (Effort::Max, "openai::gpt-5.4-mini", Some("xhigh")),
        (Effort::Ultra, "openai::gpt-5.6", Some("xhigh")),
        (Effort::Max, "openai::gpt-5", Some("high")),
        (Effort::Low, "openai::gpt-5-pro", Some("high")),
        (Effort::Max, "openai::gpt-5-pro-2025-10-06", Some("high")),
        (Effort::Low, "openai::gpt-5.2-pro", Some("medium")),
        (Effort::Max, "openai::gpt-5.2-pro", Some("xhigh")),
        (Effort::High, "openai::gpt-4.1", None),
        (Effort::High, "openai::text-embedding-3-small", None),
        (Effort::High, "openai::my-embedding-model", None),
        (Effort::Max, "openai::future-reasoner", Some("high")),
        (Effort::Auto, "anthropic::claude-sonnet-4-6", None),
        (Effort::Low, "anthropic::claude-sonnet-4-6", Some("low")),
        (
            Effort::Medium,
            "anthropic::claude-sonnet-4-6",
            Some("medium"),
        ),
        (Effort::High, "anthropic::claude-sonnet-4-6", Some("high")),
        (Effort::Max, "anthropic::claude-sonnet-4-6", Some("max")),
        (Effort::Ultra, "anthropic::claude-opus-4-6", Some("max")),
        (Effort::Max, "anthropic::claude-opus-4-5", Some("high")),
        (Effort::High, "anthropic::claude-sonnet-4-5", None),
        (Effort::Low, "gemini::gemini-3-pro", Some("low")),
        (Effort::Medium, "gemini::gemini-3-pro", Some("medium")),
        (Effort::High, "gemini::gemini-3-pro", Some("high")),
        (Effort::Max, "gemini::gemini-3-pro", Some("high")),
        (Effort::Ultra, "vertexai::gemini-3-pro", Some("high")),
        (
            Effort::Medium,
            "bedrock::us.anthropic.claude-sonnet-4-6-v1:0",
            Some("medium"),
        ),
        (
            Effort::Max,
            "bedrock::us.amazon.nova-2-lite-v1:0",
            Some("high"),
        ),
        (Effort::High, "bedrock::amazon.titan-text-express-v1", None),
        (Effort::High, "ollama::qwen3", None),
        (Effort::Max, "unqualified-model", Some("high")),
    ];
    for (effort, model, expected) in cases {
        assert_eq!(effort.api_value(model), expected, "{effort:?} / {model}");
    }
}

#[test]
fn glob_match_handles_wildcards() {
    assert!(glob_match("cargo *", "cargo build"));
    assert!(glob_match("cargo *", "cargo "));
    assert!(!glob_match("cargo *", "npm install"));
    assert!(glob_match("*.lock", "Cargo.lock"));
    assert!(!glob_match("*.lock", "Cargo.toml"));
    assert!(glob_match("git*push", "git --no-pager push"));
    assert!(glob_match("exact", "exact"));
    assert!(!glob_match("exact", "exact "));
    assert!(glob_match("*", "anything at all"));
}

#[test]
fn rule_matches_tool_and_pattern() {
    assert!(rule_matches("edit_file", "edit_file", "src/x.rs"));
    assert!(!rule_matches("edit_file", "create_file", "src/x.rs"));
    assert!(rule_matches(
        "execute_command(cargo *)",
        "execute_command",
        "cargo test"
    ));
    assert!(!rule_matches(
        "execute_command(cargo *)",
        "execute_command",
        "rm -rf /"
    ));
    // Wildcard tool name matches any tool.
    assert!(rule_matches("*", "delete_file", "x"));
    assert!(rule_matches("*(rm *)", "execute_command", "rm -rf x"));
}

#[test]
fn permission_rules_are_validated_against_the_tool_registry() {
    assert!(validate_rule("edit_file").is_ok());
    assert!(validate_rule("execute_command(cargo *)").is_ok());
    assert!(validate_rule("*(*.secret)").is_ok());
    assert!(validate_rule("mcp__github__create_issue").is_ok());
    assert!(validate_rule("made_up_tool").is_err());
    assert!(validate_rule("execute_command(").is_err());
    assert!(validate_rule("edit_file()").is_err());
    assert!(validate_rule("").is_err());
}

#[test]
fn agentic_mode_asks_for_mutating_and_allows_reads() {
    let s = LocalSettings::default(); // agentic
    assert_eq!(s.decide("read_file", "src/x.rs"), Decision::Allow);
    assert_eq!(s.decide("search_code", "query"), Decision::Allow);
    assert_eq!(s.decide("edit_file", "src/x.rs"), Decision::Ask);
    assert_eq!(s.decide("execute_command", "cargo build"), Decision::Ask);
}

#[test]
fn allow_rule_skips_the_prompt_in_agentic_mode() {
    let mut s = LocalSettings::default();
    s.permissions
        .allow
        .push("execute_command(cargo *)".to_string());
    assert_eq!(s.decide("execute_command", "cargo test"), Decision::Allow);
    // Non-matching command still asks.
    assert_eq!(s.decide("execute_command", "rm file"), Decision::Ask);
    // Matching only the first segment is not enough to auto-authorize the rest.
    assert_eq!(
        s.decide("execute_command", "cargo test && rm file"),
        Decision::Ask
    );
}

#[test]
fn shell_control_detection_ignores_quoted_literals_but_catches_execution() {
    assert!(!command_has_shell_control("cargo test"));
    assert!(!command_has_shell_control("echo 'a && b'"));
    assert!(!command_has_shell_control("echo \"a | b\""));
    for command in [
        "cargo test && rm file",
        "cargo test; rm file",
        "cargo test | tee log",
        "cargo test > log",
        "echo $(danger)",
        "echo `danger`",
    ] {
        assert!(command_has_shell_control(command), "{command}");
    }
}

#[test]
fn deny_rule_beats_everything_including_yolo() {
    let mut s = LocalSettings {
        mode: Mode::Yolo,
        ..Default::default()
    };
    s.permissions.deny.push("execute_command(rm *)".to_string());
    // Yolo allows in general...
    assert_eq!(s.decide("execute_command", "cargo build"), Decision::Allow);
    // ...but the deny rule wins.
    assert_eq!(s.decide("execute_command", "rm -rf /"), Decision::Deny);
    // Deny can even block a read tool.
    s.permissions.deny.push("read_file(*.secret)".to_string());
    assert_eq!(s.decide("read_file", "id_rsa.secret"), Decision::Deny);
    assert_eq!(s.decide("read_file", "main.rs"), Decision::Allow);
}

#[test]
fn yolo_mode_allows_mutating_without_prompt() {
    let s = LocalSettings {
        mode: Mode::Yolo,
        ..Default::default()
    };
    assert_eq!(s.decide("delete_file", "x.rs"), Decision::Allow);
    assert_eq!(s.decide("execute_command", "anything"), Decision::Allow);
}

#[test]
fn plan_mode_denies_all_mutating_tools() {
    let s = LocalSettings {
        mode: Mode::Plan,
        ..Default::default()
    };
    for t in [
        "execute_command",
        "edit_file",
        "edit_chunk",
        "create_file",
        "delete_file",
    ] {
        assert_eq!(
            s.decide(t, "x"),
            Decision::Deny,
            "{t} must be denied in plan mode"
        );
    }
    // Read tools still work for planning/exploration.
    assert_eq!(s.decide("read_file", "x.rs"), Decision::Allow);
    assert_eq!(s.decide("ask_codebase", "how does X work"), Decision::Allow);
}

#[test]
fn is_mutating_classification() {
    assert!(is_mutating("execute_command"));
    assert!(is_mutating("delete_file"));
    assert!(!is_mutating("read_file"));
    assert!(!is_mutating("ask_codebase"));
    assert!(is_mutating("mcp__github__create_issue"));
}

#[test]
fn plan_mode_withholds_mutating_tools_from_the_tool_list() {
    let agentic = tools::get_tools(Mode::Agentic, false);
    let plan = tools::get_tools(Mode::Plan, false);
    let names = |v: &[gaise_core::contracts::GaiseTool]| {
        v.iter().map(|t| t.name.clone()).collect::<Vec<_>>()
    };
    let agentic_names = names(&agentic);
    let plan_names = names(&plan);

    // Mutating tools are present in agentic mode...
    for t in [
        "execute_command",
        "edit_file",
        "edit_chunk",
        "create_file",
        "delete_file",
    ] {
        assert!(
            agentic_names.contains(&t.to_string()),
            "{t} missing in agentic"
        );
        assert!(
            !plan_names.contains(&t.to_string()),
            "{t} must be withheld in plan mode"
        );
    }
    // ...and read/search tools survive in both.
    for t in ["read_file", "search_code", "ask_codebase", "graph_overview"] {
        assert!(
            plan_names.contains(&t.to_string()),
            "{t} should remain in plan mode"
        );
    }
    assert!(plan.len() < agentic.len());
}

#[test]
fn graph_off_withholds_every_graph_backed_tool() {
    let tools = tools::get_tools_for(Mode::Agentic, false, false);
    let names = tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<Vec<_>>();
    for graph_tool in [
        "index_codebase",
        "search_code",
        "outline_file",
        "ask_codebase",
        "find_references",
        "graph_overview",
        "graph_query",
        "edit_chunk",
    ] {
        assert!(
            !names.contains(&graph_tool),
            "{graph_tool} leaked while graph mode was off"
        );
    }
    for traditional_tool in [
        "read_file",
        "list_directory",
        "edit_file",
        "execute_command",
    ] {
        assert!(
            names.contains(&traditional_tool),
            "{traditional_tool} should remain available"
        );
    }
}

#[test]
fn settings_round_trip_through_disk() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Missing file -> defaults.
    assert_eq!(LocalSettings::load(root).mode, Mode::Agentic);

    let mut s = LocalSettings {
        mode: Mode::Plan,
        effort: Effort::Ultra,
        ..Default::default()
    };
    s.permissions
        .allow
        .push("execute_command(cargo *)".to_string());
    s.permissions.deny.push("delete_file".to_string());
    s.models = Some(ModelPrefs {
        chat: Some("openai::gpt-5.4-mini".to_string()),
        embedding: None,
        summary: None,
        web: None,
    });
    s.mcp_servers.insert(
        "local".to_string(),
        McpServerConfig::Stdio {
            command: "npx".to_string(),
            args: vec!["server".to_string()],
            env: BTreeMap::new(),
            enabled: false,
        },
    );
    s.save(root).unwrap();

    assert!(root.join(".ikode").join("settings.local.json").exists());

    let loaded = LocalSettings::load(root);
    assert_eq!(loaded.mode, Mode::Plan);
    assert_eq!(loaded.effort, Effort::Ultra);
    assert_eq!(
        loaded.permissions.allow,
        vec!["execute_command(cargo *)".to_string()]
    );
    assert_eq!(loaded.permissions.deny, vec!["delete_file".to_string()]);
    assert_eq!(loaded.models.unwrap().chat.unwrap(), "openai::gpt-5.4-mini");
    assert!(matches!(
        loaded.mcp_servers.get("local"),
        Some(McpServerConfig::Stdio { enabled: false, .. })
    ));
}

#[test]
fn malformed_settings_file_falls_back_to_defaults() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".ikode")).unwrap();
    std::fs::write(
        root.join(".ikode").join("settings.local.json"),
        "{ not json",
    )
    .unwrap();
    // Must not panic; yields defaults.
    assert_eq!(LocalSettings::load(root).mode, Mode::Agentic);
}

#[test]
fn web_research_is_network_gated_not_mutating() {
    // Read-only for the checkout, so plan mode keeps it, but every call leaves
    // the machine: prompt in plan/agentic, allow in yolo, rules always win.
    assert!(!is_mutating("web_research"));
    let mut s = LocalSettings {
        mode: Mode::Agentic,
        ..Default::default()
    };
    assert_eq!(s.decide("web_research", "rust axum docs"), Decision::Ask);
    s.mode = Mode::Plan;
    assert_eq!(s.decide("web_research", "rust axum docs"), Decision::Ask);
    s.mode = Mode::Yolo;
    assert_eq!(s.decide("web_research", "rust axum docs"), Decision::Allow);

    s.mode = Mode::Agentic;
    assert!(validate_rule("web_research(*)").is_ok());
    s.permissions.allow.push("web_research(*)".to_string());
    assert_eq!(s.decide("web_research", "anything"), Decision::Allow);
    s.permissions.deny.push("web_research".to_string());
    assert_eq!(s.decide("web_research", "anything"), Decision::Deny);
    s.mode = Mode::Yolo;
    assert_eq!(s.decide("web_research", "anything"), Decision::Deny);
}

#[test]
fn auto_compact_threshold_prefers_tokens_then_percent_then_fallback() {
    use ikode::settings::{
        AutoCompactRule, DEFAULT_AUTO_COMPACT_PERCENT, DEFAULT_AUTO_COMPACT_TOKENS,
    };

    // Defaults: 80% of a known window.
    let s = LocalSettings::default();
    assert_eq!(s.auto_compact_percent(), DEFAULT_AUTO_COMPACT_PERCENT);
    let t = s.auto_compact_threshold(Some(400_000));
    assert_eq!(t.tokens, 320_000);
    assert_eq!(
        t.rule,
        AutoCompactRule::Percent {
            percent: 80,
            window: 400_000
        }
    );

    // Unknown window: absolute fallback.
    let t = s.auto_compact_threshold(None);
    assert_eq!(t.tokens, DEFAULT_AUTO_COMPACT_TOKENS);
    assert_eq!(t.rule, AutoCompactRule::Fallback);
    assert_eq!(s.auto_compact_threshold(Some(0)).rule, AutoCompactRule::Fallback);

    // A user percentage.
    let s = LocalSettings {
        auto_compact_percent: Some(50),
        ..Default::default()
    };
    assert_eq!(s.auto_compact_threshold(Some(1_000_000)).tokens, 500_000);

    // An explicit token override beats the percentage, known window or not.
    let s = LocalSettings {
        auto_compact_percent: Some(50),
        auto_compact_tokens: Some(123_456),
        ..Default::default()
    };
    let t = s.auto_compact_threshold(Some(1_000_000));
    assert_eq!((t.tokens, t.rule), (123_456, AutoCompactRule::Tokens));
    assert_eq!(s.auto_compact_threshold(None).tokens, 123_456);

    // Zero in either field disables.
    let s = LocalSettings {
        auto_compact_tokens: Some(0),
        ..Default::default()
    };
    let t = s.auto_compact_threshold(Some(1_000_000));
    assert_eq!((t.tokens, t.rule), (0, AutoCompactRule::Disabled));
    let s = LocalSettings {
        auto_compact_percent: Some(0),
        ..Default::default()
    };
    let t = s.auto_compact_threshold(Some(1_000_000));
    assert_eq!((t.tokens, t.rule), (0, AutoCompactRule::Disabled));
}

#[test]
fn auto_compact_percent_round_trips_through_settings_json() {
    let s = LocalSettings {
        auto_compact_percent: Some(65),
        ..Default::default()
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"auto_compact_percent\":65"));
    assert!(!json.contains("auto_compact_tokens"), "unset fields stay out of the file");
    let back: LocalSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(back.auto_compact_percent, Some(65));
    // Older files without the field load with the default.
    let old: LocalSettings = serde_json::from_str(r#"{"auto_compact_tokens": 128000}"#).unwrap();
    assert_eq!(old.auto_compact_percent, None);
    assert_eq!(old.auto_compact_threshold(Some(400_000)).tokens, 128_000);
}
