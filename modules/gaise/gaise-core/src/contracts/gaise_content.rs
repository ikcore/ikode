use serde::{Deserialize, Serialize};
use std::path::Path;

/// Best-effort MIME type inference for [`GaiseContent::File`].
///
/// The provider-neutral file block stores a filename rather than a mandatory MIME
/// type, so adapters use this shared mapping instead of guessing independently.
pub fn file_media_type(name: Option<&str>) -> &'static str {
    let extension = name
        .and_then(|name| Path::new(name).extension())
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();

    if extension.eq_ignore_ascii_case("pdf") {
        "application/pdf"
    } else if extension.eq_ignore_ascii_case("txt") || extension.eq_ignore_ascii_case("log") {
        "text/plain"
    } else if extension.eq_ignore_ascii_case("md")
        || extension.eq_ignore_ascii_case("markdown")
    {
        "text/markdown"
    } else if extension.eq_ignore_ascii_case("csv") {
        "text/csv"
    } else if extension.eq_ignore_ascii_case("tsv") {
        "text/tab-separated-values"
    } else if extension.eq_ignore_ascii_case("json") || extension.eq_ignore_ascii_case("jsonl") {
        "application/json"
    } else if extension.eq_ignore_ascii_case("html") || extension.eq_ignore_ascii_case("htm") {
        "text/html"
    } else if extension.eq_ignore_ascii_case("xml") {
        "application/xml"
    } else if extension.eq_ignore_ascii_case("yaml") || extension.eq_ignore_ascii_case("yml") {
        "application/yaml"
    } else if extension.eq_ignore_ascii_case("toml") {
        "application/toml"
    } else if extension.eq_ignore_ascii_case("doc") {
        "application/msword"
    } else if extension.eq_ignore_ascii_case("docx") {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    } else if extension.eq_ignore_ascii_case("rtf") {
        "application/rtf"
    } else if extension.eq_ignore_ascii_case("odt") {
        "application/vnd.oasis.opendocument.text"
    } else if extension.eq_ignore_ascii_case("ppt") {
        "application/vnd.ms-powerpoint"
    } else if extension.eq_ignore_ascii_case("pptx") {
        "application/vnd.openxmlformats-officedocument.presentationml.presentation"
    } else if extension.eq_ignore_ascii_case("xls") {
        "application/vnd.ms-excel"
    } else if extension.eq_ignore_ascii_case("xlsx") {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    } else {
        "application/octet-stream"
    }
}

/// Normalize the common shorthand accepted by callers to an image MIME type.
pub fn image_media_type(format: Option<&str>) -> String {
    match format.unwrap_or_default().trim().to_ascii_lowercase().as_str() {
        "png" | "image/png" => "image/png".to_string(),
        "jpg" | "jpeg" | "image/jpg" | "image/jpeg" => "image/jpeg".to_string(),
        "gif" | "image/gif" => "image/gif".to_string(),
        "webp" | "image/webp" => "image/webp".to_string(),
        "bmp" | "image/bmp" => "image/bmp".to_string(),
        value if value.starts_with("image/") => value.to_string(),
        _ => "image/jpeg".to_string(),
    }
}

/// Normalize the common shorthand accepted by callers to an audio MIME type.
pub fn audio_media_type(format: Option<&str>) -> String {
    match format.unwrap_or_default().trim().to_ascii_lowercase().as_str() {
        "mp3" | "mpeg" | "audio/mp3" | "audio/mpeg" => "audio/mpeg".to_string(),
        "wav" | "wave" | "audio/wav" | "audio/wave" => "audio/wav".to_string(),
        "m4a" | "mp4" | "audio/m4a" | "audio/mp4" => "audio/mp4".to_string(),
        "ogg" | "oga" | "audio/ogg" => "audio/ogg".to_string(),
        "flac" | "audio/flac" => "audio/flac".to_string(),
        "webm" | "audio/webm" => "audio/webm".to_string(),
        value if value.starts_with("audio/") => value.to_string(),
        _ => "audio/mpeg".to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum GaiseContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "audio")]
    Audio {
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
        format: Option<String>,
    },
    #[serde(rename = "image")]
    Image {
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
        format: Option<String>,
    },
    #[serde(rename = "file")]
    File {
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
        name: Option<String>,
    },
    /// Provider-returned reasoning or thought summary. Providers may omit this
    /// entirely, redact it, or require `signature` to be echoed on later turns.
    #[serde(rename = "reasoning")]
    Reasoning {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    /// Opaque provider-encrypted reasoning that must be replayed byte-for-byte.
    /// Anthropic and Bedrock can return this when reasoning is safety-redacted.
    #[serde(rename = "redacted_reasoning")]
    RedactedReasoning {
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    #[serde(rename = "parts")]
    Parts { parts: Vec<GaiseContent> },
}

impl Default for GaiseContent {
    fn default() -> Self {
        GaiseContent::Text {
            text: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{audio_media_type, file_media_type, image_media_type};

    #[test]
    fn infers_common_document_media_types_case_insensitively() {
        assert_eq!(file_media_type(Some("report.PDF")), "application/pdf");
        assert_eq!(file_media_type(Some("notes.md")), "text/markdown");
        assert_eq!(file_media_type(Some("table.csv")), "text/csv");
        assert_eq!(
            file_media_type(Some("brief.DOCX")),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert_eq!(
            file_media_type(Some("sheet.xlsx")),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        );
    }

    #[test]
    fn unknown_or_missing_extensions_are_binary() {
        assert_eq!(file_media_type(Some("archive.bin")), "application/octet-stream");
        assert_eq!(file_media_type(Some("README")), "application/octet-stream");
        assert_eq!(file_media_type(None), "application/octet-stream");
    }

    #[test]
    fn normalizes_media_type_shorthand() {
        assert_eq!(image_media_type(Some("PNG")), "image/png");
        assert_eq!(image_media_type(Some("image/webp")), "image/webp");
        assert_eq!(image_media_type(None), "image/jpeg");
        assert_eq!(audio_media_type(Some("mp3")), "audio/mpeg");
        assert_eq!(audio_media_type(Some("audio/wav")), "audio/wav");
    }
}
