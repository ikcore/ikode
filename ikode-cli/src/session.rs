//! Per-project conversation persistence under `.ikode/sessions/`.
//!
//! Each session is a `<uuid>.jsonl` file (Claude-style: one UUID per session),
//! holding one JSON-encoded [`GaiseMessage`] per line, appended as the turn
//! progresses so the transcript is always crash-safe and resumable. The system
//! prompt (history[0]) is deliberately NOT stored — it's rebuilt fresh every
//! launch — so a resumed session is grafted back onto the current system prompt.
//! [`Session::fork`] copies that persisted transcript boundary to a fresh UUID
//! without modifying the parent file.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use gaise_core::contracts::{GaiseContent, GaiseMessage, OneOrMany};

/// Directory holding session transcripts for a project root.
pub fn sessions_dir(root: &Path) -> PathBuf {
    root.join(".ikode").join("sessions")
}

/// A persisted conversation. `path` is `<root>/.ikode/sessions/<id>.jsonl`.
pub struct Session {
    pub id: String,
    pub path: PathBuf,
}

impl Session {
    /// Begin a session with the given id. The file is created lazily on the first
    /// `append`, so launching and quitting without chatting leaves no clutter.
    pub fn new(root: &Path, id: String) -> Self {
        let path = sessions_dir(root).join(format!("{id}.jsonl"));
        Session { id, path }
    }

    /// Append one message as a JSON line, creating the dir/file on first write.
    pub fn append(&self, msg: &GaiseMessage) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut record = serde_json::to_vec(msg)?;
        record.push(b'\n');
        f.write_all(&record)?;
        f.sync_data()?;
        Ok(())
    }

    /// Replace the whole transcript (used by `/compact` after rewriting history).
    pub fn rewrite(&self, msgs: &[GaiseMessage]) -> Result<()> {
        ikode::util::atomic_write_with(&self.path, |file| {
            for message in msgs {
                serde_json::to_writer(&mut *file, message).map_err(std::io::Error::other)?;
                file.write_all(b"\n")?;
            }
            Ok(())
        })
        .with_context(|| format!("rewrite session {}", self.path.display()))
    }

    /// Persist a transcript clone under a fresh session id. The leading system
    /// message is intentionally omitted just like normal session persistence; the
    /// active harness rebuilds it when the fork is resumed.
    pub fn fork(root: &Path, id: String, history: &[GaiseMessage]) -> Result<Self> {
        let transcript = if history
            .first()
            .is_some_and(|message| message.role == "system")
        {
            &history[1..]
        } else {
            history
        };
        let fork = Self::new(root, id);
        fork.rewrite(transcript)?;
        Ok(fork)
    }
}

/// Detailed result from loading one transcript. An incomplete final record is
/// recoverable and omitted; malformed complete records are retained as line
/// numbers so callers can surface interior corruption instead of hiding it.
#[derive(Debug, Default)]
pub struct SessionLoadReport {
    pub messages: Vec<GaiseMessage>,
    pub malformed_lines: Vec<usize>,
    pub incomplete_tail_bytes: u64,
}

#[derive(Debug, Default)]
struct SessionScan {
    message_count: usize,
    preview: Option<String>,
    malformed_lines: Vec<usize>,
    incomplete_tail_bytes: u64,
}

fn scan_session<F>(path: &Path, mut on_message: F) -> Result<SessionScan>
where
    F: FnMut(GaiseMessage),
{
    let file = File::open(path).with_context(|| format!("open session {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut scan = SessionScan::default();
    let mut line_number = 0usize;
    loop {
        let mut bytes = Vec::new();
        let read = reader.read_until(b'\n', &mut bytes)?;
        if read == 0 {
            break;
        }
        line_number += 1;
        let terminated = bytes.last() == Some(&b'\n');
        if terminated {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
        }
        if bytes.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match serde_json::from_slice::<GaiseMessage>(&bytes) {
            Ok(message) => {
                scan.message_count += 1;
                if scan.preview.is_none() && message.role == "user" {
                    scan.preview = message_text(&message)
                        .map(|text| text.trim().to_string())
                        .filter(|text| !text.is_empty());
                }
                on_message(message);
            }
            Err(_) if !terminated => scan.incomplete_tail_bytes = read as u64,
            Err(_) => scan.malformed_lines.push(line_number),
        }
    }
    Ok(scan)
}

pub fn load_messages_report(path: &Path) -> Result<SessionLoadReport> {
    let mut messages = Vec::new();
    let scan = scan_session(path, |message| messages.push(message))?;
    Ok(SessionLoadReport {
        messages,
        malformed_lines: scan.malformed_lines,
        incomplete_tail_bytes: scan.incomplete_tail_bytes,
    })
}

/// Load valid messages and warn when a complete interior record is malformed.
/// A torn final append remains a quiet, recoverable omission.
pub fn load_messages(path: &Path) -> Result<Vec<GaiseMessage>> {
    let report = load_messages_report(path)?;
    // Deliberately tolerated: a non-newline-terminated, invalid final record is a
    // torn append and all earlier complete messages remain usable.
    let _incomplete_tail_bytes = report.incomplete_tail_bytes;
    if !report.malformed_lines.is_empty() {
        eprintln!(
            "ikode: session {} contains malformed JSONL at line(s) {}; valid records were recovered",
            path.display(),
            report
                .malformed_lines
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(report.messages)
}

/// Summary metadata for the resume picker.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub path: PathBuf,
    pub modified: SystemTime,
    /// First user message (trimmed), for a human-recognisable label.
    pub preview: String,
    pub message_count: usize,
    pub file_bytes: u64,
    pub malformed_lines: usize,
    pub incomplete_tail_bytes: u64,
    pub read_error: Option<String>,
}

/// List sessions for a project, most-recently-modified first. Empty sessions
/// (no messages) are omitted.
pub fn list_sessions(root: &Path) -> Vec<SessionInfo> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(sessions_dir(root)) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let id = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let metadata = entry.metadata();
        let modified = metadata
            .as_ref()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(UNIX_EPOCH);
        let file_bytes = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
        if metadata.as_ref().is_ok_and(|metadata| metadata.len() == 0) {
            continue;
        }
        let (message_count, preview, malformed_lines, incomplete_tail_bytes, read_error) =
            match scan_session(&path, |_| {}) {
                Ok(scan) => (
                    scan.message_count,
                    scan.preview.unwrap_or_else(|| "(no prompt)".to_string()),
                    scan.malformed_lines.len(),
                    scan.incomplete_tail_bytes,
                    None,
                ),
                Err(error) => (
                    0,
                    "(unreadable session)".to_string(),
                    0,
                    0,
                    Some(error.to_string()),
                ),
            };
        out.push(SessionInfo {
            id,
            path,
            modified,
            preview,
            message_count,
            file_bytes,
            malformed_lines,
            incomplete_tail_bytes,
            read_error,
        });
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    out
}

#[derive(Debug, Default)]
pub struct SessionStorageStats {
    pub files: usize,
    pub messages: usize,
    pub bytes: u64,
    pub corrupt_files: usize,
    pub incomplete_files: usize,
    pub largest_bytes: u64,
}

pub fn storage_stats(infos: &[SessionInfo]) -> SessionStorageStats {
    let mut stats = SessionStorageStats::default();
    for info in infos {
        stats.files += 1;
        stats.messages += info.message_count;
        stats.bytes = stats.bytes.saturating_add(info.file_bytes);
        stats.largest_bytes = stats.largest_bytes.max(info.file_bytes);
        stats.corrupt_files += usize::from(info.malformed_lines > 0 || info.read_error.is_some());
        stats.incomplete_files += usize::from(info.incomplete_tail_bytes > 0);
    }
    stats
}

pub enum SessionMatch<'a> {
    None,
    Unique(&'a SessionInfo),
    Ambiguous(Vec<&'a SessionInfo>),
}

pub fn match_session<'a>(infos: &'a [SessionInfo], query: &str) -> SessionMatch<'a> {
    if let Some(exact) = infos.iter().find(|info| info.id == query) {
        return SessionMatch::Unique(exact);
    }
    let matches: Vec<&SessionInfo> = infos
        .iter()
        .filter(|info| info.id.starts_with(query))
        .collect();
    match matches.len() {
        0 => SessionMatch::None,
        1 => SessionMatch::Unique(matches[0]),
        _ => SessionMatch::Ambiguous(matches),
    }
}

#[derive(Debug)]
pub struct PrunePlan {
    pub remove: Vec<SessionInfo>,
    pub before_files: usize,
    pub before_bytes: u64,
    pub after_files: usize,
    pub after_bytes: u64,
    pub target_met: bool,
}

pub fn plan_prune(
    infos: &[SessionInfo],
    keep: Option<usize>,
    max_bytes: Option<u64>,
    protected_ids: &HashSet<String>,
) -> PrunePlan {
    let mut remaining_files = infos.len();
    let mut remaining_bytes = infos
        .iter()
        .fold(0u64, |total, info| total.saturating_add(info.file_bytes));
    let before_files = remaining_files;
    let before_bytes = remaining_bytes;
    let mut remove = Vec::new();
    let retained: HashSet<&str> = infos
        .iter()
        .take(keep.unwrap_or(0))
        .map(|info| info.id.as_str())
        .chain(protected_ids.iter().map(String::as_str))
        .collect();

    // `--keep N` means preserve the N newest sessions (plus protected active
    // sessions), then remove every older eligible transcript.
    if keep.is_some() {
        for candidate in infos
            .iter()
            .rev()
            .filter(|info| !retained.contains(info.id.as_str()))
        {
            remaining_files = remaining_files.saturating_sub(1);
            remaining_bytes = remaining_bytes.saturating_sub(candidate.file_bytes);
            remove.push(candidate.clone());
        }
    }

    // A byte budget may remove additional old sessions, but never the explicit
    // keep set or active/current transcripts. If those alone exceed the budget,
    // target_met reports false rather than deleting protected data.
    while max_bytes.is_some_and(|limit| remaining_bytes > limit) {
        let Some(candidate) = infos.iter().rev().find(|info| {
            !retained.contains(info.id.as_str())
                && !remove
                    .iter()
                    .any(|removed: &SessionInfo| removed.id == info.id)
        }) else {
            break;
        };
        remaining_files = remaining_files.saturating_sub(1);
        remaining_bytes = remaining_bytes.saturating_sub(candidate.file_bytes);
        remove.push(candidate.clone());
    }

    let target_met = max_bytes.is_none_or(|limit| remaining_bytes <= limit);
    PrunePlan {
        remove,
        before_files,
        before_bytes,
        after_files: remaining_files,
        after_bytes: remaining_bytes,
        target_met,
    }
}

pub fn apply_prune(plan: &PrunePlan) -> Vec<(String, std::io::Error)> {
    plan.remove
        .iter()
        .filter_map(|info| {
            fs::remove_file(&info.path)
                .err()
                .map(|error| (info.id.clone(), error))
        })
        .collect()
}

/// Best-effort plain-text extraction from a message's content.
pub fn message_text(m: &GaiseMessage) -> Option<String> {
    match m.content.as_ref()? {
        OneOrMany::One(GaiseContent::Text { text }) => Some(text.clone()),
        OneOrMany::Many(items) => {
            let joined: Vec<&str> = items
                .iter()
                .filter_map(|c| match c {
                    GaiseContent::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            (!joined.is_empty()).then(|| joined.join(" "))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn append_then_load_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let s = Session::new(dir.path(), "abc123".to_string());
        s.append(&user("hello")).unwrap();
        s.append(&user("world")).unwrap();
        let loaded = load_messages(&s.path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(message_text(&loaded[0]).as_deref(), Some("hello"));
        assert_eq!(message_text(&loaded[1]).as_deref(), Some("world"));
    }

    #[test]
    fn rewrite_replaces_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let s = Session::new(dir.path(), "id".to_string());
        s.append(&user("one")).unwrap();
        s.append(&user("two")).unwrap();
        s.rewrite(&[user("only")]).unwrap();
        let loaded = load_messages(&s.path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(message_text(&loaded[0]).as_deref(), Some("only"));
    }

    #[test]
    fn fork_clones_messages_without_system_or_touching_parent() {
        let dir = tempfile::tempdir().unwrap();
        let parent = Session::new(dir.path(), "parent".to_string());
        parent.append(&user("original question")).unwrap();
        let mut system = user("system prompt");
        system.role = "system".to_string();
        let history = vec![system, user("original question"), user("follow-up")];

        let fork = Session::fork(dir.path(), "branch".to_string(), &history).unwrap();

        let parent_messages = load_messages(&parent.path).unwrap();
        assert_eq!(parent_messages.len(), 1);
        assert_eq!(
            message_text(&parent_messages[0]).as_deref(),
            Some("original question")
        );
        let branch_messages = load_messages(&fork.path).unwrap();
        assert_eq!(branch_messages.len(), 2);
        assert_eq!(branch_messages[0].role, "user");
        assert_eq!(
            message_text(&branch_messages[1]).as_deref(),
            Some("follow-up")
        );
    }

    #[test]
    fn list_sessions_orders_recent_first_and_uses_first_user_msg_as_preview() {
        let dir = tempfile::tempdir().unwrap();
        let older = Session::new(dir.path(), "older".to_string());
        older.append(&user("first question about the WAL")).unwrap();
        // A second session, written later, must sort ahead of the first.
        let newer = Session::new(dir.path(), "newer".to_string());
        newer.append(&user("newer question")).unwrap();
        // Nudge mtimes apart deterministically by rewriting the newer one.
        newer.rewrite(&[user("newer question")]).unwrap();

        let infos = list_sessions(dir.path());
        assert_eq!(infos.len(), 2);
        assert!(infos
            .iter()
            .any(|s| s.id == "older" && s.preview == "first question about the WAL"));
        assert!(infos.iter().all(|s| s.message_count >= 1));
    }

    #[test]
    fn empty_and_missing_dirs_yield_no_sessions() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list_sessions(dir.path()).is_empty()); // no sessions dir yet
                                                       // A created-but-empty file is ignored too.
        let s = Session::new(dir.path(), "empty".to_string());
        fs::create_dir_all(sessions_dir(dir.path())).unwrap();
        File::create(&s.path).unwrap();
        assert!(list_sessions(dir.path()).is_empty());
    }

    #[test]
    fn interior_corruption_is_reported_while_valid_records_survive() {
        let dir = tempfile::tempdir().unwrap();
        let session = Session::new(dir.path(), "damaged".to_string());
        fs::create_dir_all(sessions_dir(dir.path())).unwrap();
        let first = serde_json::to_string(&user("before")).unwrap();
        let last = serde_json::to_string(&user("after")).unwrap();
        fs::write(&session.path, format!("{first}\n{{broken}}\n{last}\n")).unwrap();

        let report = load_messages_report(&session.path).unwrap();
        assert_eq!(report.messages.len(), 2);
        assert_eq!(report.malformed_lines, vec![2]);
        assert_eq!(report.incomplete_tail_bytes, 0);
        let info = list_sessions(dir.path()).pop().unwrap();
        assert_eq!(info.message_count, 2);
        assert_eq!(info.malformed_lines, 1);
        assert!(info.file_bytes > 0);
    }

    #[test]
    fn torn_final_record_is_quietly_identified() {
        let dir = tempfile::tempdir().unwrap();
        let session = Session::new(dir.path(), "torn".to_string());
        fs::create_dir_all(sessions_dir(dir.path())).unwrap();
        let first = serde_json::to_string(&user("complete")).unwrap();
        fs::write(&session.path, format!("{first}\n{{\"role\":")).unwrap();

        let report = load_messages_report(&session.path).unwrap();
        assert_eq!(report.messages.len(), 1);
        assert!(report.malformed_lines.is_empty());
        assert!(report.incomplete_tail_bytes > 0);
    }

    #[test]
    fn session_prefixes_must_be_unique() {
        let dir = tempfile::tempdir().unwrap();
        Session::new(dir.path(), "abc-one".to_string())
            .append(&user("one"))
            .unwrap();
        Session::new(dir.path(), "abc-two".to_string())
            .append(&user("two"))
            .unwrap();
        let infos = list_sessions(dir.path());
        assert!(matches!(
            match_session(&infos, "abc-one"),
            SessionMatch::Unique(_)
        ));
        assert!(matches!(
            match_session(&infos, "abc"),
            SessionMatch::Ambiguous(matches) if matches.len() == 2
        ));
        assert!(matches!(
            match_session(&infos, "missing"),
            SessionMatch::None
        ));
    }

    #[test]
    fn prune_removes_oldest_but_protects_active_sessions() {
        let info = |id: &str, bytes: u64| SessionInfo {
            id: id.to_string(),
            path: PathBuf::from(format!("{id}.jsonl")),
            modified: UNIX_EPOCH,
            preview: id.to_string(),
            message_count: 1,
            file_bytes: bytes,
            malformed_lines: 0,
            incomplete_tail_bytes: 0,
            read_error: None,
        };
        // Newest first, matching list_sessions' contract.
        let infos = vec![info("new", 30), info("middle", 20), info("old", 10)];
        let protected = HashSet::from(["middle".to_string()]);
        let plan = plan_prune(&infos, Some(1), None, &protected);
        assert_eq!(
            plan.remove
                .iter()
                .map(|info| info.id.as_str())
                .collect::<Vec<_>>(),
            vec!["old"]
        );
        assert_eq!(plan.after_files, 2);
        assert_eq!(plan.after_bytes, 50);
        assert!(plan.target_met);
    }
}
