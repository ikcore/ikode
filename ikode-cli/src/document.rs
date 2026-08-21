//! Loading local documents for chat attachments.
//!
//! UTF-8 text and source files are wrapped as ordinary text content so every
//! provider, including Ollama, can consume them. Binary documents stay as
//! `GaiseContent::File` and are admitted only when the selected provider has a
//! native document request shape. Like images, document bytes are turn-scoped and
//! are never written into the JSONL conversation transcript.

use std::path::{Path, PathBuf};

use gaise_core::contracts::{file_media_type, GaiseContent};

/// Conservative cross-provider inline limit. Bedrock's Converse document limit is
/// lower than the limits exposed by OpenAI, Anthropic, and Gemini, so keeping files
/// at 4 MiB avoids accepting an attachment that fails only after a network request.
pub const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;

const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "json", "jsonl", "html", "htm", "xml", "yaml", "yml", "toml", "csv",
    "tsv", "log", "ini", "cfg", "conf", "rs", "py", "js", "jsx", "ts", "tsx", "java", "c", "h",
    "cc", "cpp", "hpp", "cs", "go", "rb", "php", "swift", "kt", "kts", "scala", "sh", "bash",
    "zsh", "fish", "ps1", "sql", "css", "scss", "sass", "less", "vue", "svelte", "astro", "proto",
    "graphql", "gql", "tex",
];

const BINARY_EXTENSIONS: &[&str] = &[
    "pdf", "doc", "docx", "rtf", "odt", "ppt", "pptx", "xls", "xlsx",
];

/// Extensions worth probing when they appear as a bare prompt token. Source-code
/// paths are intentionally absent: `please edit src/main.rs` must remain prose, not
/// silently turn into an attachment. Quoted or absolute dragged paths are probed
/// independently and may still attach source files.
const INLINE_DOCUMENT_EXTENSIONS: &[&str] = &[
    "pdf", "doc", "docx", "rtf", "odt", "ppt", "pptx", "xls", "xlsx", "txt", "csv", "tsv",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DocumentKind {
    Text,
    Binary,
}

/// A validated local document ready to accompany the next user message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedDocument {
    pub label: String,
    pub name: String,
    pub media_type: String,
    pub data: Vec<u8>,
    kind: DocumentKind,
}

impl LoadedDocument {
    /// Materialise the provider-neutral content block. Text is deliberately sent as
    /// text for universal compatibility; binary documents use the native file block.
    pub fn content(&self) -> GaiseContent {
        match self.kind {
            DocumentKind::Text => {
                let text = std::str::from_utf8(&self.data)
                    .expect("LoadedDocument text was validated as UTF-8");
                let quoted_name = serde_json::to_string(&self.name)
                    .expect("serializing a filename as JSON cannot fail");
                GaiseContent::Text {
                    text: format!(
                        "<attached_document name={quoted_name} media_type=\"{}\">\n{text}\n</attached_document>",
                        self.media_type
                    ),
                }
            }
            DocumentKind::Binary => GaiseContent::File {
                data: self.data.clone(),
                name: Some(self.name.clone()),
            },
        }
    }

    pub fn is_binary(&self) -> bool {
        self.kind == DocumentKind::Binary
    }

    fn extension(&self) -> &str {
        Path::new(&self.name)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
    }

    /// Validate native binary-document support for the currently selected provider.
    /// Text documents have already been converted to ordinary text and always pass.
    pub fn support_error(&self, model: &str) -> Option<String> {
        if !self.is_binary() {
            return None;
        }

        let (provider, _) = model.split_once("::").unwrap_or(("", model));
        let extension = self.extension();
        let is_pdf = extension.eq_ignore_ascii_case("pdf");
        let supported = match provider {
            // Chat Completions exposes a native `file` content part for these common
            // document families.
            "openai" => BINARY_EXTENSIONS
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate)),
            // Anthropic and the Google inline-document APIs have a stable PDF path;
            // text/source files use the universal text path above.
            "anthropic" | "gemini" | "vertexai" => is_pdf,
            // These are the binary formats represented by Bedrock Converse's
            // DocumentFormat mapping in the GAISe adapter.
            "bedrock" => ["pdf", "doc", "docx", "xls", "xlsx"]
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate)),
            // Ollama's chat request has text and image fields but no document field.
            _ => false,
        };

        (!supported).then(|| {
            format!(
                "{} cannot send {} documents with model {}. Use a PDF-capable provider, or attach a UTF-8 text/code file instead",
                self.name, extension.to_ascii_uppercase(), model
            )
        })
    }
}

/// Load a document path exactly as typed by the user. Surrounding quotes are
/// accepted because terminal drag-and-drop commonly inserts them.
pub fn load_document(raw: &str, base: &Path) -> Result<LoadedDocument, String> {
    let trimmed = raw.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    if trimmed.is_empty() {
        return Err("no document path given".to_string());
    }

    let path = resolve(base, trimmed);
    let data =
        std::fs::read(&path).map_err(|error| format!("could not read {trimmed}: {error}"))?;
    let name = clean_name(&path, trimmed);

    if data.is_empty() {
        return Err(format!("{name} is empty"));
    }
    if data.len() > MAX_DOCUMENT_BYTES {
        return Err(format!(
            "{name} is too large ({}, max {})",
            human_size(data.len()),
            human_size(MAX_DOCUMENT_BYTES)
        ));
    }
    if crate::image::sniff_media_type(&data).is_some() {
        return Err(format!("{name} is an image; use /image or /attach"));
    }
    if crate::image::has_supported_extension(trimmed) {
        return Err(format!(
            "{name} has an image extension but is not a valid supported image"
        ));
    }

    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    let known_binary = BINARY_EXTENSIONS
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate));
    let known_text = TEXT_EXTENSIONS
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate));

    let kind = if known_binary {
        validate_binary_signature(extension, &data, &name)?;
        DocumentKind::Binary
    } else if std::str::from_utf8(&data).is_ok() && !data.contains(&0) {
        // Explicit `/attach` also accepts extensionless and uncommon source files
        // when their bytes are unambiguously UTF-8 text.
        DocumentKind::Text
    } else if known_text {
        return Err(format!("{name} is not valid UTF-8 text"));
    } else {
        return Err(format!(
            "{name} is not a supported document (expected UTF-8 text/code, PDF, DOC/DOCX, RTF/ODT, PPT/PPTX, or XLS/XLSX)"
        ));
    };

    let inferred = file_media_type(Some(&name));
    let media_type = if kind == DocumentKind::Text && inferred == "application/octet-stream" {
        "text/plain"
    } else {
        inferred
    };
    let label = format!("{name} ({media_type}, {})", human_size(data.len()));

    Ok(LoadedDocument {
        label,
        name,
        media_type: media_type.to_string(),
        data,
        kind,
    })
}

/// Extract dragged/referenced local documents and remove only successfully loaded
/// paths from the prose. Failed candidates are left byte-for-byte unchanged.
pub fn extract_inline_documents(prompt: &str, base: &Path) -> (String, Vec<LoadedDocument>) {
    let mut candidates: Vec<String> = quoted_spans(prompt)
        .into_iter()
        .filter(|candidate| {
            Path::new(candidate).is_absolute() || looks_like_inline_document_path(candidate)
        })
        .collect();
    for token in prompt.split_whitespace() {
        let token = token.trim_matches(|c| c == '"' || c == '\'');
        if looks_like_inline_document_path(token) {
            candidates.push(token.to_string());
        }
    }

    let mut text = prompt.to_string();
    let mut documents = Vec::new();
    for candidate in candidates {
        if !text.contains(&candidate) {
            continue;
        }
        if let Ok(document) = load_document(&candidate, base) {
            documents.push(document);
            for pattern in [
                format!("\"{candidate}\""),
                format!("'{candidate}'"),
                candidate.clone(),
            ] {
                text = text.replace(&pattern, "");
            }
        }
    }

    if documents.is_empty() {
        (prompt.to_string(), Vec::new())
    } else {
        (text.trim().to_string(), documents)
    }
}

fn validate_binary_signature(extension: &str, data: &[u8], name: &str) -> Result<(), String> {
    let valid = if extension.eq_ignore_ascii_case("pdf") {
        data.windows(5).take(1024).any(|window| window == b"%PDF-")
    } else if ["docx", "xlsx", "pptx", "odt"]
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
    {
        data.starts_with(b"PK\x03\x04")
            || data.starts_with(b"PK\x05\x06")
            || data.starts_with(b"PK\x07\x08")
    } else if ["doc", "xls", "ppt"]
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
    {
        data.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1])
    } else if extension.eq_ignore_ascii_case("rtf") {
        data.starts_with(b"{\\rtf")
    } else {
        false
    };

    if valid {
        Ok(())
    } else {
        Err(format!(
            "{name} does not contain a valid {} document signature",
            extension.to_ascii_uppercase()
        ))
    }
}

fn quoted_spans(input: &str) -> Vec<String> {
    let mut spans = Vec::new();
    for quote in ['"', '\''] {
        let mut rest = input;
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

fn looks_like_inline_document_path(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let path = Path::new(token);
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            INLINE_DOCUMENT_EXTENSIONS
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
        .unwrap_or(false)
}

fn resolve(base: &Path, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn clean_name(path: &Path, fallback: &str) -> String {
    let raw = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(fallback);
    let clean: String = raw
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        "document".to_string()
    } else {
        clean.to_string()
    }
}

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

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::File::create(path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }

    #[test]
    fn loads_utf8_text_as_a_bounded_document_block() {
        let directory = tempfile::tempdir().unwrap();
        write(&directory.path().join("notes.md"), b"# Notes\nhello");

        let document = load_document("notes.md", directory.path()).unwrap();
        assert!(!document.is_binary());
        assert_eq!(document.name, "notes.md");
        assert_eq!(document.media_type, "text/markdown");
        assert!(document.label.starts_with("notes.md (text/markdown,"));
        match document.content() {
            GaiseContent::Text { text } => {
                assert!(text.contains("<attached_document name=\"notes.md\""));
                assert!(text.contains("# Notes\nhello"));
            }
            other => panic!("expected text content, got {other:?}"),
        }
    }

    #[test]
    fn loads_and_signature_checks_binary_documents() {
        let directory = tempfile::tempdir().unwrap();
        write(&directory.path().join("report.PDF"), b"%PDF-1.7\nbody");
        let document = load_document("report.PDF", directory.path()).unwrap();
        assert!(document.is_binary());
        assert_eq!(document.media_type, "application/pdf");
        match document.content() {
            GaiseContent::File { data, name } => {
                assert_eq!(data, b"%PDF-1.7\nbody");
                assert_eq!(name.as_deref(), Some("report.PDF"));
            }
            other => panic!("expected file content, got {other:?}"),
        }

        write(&directory.path().join("fake.pdf"), b"not really a pdf");
        assert!(load_document("fake.pdf", directory.path())
            .unwrap_err()
            .contains("valid PDF document signature"));

        write(&directory.path().join("brief.docx"), b"PK\x03\x04zip");
        assert!(load_document("brief.docx", directory.path()).is_ok());
        write(
            &directory.path().join("legacy.doc"),
            &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0],
        );
        assert!(load_document("legacy.doc", directory.path()).is_ok());
    }

    #[test]
    fn rejects_empty_oversized_images_and_unknown_binary() {
        let directory = tempfile::tempdir().unwrap();
        write(&directory.path().join("empty.txt"), b"");
        assert!(load_document("empty.txt", directory.path())
            .unwrap_err()
            .contains("empty"));

        let mut huge = b"text".to_vec();
        huge.resize(MAX_DOCUMENT_BYTES + 1, b'x');
        write(&directory.path().join("huge.txt"), &huge);
        assert!(load_document("huge.txt", directory.path())
            .unwrap_err()
            .contains("too large"));

        write(
            &directory.path().join("picture.png"),
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
        );
        assert!(load_document("picture.png", directory.path())
            .unwrap_err()
            .contains("is an image"));

        write(
            &directory.path().join("corrupt.png"),
            b"not really an image",
        );
        assert!(load_document("corrupt.png", directory.path())
            .unwrap_err()
            .contains("image extension"));

        write(&directory.path().join("blob.bin"), &[0, 159, 146, 150]);
        assert!(load_document("blob.bin", directory.path())
            .unwrap_err()
            .contains("not a supported document"));
    }

    #[test]
    fn provider_capability_matrix_is_explicit_and_text_is_universal() {
        let text = LoadedDocument {
            label: "a.txt".to_string(),
            name: "a.txt".to_string(),
            media_type: "text/plain".to_string(),
            data: b"hello".to_vec(),
            kind: DocumentKind::Text,
        };
        assert!(text.support_error("ollama::llama3").is_none());

        let pdf = LoadedDocument {
            label: "a.pdf".to_string(),
            name: "a.pdf".to_string(),
            media_type: "application/pdf".to_string(),
            data: b"%PDF-".to_vec(),
            kind: DocumentKind::Binary,
        };
        for model in [
            "openai::gpt-5.4",
            "anthropic::claude-sonnet-4-5",
            "gemini::gemini-2.5-pro",
            "vertexai::gemini-2.5-pro",
            "bedrock::anthropic.claude-sonnet-4-5",
        ] {
            assert!(pdf.support_error(model).is_none(), "{model}");
        }
        assert!(pdf
            .support_error("ollama::llama3")
            .unwrap()
            .contains("cannot send PDF documents"));

        let docx = LoadedDocument {
            name: "brief.docx".to_string(),
            label: "brief.docx".to_string(),
            media_type: file_media_type(Some("brief.docx")).to_string(),
            data: b"PK\x03\x04".to_vec(),
            kind: DocumentKind::Binary,
        };
        assert!(docx.support_error("openai::gpt-5.4").is_none());
        assert!(docx.support_error("bedrock::nova-pro").is_none());
        assert!(docx.support_error("anthropic::claude-opus-4").is_some());
    }

    #[test]
    fn extracts_dragged_documents_without_consuming_normal_source_references() {
        let directory = tempfile::tempdir().unwrap();
        write(&directory.path().join("my report.pdf"), b"%PDF-1.7\n");
        std::fs::create_dir_all(directory.path().join("src")).unwrap();
        write(&directory.path().join("src/main.rs"), b"fn main() {}");

        let absolute = directory.path().join("my report.pdf");
        let prompt = format!(
            "summarize \"{}\" but compare src/main.rs",
            absolute.display()
        );
        let (text, documents) = extract_inline_documents(&prompt, directory.path());
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].name, "my report.pdf");
        assert!(!text.contains("my report.pdf"));
        assert!(text.contains("src/main.rs"));

        let untouched = "please edit src/main.rs";
        let (text, documents) = extract_inline_documents(untouched, directory.path());
        assert_eq!(text, untouched);
        assert!(documents.is_empty());

        let quoted_source = "please edit \"src/main.rs\"";
        let (text, documents) = extract_inline_documents(quoted_source, directory.path());
        assert_eq!(text, quoted_source);
        assert!(documents.is_empty());
    }

    #[test]
    fn extracts_bare_relative_pdf_and_preserves_missing_candidates() {
        let directory = tempfile::tempdir().unwrap();
        write(&directory.path().join("report.pdf"), b"%PDF-1.4\n");
        let (text, documents) =
            extract_inline_documents("review report.pdf please", directory.path());
        assert_eq!(documents.len(), 1);
        assert_eq!(text, "review  please".trim());

        let prompt = "review missing.pdf please";
        let (text, documents) = extract_inline_documents(prompt, directory.path());
        assert_eq!(text, prompt);
        assert!(documents.is_empty());
    }
}
