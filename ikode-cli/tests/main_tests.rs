//! Tests for the CLI helper functions in [`ikode::util`] — token/usage handling,
//! the working-directory path guard, the per-turn mode line, and the command /
//! tool-argument normalisers. These live in the library so this integration test
//! can exercise them directly, mirroring the other suites under `tests/`.

use std::collections::HashMap;

use ikode::settings::{Effort, Mode};
use ikode::util::{
    atomic_write, atomic_write_with, attach_images_to_last_user, effort_status_line, human_bytes,
    is_sendable_message, mask_secret, mode_status_line, normalize_command, parse_byte_size,
    resolve_within, sanitize_tool_arguments, usage_totals,
};

use gaise_core::contracts::{GaiseContent, GaiseMessage, GaiseToolCall, GaiseUsage, OneOrMany};

#[test]
fn mask_secret_shows_only_the_last_four_chars() {
    assert_eq!(mask_secret("sk-abc123456789wxyz"), "***wxyz");
    assert_eq!(mask_secret("  padded-key-1234  "), "***1234");
    // Short values mask entirely — the tail would be the whole secret.
    assert_eq!(mask_secret("abcd"), "***");
    assert_eq!(mask_secret("x"), "***");
    assert_eq!(mask_secret(""), "***");
    // Multi-byte characters count as characters, not bytes.
    assert_eq!(mask_secret("clé-secrète-économie"), "***omie");
}

#[test]
fn mode_status_line_reflects_active_mode() {
    assert!(mode_status_line(Mode::Plan).contains("PLAN"));
    assert!(mode_status_line(Mode::Plan).contains("read-only"));
    // Agentic must positively grant write capability and forbid the stale refusal.
    let agentic = mode_status_line(Mode::Agentic);
    assert!(agentic.contains("AGENTIC"));
    assert!(agentic.contains("ARE available"));
    assert!(agentic.contains("Do NOT tell the user to switch modes"));
    assert!(mode_status_line(Mode::Yolo).contains("YOLO"));
}

#[test]
fn effort_status_line_distinguishes_depth_from_delegation() {
    assert!(effort_status_line(Effort::Low).contains("LOW"));
    assert!(effort_status_line(Effort::Max).contains("deepest"));
    let ultra = effort_status_line(Effort::Ultra);
    assert!(ultra.contains("ULTRA"));
    assert!(ultra.contains("proactively delegate"));
}

#[test]
fn atomic_write_replaces_contents_without_leaving_a_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    atomic_write(&path, b"first").unwrap();
    atomic_write(&path, b"second").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"second");
    let names = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["state.json"]);
}

#[test]
fn failed_atomic_write_preserves_the_previous_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    atomic_write(&path, b"stable").unwrap();
    let error = atomic_write_with(&path, |file| {
        use std::io::Write;
        file.write_all(b"partial")?;
        Err(std::io::Error::other("simulated failure"))
    })
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Other);
    assert_eq!(std::fs::read(&path).unwrap(), b"stable");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn byte_sizes_parse_and_render_for_storage_commands() {
    assert_eq!(parse_byte_size("512").unwrap(), 512);
    assert_eq!(parse_byte_size("64 KiB").unwrap(), 65_536);
    assert_eq!(parse_byte_size("2mb").unwrap(), 2 * 1024 * 1024);
    assert!(parse_byte_size("12 parsecs").is_err());
    assert_eq!(human_bytes(1536), "1.5 KiB");
}

#[test]
fn usage_totals_selects_provider_aggregates_without_double_counting_details() {
    let openai = GaiseUsage {
        input: Some(HashMap::from([
            ("prompt_tokens".to_string(), 100),
            ("audio_tokens".to_string(), 20),
            ("cached_tokens".to_string(), 40),
        ])),
        output: Some(HashMap::from([
            ("completion_tokens".to_string(), 50),
            ("audio_tokens".to_string(), 5),
            ("reasoning_tokens".to_string(), 30),
        ])),
        total: Some(HashMap::from([("total_tokens".to_string(), 150)])),
    };
    assert_eq!(usage_totals(&openai), (100, 50, 40));

    let google = GaiseUsage {
        input: Some(HashMap::from([
            ("prompt_tokens".to_string(), 100),
            ("text_tokens".to_string(), 40),
            ("image_tokens".to_string(), 25),
            ("audio_tokens".to_string(), 35),
            ("cached_tokens".to_string(), 20),
            ("cached_text_tokens".to_string(), 20),
            ("tool_prompt_tokens".to_string(), 5),
        ])),
        output: Some(HashMap::from([
            ("candidates_tokens".to_string(), 80),
            ("text_tokens".to_string(), 30),
            ("image_tokens".to_string(), 20),
            ("audio_tokens".to_string(), 30),
            ("reasoning_tokens".to_string(), 12),
        ])),
        total: Some(HashMap::from([("total_tokens".to_string(), 180)])),
    };
    assert_eq!(usage_totals(&google), (100, 80, 20));
}

#[test]
fn usage_totals_prefers_effective_input_and_counts_only_cache_reads() {
    let anthropic = GaiseUsage {
        input: Some(HashMap::from([
            ("input_tokens".to_string(), 20),
            ("effective_input_tokens".to_string(), 38),
            ("cache_read_input_tokens".to_string(), 11),
            ("cache_creation_input_tokens".to_string(), 7),
            ("cache_creation_1h_input_tokens".to_string(), 2),
            ("cache_creation_5m_input_tokens".to_string(), 5),
        ])),
        output: Some(HashMap::from([
            ("output_tokens".to_string(), 30),
            ("reasoning_tokens".to_string(), 9),
        ])),
        total: None,
    };
    assert_eq!(usage_totals(&anthropic), (38, 30, 11));
}

#[test]
fn normalize_command_maps_underscore_to_kebab_ask_codebase() {
    // Underscore spelling (tool name) -> kebab-case REPL command.
    assert_eq!(
        normalize_command("/ask_codebase what writes the WAL").as_deref(),
        Some("/ask-codebase what writes the WAL")
    );
    assert_eq!(
        normalize_command("/ask_codebase").as_deref(),
        Some("/ask-codebase")
    );
    // The kebab form is already canonical: not rewritten.
    assert_eq!(normalize_command("/ask-codebase foo"), None);
    // `/ask` is a distinct command, never rewritten.
    assert_eq!(normalize_command("/ask hello"), None);
    assert_eq!(normalize_command("/index"), None);
    // A longer command that merely starts with the alias text is not rewritten.
    assert_eq!(normalize_command("/ask_codebasex foo"), None);
}

#[test]
fn resolve_within_handles_relative_paths_and_blocks_escape() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    std::fs::create_dir_all(base.join("ikode-cli/src")).unwrap();
    std::fs::write(base.join("ikode-cli/src/main.rs"), "fn main() {}").unwrap();

    // An existing file via a plain relative path — the case that wrongly failed on
    // Windows because of the `\\?\` canonicalisation mismatch.
    let r = resolve_within(base, "ikode-cli/src/main.rs").unwrap();
    assert!(r.ends_with("main.rs"));
    // A `./`-prefixed relative path resolves the same way.
    assert!(resolve_within(base, "./ikode-cli/src/main.rs").is_ok());
    // A file that doesn't exist yet (e.g. create_file target) is still allowed.
    assert!(resolve_within(base, "ikode-cli/src/new_module.rs").is_ok());
    // `..` traversal out of the working directory is rejected.
    assert!(resolve_within(base, "../escape.rs").is_err());
    // The same guarantee holds when several leading directories do not exist. The
    // old one-parent fallback returned this with unchecked `..` components.
    assert!(resolve_within(base, "missing/deeper/../../../escape.rs").is_err());
    // Benign traversal that normalises back inside the project remains valid.
    let nested = resolve_within(base, "missing/deeper/../../inside.rs").unwrap();
    assert!(nested.starts_with(base.canonicalize().unwrap()));
    assert!(nested.ends_with("inside.rs"));
}

fn msg(content: Option<&str>, tool_calls: Option<Vec<GaiseToolCall>>) -> GaiseMessage {
    GaiseMessage {
        role: "assistant".to_string(),
        content: content.map(|t| {
            OneOrMany::One(GaiseContent::Text {
                text: t.to_string(),
            })
        }),
        tool_calls,
        tool_call_id: None,
        tool_name: None,
    }
}

fn img(byte: u8) -> GaiseContent {
    GaiseContent::Image {
        data: vec![byte],
        format: Some("image/png".to_string()),
    }
}

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

#[test]
fn images_splice_onto_the_last_user_message_after_its_text() {
    // History: an earlier user turn, an assistant reply, then the current user turn.
    let mut msgs = vec![user("first"), msg(Some("ok"), None), user("look at this")];
    attach_images_to_last_user(&mut msgs, &[img(1), img(2)]);

    // The earlier user message is untouched (still single text content).
    assert!(matches!(
        msgs[0].content,
        Some(OneOrMany::One(GaiseContent::Text { .. }))
    ));
    // The latest user message now carries text first, then both images, in order.
    match &msgs[2].content {
        Some(OneOrMany::Many(blocks)) => {
            assert_eq!(blocks.len(), 3);
            assert!(matches!(&blocks[0], GaiseContent::Text { text } if text == "look at this"));
            assert!(matches!(&blocks[1], GaiseContent::Image { data, .. } if data == &[1]));
            assert!(matches!(&blocks[2], GaiseContent::Image { data, .. } if data == &[2]));
        }
        other => panic!("expected Many content, got {other:?}"),
    }
}

#[test]
fn images_noop_when_empty_or_no_user_message() {
    // No images: leave the message exactly as-is (still a single text content).
    let mut msgs = vec![user("hi")];
    attach_images_to_last_user(&mut msgs, &[]);
    assert!(matches!(
        msgs[0].content,
        Some(OneOrMany::One(GaiseContent::Text { .. }))
    ));
    // No user message to attach to: nothing changes, no panic.
    let mut only_assistant = vec![msg(Some("reply"), None)];
    attach_images_to_last_user(&mut only_assistant, &[img(9)]);
    assert!(matches!(
        only_assistant[0].content,
        Some(OneOrMany::One(GaiseContent::Text { .. }))
    ));
}

#[test]
fn empty_turn_is_not_sendable() {
    // content=None, no tool_calls → the OpenAI "content: null" rejection case.
    assert!(!is_sendable_message(&msg(None, None)));
    // content=None with an *empty* tool_calls vec is still empty.
    assert!(!is_sendable_message(&msg(None, Some(vec![]))));
}

#[test]
fn turns_with_content_or_tool_calls_are_sendable() {
    assert!(is_sendable_message(&msg(Some("hello"), None)));
    assert!(is_sendable_message(&msg(Some(""), None)));
    // A tool-call-only turn legitimately has null content — it must be kept.
    assert!(is_sendable_message(&msg(
        None,
        Some(vec![GaiseToolCall::default()])
    )));
}

#[test]
fn none_and_blank_stay_none() {
    assert_eq!(sanitize_tool_arguments(None), None);
    assert_eq!(sanitize_tool_arguments(Some("   ")), None);
}

#[test]
fn clean_object_round_trips() {
    let out = sanitize_tool_arguments(Some(r#"{"question":"hi"}"#)).unwrap();
    assert_eq!(out, r#"{"question":"hi"}"#);
}

#[test]
fn trailing_object_after_first_is_dropped() {
    // The reported failure: a valid object followed by a second one (streaming/provider
    // quirk) — strict from_str would reject it with "trailing characters".
    let raw = "{\n  \"question\": \"What writes to the WAL log\"\n}\n{\"stray\":true}";
    let out = sanitize_tool_arguments(Some(raw)).unwrap();
    assert_eq!(out, r#"{"question":"What writes to the WAL log"}"#);
}

#[test]
fn trailing_text_after_object_is_dropped() {
    let out = sanitize_tool_arguments(Some(r#"{"k":1} oops"#)).unwrap();
    assert_eq!(out, r#"{"k":1}"#);
}

#[test]
fn non_json_passes_through_for_the_tool_to_report() {
    // Not a JSON value at all: leave it so the tool surfaces a real parse error
    // rather than this sanitiser masking it.
    assert_eq!(
        sanitize_tool_arguments(Some("not json")),
        Some("not json".to_string())
    );
}
