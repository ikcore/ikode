//! Integration tests for the project-model helpers: root discovery, config
//! loading/precedence, `ikode init` scaffolding, and graph-overview rendering.

use ikode::harness::{
    ensure_gitignore, find_project_root, render_graph_stats, run_init, save_config_typed,
    save_config_value, ConfigScope, ProjectConfig, GITIGNORE_MARKER, INIT_CONFIG_TOML,
};
use ikode::index::GraphStats;
use livec_graph::WalStats;
use std::fs;
use tempfile::tempdir;

fn write_config(root: &std::path::Path, body: &str) {
    let ikode = root.join(".ikode");
    fs::create_dir_all(&ikode).unwrap();
    fs::write(ikode.join("config.toml"), body).unwrap();
}

#[test]
fn project_root_prefers_ikode_then_git_then_cwd() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::create_dir_all(root.join("sub").join("deep")).unwrap();
    let start = root.join("sub").join("deep");

    // Only .git exists -> falls back to the .git root.
    assert_eq!(find_project_root(&start), root.to_path_buf());

    // .ikode takes precedence and is found from a nested cwd.
    fs::create_dir_all(root.join(".ikode")).unwrap();
    assert_eq!(find_project_root(&start), root.to_path_buf());
}

#[test]
fn project_config_loads_and_parses_all_fields() {
    let dir = tempdir().unwrap();
    write_config(
        dir.path(),
        "model = \"openai::gpt-4o\"\nbrave = true\nmax_history = 10\nprefix_keep = 2\nauto_index = true\ngraph_enabled = false\nsession_keep = 12\nsession_max_bytes = 1048576\nsummarize_tests = true\nsummarize_min_lines = 6\nsummarize_concurrency = 8\nagent_max_threads = 3\nagent_max_turns = 9\n",
    );
    let cfg = ProjectConfig::load(dir.path());
    assert_eq!(cfg.model.as_deref(), Some("openai::gpt-4o"));
    assert_eq!(cfg.brave, Some(true));
    assert_eq!(cfg.max_history, Some(10));
    assert_eq!(cfg.prefix_keep, Some(2));
    assert_eq!(cfg.auto_index, Some(true));
    assert_eq!(cfg.graph_enabled, Some(false));
    assert_eq!(cfg.session_keep, Some(12));
    assert_eq!(cfg.session_max_bytes, Some(1_048_576));
    assert_eq!(cfg.summarize_tests, Some(true));
    assert_eq!(cfg.summarize_min_lines, Some(6));
    assert_eq!(cfg.summarize_concurrency, Some(8));
    assert_eq!(cfg.agent_max_threads, Some(3));
    assert_eq!(cfg.agent_max_turns, Some(9));
}

#[test]
fn project_config_missing_file_is_default() {
    let dir = tempdir().unwrap();
    let cfg = ProjectConfig::load(dir.path());
    assert!(cfg.model.is_none());
    assert!(cfg.brave.is_none());
    assert!(cfg.auto_index.is_none());
    assert!(cfg.graph_enabled.is_none());
}

#[test]
fn init_config_template_is_all_commented_defaults() {
    let dir = tempdir().unwrap();
    write_config(dir.path(), INIT_CONFIG_TOML);
    let cfg = ProjectConfig::load(dir.path());
    assert!(cfg.model.is_none());
    assert!(cfg.brave.is_none());
    assert!(cfg.auto_index.is_none());
    assert!(cfg.graph_enabled.is_none());
}

#[test]
fn project_config_parses_embedding_model() {
    let dir = tempdir().unwrap();
    write_config(
        dir.path(),
        "model = \"openai::gpt-4o\"\nembedding_model = \"ollama::nomic-embed-text\"\n",
    );
    let cfg = ProjectConfig::load(dir.path());
    assert_eq!(cfg.model.as_deref(), Some("openai::gpt-4o"));
    assert_eq!(
        cfg.embedding_model.as_deref(),
        Some("ollama::nomic-embed-text")
    );
}

#[test]
fn save_config_value_writes_and_is_reloadable() {
    let dir = tempdir().unwrap();
    // Saving into a project with no config file yet creates it.
    let path =
        save_config_value(ConfigScope::Project, dir.path(), "model", "openai::gpt-4o").unwrap();
    assert!(path.exists());
    let cfg = ProjectConfig::load(dir.path());
    assert_eq!(cfg.model.as_deref(), Some("openai::gpt-4o"));

    // A second key is added without clobbering the first.
    save_config_value(
        ConfigScope::Project,
        dir.path(),
        "embedding_model",
        "ollama::nomic-embed-text",
    )
    .unwrap();
    let cfg = ProjectConfig::load(dir.path());
    assert_eq!(cfg.model.as_deref(), Some("openai::gpt-4o"));
    assert_eq!(
        cfg.embedding_model.as_deref(),
        Some("ollama::nomic-embed-text")
    );
}

#[test]
fn save_config_value_preserves_comments_and_overwrites_key() {
    let dir = tempdir().unwrap();
    write_config(
        dir.path(),
        "# a helpful comment\nmodel = \"openai::gpt-4o\"\n",
    );
    save_config_value(
        ConfigScope::Project,
        dir.path(),
        "model",
        "anthropic::claude-opus-4-8",
    )
    .unwrap();
    let raw = fs::read_to_string(dir.path().join(".ikode").join("config.toml")).unwrap();
    assert!(
        raw.contains("# a helpful comment"),
        "comment must be preserved"
    );
    let cfg = ProjectConfig::load(dir.path());
    assert_eq!(cfg.model.as_deref(), Some("anthropic::claude-opus-4-8"));
}

#[test]
fn save_config_typed_writes_bool_and_int_and_reloads() {
    let dir = tempdir().unwrap();
    // The `update_settings` tool writes the enrichment knobs as their native TOML
    // types (bool / integer), not quoted strings.
    save_config_typed(ConfigScope::Project, dir.path(), "summarize_tests", true).unwrap();
    save_config_typed(
        ConfigScope::Project,
        dir.path(),
        "summarize_concurrency",
        8i64,
    )
    .unwrap();

    let raw = fs::read_to_string(dir.path().join(".ikode").join("config.toml")).unwrap();
    assert!(
        raw.contains("summarize_tests = true"),
        "bool must be unquoted: {raw}"
    );
    assert!(
        raw.contains("summarize_concurrency = 8"),
        "int must be unquoted: {raw}"
    );

    let cfg = ProjectConfig::load(dir.path());
    assert_eq!(cfg.summarize_tests, Some(true));
    assert_eq!(cfg.summarize_concurrency, Some(8));
}

#[test]
fn ensure_gitignore_appends_and_is_idempotent() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "node_modules/\n").unwrap();

    ensure_gitignore(dir.path()).unwrap();
    let c1 = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(c1.contains("node_modules/")); // preserved
    assert!(c1.contains(".ikode/graph.log"));
    assert_eq!(c1.matches(GITIGNORE_MARKER).count(), 1);

    ensure_gitignore(dir.path()).unwrap();
    let c2 = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert_eq!(c2.matches(GITIGNORE_MARKER).count(), 1);
}

#[test]
fn run_init_scaffolds_expected_files() {
    let dir = tempdir().unwrap();
    run_init(dir.path()).unwrap();
    assert!(dir.path().join(".ikode").join("ikode.md").exists());
    assert!(dir.path().join(".ikode").join("config.toml").exists());
    assert!(dir.path().join(".gitignore").exists());
    // running again must not error or duplicate gitignore entries
    run_init(dir.path()).unwrap();
    let gi = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert_eq!(gi.matches(GITIGNORE_MARKER).count(), 1);
}

#[test]
fn render_graph_stats_includes_totals_and_labels() {
    let stats = GraphStats {
        nodes_total: 3,
        edges_total: 2,
        node_labels: vec![("Function".to_string(), 2), ("File".to_string(), 1)],
        edge_labels: vec![("DEFINES".to_string(), 2)],
        wal: Some(WalStats {
            file_bytes: 1536,
            bytes_since_checkpoint: 512,
            last_sequence: 7,
            original_payload_bytes: 4096,
            stored_payload_bytes: 1024,
            ..WalStats::default()
        }),
    };
    let s = render_graph_stats(&stats);
    assert!(s.contains("3 nodes"));
    assert!(s.contains("2 edges"));
    assert!(s.contains("File"));
    assert!(s.contains("Function"));
    assert!(s.contains("DEFINES"));
    assert!(s.contains("1.5 KiB"));
    assert!(s.contains("512 B delta"));
    assert!(s.contains("sequence 7"));
    assert!(s.contains("3.0 KiB compressed away"));
}

#[test]
fn list_dir_returns_subdirs_first_then_files_each_sorted() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::create_dir(dir.path().join("docs")).unwrap();
    fs::write(dir.path().join("z.txt"), "z").unwrap();
    fs::write(dir.path().join("a.txt"), "a").unwrap();

    let entries = ikode::harness::list_dir(dir.path()).unwrap();
    // Directories (with trailing /) first and sorted, then files sorted.
    assert_eq!(entries, vec!["docs/", "src/", "a.txt", "z.txt"]);
}
