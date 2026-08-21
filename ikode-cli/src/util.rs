//! Small, dependency-light helpers used by the CLI binary: token-count
//! formatting, the working-directory path guard, history/usage predicates, and
//! the command/argument normalisers. Kept free of `App` so they stay easy to
//! unit-test in isolation — they live in the library (rather than the binary) so
//! the integration tests under `tests/` can exercise them directly.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use anyhow::{anyhow, Result};
use gaise_core::contracts::{GaiseContent, GaiseMessage, GaiseUsage, OneOrMany};

use crate::settings::{Effort, Mode};

static ATOMIC_WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Replace `path` with bytes produced by `write` without ever exposing a partially
/// written destination. The temporary file lives beside the destination so the
/// final replace stays on one filesystem; its contents are synced before replace.
pub fn atomic_write_with<F>(path: &Path, write: F) -> io::Result<()>
where
    F: FnOnce(&mut File) -> io::Result<()>,
{
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("ikode-state");

    let mut candidate = None;
    for _ in 0..16 {
        let sequence = ATOMIC_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(
            ".{file_name}.tmp-{}-{sequence}",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => {
                candidate = Some((temp, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    let (temp, mut file) = candidate.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "could not allocate a temporary file beside {}",
                path.display()
            ),
        )
    })?;

    let result = (|| {
        write(&mut file)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        replace_file(&temp, path)?;
        sync_parent(parent);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Atomically replace `path` with `contents`.
pub fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    atomic_write_with(path, |file| file.write_all(contents))
}

#[cfg(not(windows))]
fn replace_file(temp: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temp, destination)
}

#[cfg(windows)]
fn replace_file(temp: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let temp: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both pointers refer to NUL-terminated UTF-16 buffers that remain
    // alive for the duration of the call.
    let moved = unsafe {
        MoveFileExW(
            temp.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn sync_parent(parent: &Path) {
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) {}

/// Human-readable IEC byte count for storage/status output.
pub fn human_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{} B", bytes as u64)
    }
}

/// Parse a byte budget such as `512`, `64KiB`, `20 MB`, or `1gib`.
pub fn parse_byte_size(value: &str) -> Result<u64> {
    let compact: String = value.chars().filter(|ch| !ch.is_whitespace()).collect();
    let split = compact
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(compact.len());
    let number = compact[..split]
        .parse::<u64>()
        .map_err(|_| anyhow!("invalid byte size '{value}'"))?;
    let multiplier = match compact[split..].to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => 1024 * 1024,
        "g" | "gb" | "gib" => 1024 * 1024 * 1024,
        suffix => return Err(anyhow!("unsupported byte-size suffix '{suffix}'")),
    };
    number
        .checked_mul(multiplier)
        .ok_or_else(|| anyhow!("byte size '{value}' is too large"))
}

fn preferred_counter(counters: &HashMap<String, usize>, names: &[&str]) -> Option<usize> {
    names.iter().find_map(|name| counters.get(*name).copied())
}

/// Flatten a usage record into `(input, output, cached)` token counts.
///
/// GAISe retains provider aggregates alongside modality, reasoning, cache, and
/// tool breakdowns. Those detail counters overlap their aggregate and must not be
/// added to it. Prefer each supported provider's aggregate name; the maximum is a
/// conservative fallback for a custom provider that reports unfamiliar keys.
pub fn usage_totals(u: &GaiseUsage) -> (usize, usize, usize) {
    let input = u
        .input
        .as_ref()
        .map(|counters| {
            preferred_counter(
                counters,
                &[
                    "effective_input_tokens",
                    "prompt_tokens",
                    "input_tokens",
                    "transcription_input_tokens",
                ],
            )
            .or_else(|| counters.values().copied().max())
            .unwrap_or(0)
        })
        .unwrap_or(0);
    let cached = u
        .input
        .as_ref()
        .map(|counters| {
            preferred_counter(counters, &["cached_tokens", "cache_read_input_tokens"])
                .or_else(|| {
                    counters
                        .iter()
                        .filter(|(name, _)| name.contains("cached") || name.contains("cache_read"))
                        .map(|(_, value)| *value)
                        .max()
                })
                .unwrap_or(0)
        })
        .unwrap_or(0);
    let output = u
        .output
        .as_ref()
        .map(|counters| {
            preferred_counter(
                counters,
                &[
                    "completion_tokens",
                    "output_tokens",
                    "candidates_tokens",
                    "response_tokens",
                    "transcription_output_tokens",
                ],
            )
            .or_else(|| counters.values().copied().max())
            .unwrap_or(0)
        })
        .unwrap_or(0);
    (input, output, cached)
}

/// `1234` -> `1,234`.
pub fn comma(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    let bytes = s.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

/// Compact magnitude for the running session total: `18432` -> `18.4k`.
pub fn abbrev(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Resolve `path` against `base` (the working directory) and confirm it stays inside.
/// Both sides are canonicalised so the comparison is robust on Windows, where
/// `canonicalize()` yields a `\\?\` verbatim prefix: canonicalising only the requested
/// path (as a naive impl does) makes `starts_with` fail against a non-verbatim base, so
/// every *existing* file is wrongly rejected as "outside the working directory". A path
/// that doesn't exist yet (e.g. a file to be created) is resolved against the
/// already-canonical base, and `..` traversal out of the tree is still rejected.
pub fn resolve_within(base: &Path, path: &str) -> Result<PathBuf> {
    let base = base.canonicalize().map_err(|e| {
        anyhow!(
            "Could not resolve working directory '{}': {e}",
            base.display()
        )
    })?;
    let requested = Path::new(path);
    let joined = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        base.join(requested)
    };
    let canonical = canonicalize_allow_missing(&joined)?;

    if !canonical.starts_with(&base) {
        return Err(anyhow!(
            "Path '{}' is outside the working directory. For security reasons, file operations are restricted to the working directory and its subdirectories.",
            path
        ));
    }
    Ok(canonical)
}

/// Canonicalise the longest existing prefix of `path`, then lexically resolve the
/// missing suffix. This handles create targets with several nonexistent directory
/// levels without leaving unchecked `..` components in the returned path.
fn canonicalize_allow_missing(path: &Path) -> Result<PathBuf> {
    if let Ok(canonical) = path.canonicalize() {
        return Ok(canonical);
    }

    let ancestor = path
        .ancestors()
        .find(|candidate| candidate.exists())
        .ok_or_else(|| anyhow!("Path '{}' has no existing ancestor", path.display()))?;
    let canonical_ancestor = ancestor
        .canonicalize()
        .map_err(|e| anyhow!("Could not resolve '{}': {e}", ancestor.display()))?;
    let suffix = path
        .strip_prefix(ancestor)
        .map_err(|_| anyhow!("Could not resolve path '{}'.", path.display()))?;
    normalize_absolute(&canonical_ancestor.join(suffix))
}

/// Resolve `.` and `..` components without consulting the filesystem. Input is
/// always rooted at a canonical existing ancestor, so an attempt to walk above the
/// filesystem root is invalid rather than a relative path to preserve.
fn normalize_absolute(path: &Path) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(std::path::MAIN_SEPARATOR.to_string()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return Err(anyhow!(
                        "Path '{}' attempts to traverse above the filesystem root.",
                        path.display()
                    ));
                }
            }
            std::path::Component::Normal(part) => normalized.push(part),
        }
    }
    Ok(normalized)
}

/// Is this message worth sending to the provider? A message contributes only if it
/// carries content or at least one tool call. An empty assistant turn (model streamed
/// nothing → `content: None`, no `tool_calls`) is noise and, worse, makes providers
/// reject the whole request (`content: null` is invalid without tool_calls).
pub fn is_sendable_message(m: &GaiseMessage) -> bool {
    m.content.is_some()
        || m.tool_calls
            .as_ref()
            .map(|t| !t.is_empty())
            .unwrap_or(false)
}

/// Append ephemeral content blocks to the most recent `user` message after its
/// stored text. This is the common request-only splice used by image and document
/// attachments; a single block is promoted to `Many` without modifying history.
pub fn attach_content_to_last_user(msgs: &mut [GaiseMessage], attachments: &[GaiseContent]) {
    if attachments.is_empty() {
        return;
    }
    if let Some(m) = msgs.iter_mut().rev().find(|m| m.role == "user") {
        let mut blocks: Vec<GaiseContent> = match m.content.take() {
            Some(OneOrMany::One(c)) => vec![c],
            Some(OneOrMany::Many(v)) => v,
            None => Vec::new(),
        };
        blocks.extend(attachments.iter().cloned());
        m.content = Some(OneOrMany::Many(blocks));
    }
}

/// Backwards-compatible image-specific name retained for library callers.
pub fn attach_images_to_last_user(msgs: &mut [GaiseMessage], images: &[GaiseContent]) {
    attach_content_to_last_user(msgs, images);
}

/// First 8 chars of a session UUID — enough to recognise/disambiguate in listings.
pub fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// The local date (`YYYY-MM-DD`) a session file was created, used to re-anchor a
/// resumed prompt to when the conversation began. Falls back to last-modified when
/// the platform doesn't expose a creation time; `None` if neither is available.
pub fn session_start_date(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    let when = meta.created().or_else(|_| meta.modified()).ok()?;
    let dt: chrono::DateTime<chrono::Local> = when.into();
    Some(dt.format("%Y-%m-%d").to_string())
}

/// Collapse a preview to a single line and cap its length with an ellipsis.
pub fn truncate_preview(s: &str, max: usize) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > max {
        let mut t: String = one_line.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    } else {
        one_line
    }
}

/// Coarse "Xs/m/h/d ago" rendering of a timestamp for the resume picker.
pub fn rel_time(t: SystemTime) -> String {
    match t.elapsed() {
        Ok(d) => {
            let s = d.as_secs();
            if s < 60 {
                format!("{s}s ago")
            } else if s < 3600 {
                format!("{}m ago", s / 60)
            } else if s < 86_400 {
                format!("{}h ago", s / 3600)
            } else {
                format!("{}d ago", s / 86_400)
            }
        }
        Err(_) => "just now".to_string(),
    }
}

/// Live, per-turn statement of the active permission mode, appended to the system
/// prompt so the model always knows its current capabilities (rather than guessing
/// from the tool list and anchoring on stale refusals).
pub fn mode_status_line(mode: Mode) -> &'static str {
    match mode {
        Mode::Plan => {
            "ACTIVE MODE: PLAN (read-only). The mutating tools (create_file, edit_file, \
            delete_file, execute_command) are NOT in your tool list this turn. Investigate and \
            produce a concrete plan; tell the user to run `/mode agentic` to apply changes."
        }
        Mode::Agentic => {
            "ACTIVE MODE: AGENTIC. The mutating tools (create_file, edit_file, \
            delete_file, execute_command) ARE available this turn — call them directly to make the \
            requested changes (each runs after a quick user confirmation). Do NOT tell the user to \
            switch modes or claim you are read-only; you can edit files now."
        }
        Mode::Yolo => {
            "ACTIVE MODE: YOLO. All mutating tools are available and run without \
            confirmation (deny rules still apply). Proceed with changes directly."
        }
    }
}

/// Live, per-turn reasoning/delegation statement appended beside the permission
/// mode. This makes `ultra`'s orchestration semantics explicit to the lead model
/// while keeping lower levels opt-in for delegation.
pub fn effort_status_line(effort: Effort) -> &'static str {
    match effort {
        Effort::Auto => {
            "ACTIVE EFFORT: AUTO. Use the selected model's normal reasoning depth. Use subagents only when the user explicitly requests parallel or delegated work."
        }
        Effort::Low => {
            "ACTIVE EFFORT: LOW. Prefer the shortest reliable path, avoid speculative exploration, and do not delegate unless the user explicitly requests it."
        }
        Effort::Medium => {
            "ACTIVE EFFORT: MEDIUM. Balance speed with verification. Delegate only when the user explicitly requests parallel work."
        }
        Effort::High => {
            "ACTIVE EFFORT: HIGH. Trace complex logic, check assumptions and edge cases, and verify the result. Delegate only when requested or when project instructions require it."
        }
        Effort::Max => {
            "ACTIVE EFFORT: MAX. Apply the deepest available reasoning and thorough verification. Delegate only when requested or when project instructions require it."
        }
        Effort::Ultra => {
            "ACTIVE EFFORT: ULTRA. Apply the deepest available reasoning and proactively delegate genuinely independent, read-heavy subtasks to parallel subagents when that materially improves speed or quality. Keep sequential or write-heavy work with the lead, wait for the workers you need, and consolidate their results."
        }
    }
}

/// `/ask_codebase` (snake_case, matching the `ask_codebase` tool name) is an
/// alias for the kebab-case REPL command `/ask-codebase`. Returns the rewritten
/// line when `input` uses the underscore spelling, else `None`. The remainder
/// (including the separating space) is preserved verbatim.
pub fn normalize_command(input: &str) -> Option<String> {
    if let Some(rest) = input.strip_prefix("/ask_codebase") {
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return Some(format!("/ask-codebase{}", rest));
        }
    }
    None
}

/// Normalise model-produced tool-call arguments before per-tool parsing. Providers
/// occasionally stream arguments with trailing junk after the JSON object (e.g. a
/// second value concatenated, or stray text), which makes a strict
/// `serde_json::from_str` fail with "trailing characters". Extract just the first JSON
/// value and re-serialise it so each tool receives clean input. `None`/blank stays
/// `None` (tools default to `{}`); unparseable input is passed through unchanged so the
/// tool surfaces a meaningful error rather than this function hiding it.
/// Mask a credential for display: `***` plus the last four characters, enough
/// to tell keys apart without revealing them. Values of four characters or
/// fewer mask entirely (`***`) — showing the tail would show the whole secret.
pub fn mask_secret(value: &str) -> String {
    let value = value.trim();
    let chars = value.chars().count();
    if chars <= 4 {
        return "***".to_string();
    }
    let tail: String = value.chars().skip(chars - 4).collect();
    format!("***{tail}")
}

pub fn sanitize_tool_arguments(arguments: Option<&str>) -> Option<String> {
    let raw = arguments?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut values = serde_json::Deserializer::from_str(trimmed).into_iter::<serde_json::Value>();
    match values.next() {
        Some(Ok(value)) => serde_json::to_string(&value).ok(),
        _ => Some(raw.to_string()),
    }
}
