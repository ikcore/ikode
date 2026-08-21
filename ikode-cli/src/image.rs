//! Loading and validating local image files for chat attachments.
//!
//! An image is sent to the model as a `GaiseContent::Image { data, format }` block
//! — raw bytes plus a MIME type that every GAISe provider (Anthropic, OpenAI,
//! Gemini, Ollama, Vertex, Bedrock) already maps. To maximise cross-provider
//! compatibility this module is deliberately strict about what it accepts:
//!   - the format is **sniffed from the file's magic bytes**, never trusted from
//!     the extension, so a mislabelled file can't slip through as the wrong type;
//!   - only the universally-supported set (PNG, JPEG, GIF, WebP) is allowed —
//!     these are the formats accepted across the providers above;
//!   - the byte size is capped conservatively, since providers reject very large
//!     images (Anthropic's ceiling is ~5 MB) and a too-big payload fails the turn.
//!
//! Attachments are **ephemeral**: the bytes are carried only for the turn they're
//! sent on (injected into the request, never written to the session transcript).
//! See `App::process_prompt` / `App::build_request_history`.

use std::path::{Path, PathBuf};

use gaise_core::contracts::GaiseContent;

/// Largest image we will attach, in bytes. Anthropic rejects images above ~5 MB,
/// and the other providers have comparable ceilings; staying under it keeps a
/// single attachment acceptable everywhere rather than failing late at the API.
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

/// A validated image ready to attach to a turn: the raw bytes, their MIME type,
/// and a short human label for the echo / transcript breadcrumb (e.g.
/// `screenshot.png (image/png, 24 KB)`). Deliberately holds primitives rather than
/// a `GaiseContent` so it stays `Eq` — the editor's attachment store keeps one of
/// these behind a sentinel char, exactly like a pasted-text attachment. Call
/// [`LoadedImage::content`] to materialise the model-bound block at send time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedImage {
    pub label: String,
    pub media_type: String,
    pub data: Vec<u8>,
}

impl LoadedImage {
    /// The model-bound content block for this image.
    pub fn content(&self) -> GaiseContent {
        GaiseContent::Image {
            data: self.data.clone(),
            format: Some(self.media_type.clone()),
        }
    }
}

/// Detect a supported image format from its leading bytes, returning the canonical
/// `image/<fmt>` MIME type providers expect. Returns `None` for anything outside
/// the widely-supported set, so an unknown/unsupported file is rejected rather
/// than sent under a guessed type.
pub fn sniff_media_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        // WebP is a RIFF container: "RIFF" <u32 size> "WEBP".
        Some("image/webp")
    } else {
        None
    }
}

/// Load and validate a local image file for attachment. `raw` is the path exactly
/// as the user typed it; surrounding quotes and whitespace (common when a path is
/// dragged into the terminal) are trimmed first. On failure the returned `Err`
/// carries a user-facing message ready to print.
pub fn load_image(raw: &str, base: &Path) -> Result<LoadedImage, String> {
    let trimmed = raw.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    if trimmed.is_empty() {
        return Err("no image path given".to_string());
    }
    let path = resolve(base, trimmed);
    let data = std::fs::read(&path).map_err(|e| format!("could not read {trimmed}: {e}"))?;
    loaded_from_bytes(data, &file_name(&path, trimmed))
}

/// Validate already-in-memory image bytes (e.g. read from the clipboard) and wrap
/// them as a [`LoadedImage`]. Shares the format/size checks with [`load_image`] so
/// every attachment, whatever its source, is held to the same compatibility bar.
/// `name` is a display name for the label (e.g. `clipboard.png`).
pub fn loaded_from_bytes(data: Vec<u8>, name: &str) -> Result<LoadedImage, String> {
    if data.is_empty() {
        return Err(format!("{name} is empty"));
    }
    let media_type = sniff_media_type(&data).ok_or_else(|| {
        format!("{name} is not a supported image (expected PNG, JPEG, GIF or WebP)")
    })?;
    if data.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "{name} is too large ({}, max {})",
            human_size(data.len()),
            human_size(MAX_IMAGE_BYTES)
        ));
    }
    let label = format!("{name} ({media_type}, {})", human_size(data.len()));
    Ok(LoadedImage {
        label,
        media_type: media_type.to_string(),
        data,
    })
}

/// Image-file extensions recognised in a *bare* (unquoted) prompt token. Quoted
/// spans are probed regardless of extension, since a dragged-in path with spaces
/// arrives quoted and we want it to work whatever it's called.
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];

/// Whether a user path names one of the image extensions handled by this module.
/// Generic `/attach` uses this to avoid reinterpreting a corrupt `*.png` as text.
pub fn has_supported_extension(raw: &str) -> bool {
    let trimmed = raw.trim().trim_matches(|c| c == '"' || c == '\'');
    Path::new(trimmed)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            IMAGE_EXTS
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
        .unwrap_or(false)
}

/// Scan a chat prompt for references to local image files and load any that
/// resolve to a real, supported image, returning the prompt with those references
/// stripped plus the loaded images. This is what makes drag-drop "just work": a
/// terminal delivers a dragged file as a quoted path, which lands here as a normal
/// message. A candidate that does NOT resolve to a real image is left in the text
/// untouched, so ordinary prose that happens to mention a `.png` behaves exactly
/// as before. Relative paths resolve against `base`.
pub fn extract_inline_images(prompt: &str, base: &Path) -> (String, Vec<LoadedImage>) {
    // Candidates, most-specific first: quoted spans (may contain spaces), then bare
    // tokens that look path-like. Probing a non-existent/again-unsupported path just
    // fails fast and is skipped, so false positives can't change behaviour.
    let mut candidates: Vec<String> = quoted_spans(prompt);
    for tok in prompt.split_whitespace() {
        let t = tok.trim_matches(|c| c == '"' || c == '\'');
        if looks_like_image_path(t) {
            candidates.push(t.to_string());
        }
    }

    let mut text = prompt.to_string();
    let mut images = Vec::new();
    for cand in candidates {
        // Skip anything already removed (e.g. the same path seen quoted and bare).
        if !text.contains(&cand) {
            continue;
        }
        if let Ok(img) = load_image(&cand, base) {
            images.push(img);
            // Remove the reference (with any surrounding quotes) from the prose; the
            // attachment's breadcrumb is added separately by the turn path.
            for pat in [format!("\"{cand}\""), format!("'{cand}'"), cand.clone()] {
                text = text.replace(&pat, "");
            }
        }
    }

    if images.is_empty() {
        // Nothing matched — return the prompt untouched (preserve exact formatting).
        return (prompt.to_string(), Vec::new());
    }
    (text.trim().to_string(), images)
}

/// Substrings enclosed in matching single or double quotes, in order of appearance.
/// Used to recover a dragged path that contains spaces (terminals quote those).
fn quoted_spans(s: &str) -> Vec<String> {
    let mut spans = Vec::new();
    for quote in ['"', '\''] {
        let mut rest = s;
        while let Some(open) = rest.find(quote) {
            let after = &rest[open + 1..];
            if let Some(close) = after.find(quote) {
                let span = &after[..close];
                if !span.is_empty() {
                    spans.push(span.to_string());
                }
                rest = &after[close + 1..];
            } else {
                break;
            }
        }
    }
    spans
}

/// Whether a bare token is worth probing as an image path: it has a path separator
/// or ends in a known image extension. Keeps us from stat-ing every ordinary word.
fn looks_like_image_path(tok: &str) -> bool {
    if tok.is_empty() {
        return false;
    }
    let has_sep = tok.contains('/') || tok.contains('\\');
    let ext_ok = has_supported_extension(tok);
    has_sep || ext_ok
}

/// Resolve a user-supplied path: absolute paths as-is, relative ones against `base`.
fn resolve(base: &Path, p: &str) -> PathBuf {
    let path = Path::new(p);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

/// Display name for a resolved path, falling back to the raw token.
fn file_name(path: &Path, raw: &str) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(raw)
        .to_string()
}

/// Compact, human-readable byte size (e.g. `512 B`, `24 KB`, `3.1 MB`) for labels.
fn human_size(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = 1024 * 1024;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // Minimal valid-enough magic-byte headers for each supported format.
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];
    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0];
    const GIF: &[u8] = b"GIF89a....";

    fn webp() -> Vec<u8> {
        let mut v = b"RIFF".to_vec();
        v.extend_from_slice(&[0, 0, 0, 0]); // size (ignored)
        v.extend_from_slice(b"WEBP");
        v.extend_from_slice(b"VP8 ");
        v
    }

    #[test]
    fn sniffs_each_supported_format_and_rejects_others() {
        assert_eq!(sniff_media_type(PNG), Some("image/png"));
        assert_eq!(sniff_media_type(JPEG), Some("image/jpeg"));
        assert_eq!(sniff_media_type(GIF), Some("image/gif"));
        assert_eq!(sniff_media_type(&webp()), Some("image/webp"));
        // Plain text / unknown binary is not an image.
        assert_eq!(sniff_media_type(b"#!/bin/sh\n"), None);
        assert_eq!(sniff_media_type(&[]), None);
        // "RIFF" without the "WEBP" fourcc (e.g. a WAV) is not WebP.
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&[0, 0, 0, 0]);
        wav.extend_from_slice(b"WAVE");
        assert_eq!(sniff_media_type(&wav), None);
    }

    #[test]
    fn load_image_reads_bytes_and_builds_content() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("shot.png");
        std::fs::File::create(&p).unwrap().write_all(PNG).unwrap();

        let img = load_image(p.to_str().unwrap(), dir.path()).unwrap();
        assert!(img.label.starts_with("shot.png (image/png,"));
        assert_eq!(img.media_type, "image/png");
        assert_eq!(img.data, PNG);
        // The materialised content block carries the same bytes and MIME type.
        match img.content() {
            GaiseContent::Image { data, format } => {
                assert_eq!(data, PNG);
                assert_eq!(format.as_deref(), Some("image/png"));
            }
            other => panic!("expected image content, got {other:?}"),
        }
    }

    #[test]
    fn load_image_trims_surrounding_quotes() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("q.png");
        std::fs::File::create(&p).unwrap().write_all(PNG).unwrap();
        // Drag-drop often delivers a quoted path.
        let quoted = format!("\"{}\"", p.to_str().unwrap());
        assert!(load_image(&quoted, dir.path()).is_ok());
        // A relative path resolves against `base`.
        assert!(load_image("q.png", dir.path()).is_ok());
    }

    #[test]
    fn rejects_unsupported_and_empty_and_missing() {
        let dir = tempfile::tempdir().unwrap();
        // Unsupported content.
        let txt = dir.path().join("notes.txt");
        std::fs::File::create(&txt)
            .unwrap()
            .write_all(b"hello")
            .unwrap();
        assert!(load_image(txt.to_str().unwrap(), dir.path())
            .unwrap_err()
            .contains("not a supported image"));
        // Empty file.
        let empty = dir.path().join("empty.png");
        std::fs::File::create(&empty).unwrap();
        assert!(load_image(empty.to_str().unwrap(), dir.path())
            .unwrap_err()
            .contains("empty"));
        // Missing path.
        assert!(load_image(dir.path().join("nope.png").to_str().unwrap(), dir.path()).is_err());
        // Blank arg.
        assert!(load_image("   ", dir.path())
            .unwrap_err()
            .contains("no image path"));
    }

    #[test]
    fn rejects_oversized_image() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.png");
        let mut bytes = PNG.to_vec();
        bytes.resize(MAX_IMAGE_BYTES + 1, 0);
        std::fs::File::create(&p)
            .unwrap()
            .write_all(&bytes)
            .unwrap();
        assert!(load_image(p.to_str().unwrap(), dir.path())
            .unwrap_err()
            .contains("too large"));
    }

    #[test]
    fn looks_like_image_path_filters_bare_tokens() {
        assert!(looks_like_image_path("shot.png"));
        assert!(looks_like_image_path("diagram.JPEG")); // case-insensitive ext
        assert!(looks_like_image_path("sub/dir/a.gif"));
        assert!(looks_like_image_path("C:\\users\\x\\pic")); // has separator
        assert!(!looks_like_image_path("hello"));
        assert!(!looks_like_image_path("notes.txt"));
        assert!(!looks_like_image_path(""));
        assert!(has_supported_extension("\"SHOT.PNG\""));
        assert!(!has_supported_extension("notes.txt"));
    }

    #[test]
    fn quoted_spans_recovers_paths_with_spaces() {
        let got = quoted_spans("look at \"funky chars.png\" please");
        assert_eq!(got, vec!["funky chars.png".to_string()]);
        // Single quotes too; empty quotes ignored.
        let got = quoted_spans("a 'b.png' c '' d");
        assert_eq!(got, vec!["b.png".to_string()]);
    }

    #[test]
    fn extract_inline_attaches_dragged_quoted_path_and_strips_it() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::File::create(dir.path().join("funky chars.png"))
            .unwrap()
            .write_all(PNG)
            .unwrap();
        // Mirrors a terminal drag-drop: an absolute, quoted path with spaces.
        let abs = dir.path().join("funky chars.png");
        let prompt = format!("what is in \"{}\"?", abs.to_str().unwrap());

        let (text, images) = extract_inline_images(&prompt, dir.path());
        assert_eq!(images.len(), 1);
        assert!(images[0].label.starts_with("funky chars.png"));
        // The raw path (and its quotes) are gone; the prose remains.
        assert!(!text.contains(".png"));
        assert!(text.contains("what is in"));
        assert!(text.contains('?'));
    }

    #[test]
    fn extract_inline_attaches_bare_relative_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::File::create(dir.path().join("a.png"))
            .unwrap()
            .write_all(PNG)
            .unwrap();
        let (text, images) = extract_inline_images("review a.png for me", dir.path());
        assert_eq!(images.len(), 1);
        assert!(!text.contains("a.png")); // path removed
        assert!(text.contains("review") && text.contains("for me"));
    }

    #[test]
    fn extract_inline_leaves_prose_untouched_when_nothing_resolves() {
        let dir = tempfile::tempdir().unwrap();
        // Mentions a .png that doesn't exist, and an existing non-image file.
        std::fs::File::create(dir.path().join("notes.txt"))
            .unwrap()
            .write_all(b"x")
            .unwrap();
        let prompt = "see missing.png and notes.txt\nsecond line";
        let (text, images) = extract_inline_images(prompt, dir.path());
        assert!(images.is_empty());
        // Returned verbatim — formatting (newline) preserved.
        assert_eq!(text, prompt);
    }
}
