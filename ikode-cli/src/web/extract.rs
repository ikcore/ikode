//! Pure, dependency-free HTML → readable text extraction for the web tools.
//!
//! This is deliberately NOT a readability engine: a single tag-stripping pass
//! that drops non-content subtrees (`script`/`style`/`head`/`nav`/…), decodes
//! the common entities, and preserves paragraph structure via block-tag line
//! breaks. Good enough for an agent that summarises; zero crates.
//!
//! Two extras cover the modern-web gap without executing JavaScript:
//! - [`Extracted::js_shell`] detects "empty shell" SPA pages so the caller can
//!   fall back (search snippets, or a headless-Chrome render when available).
//! - [`harvest_hydration_json`] pulls human-readable strings out of embedded
//!   SSR payloads (`__NEXT_DATA__`, `application/ld+json`), which often contain
//!   the whole article the un-rendered DOM lacks.

/// The readable form of one HTML document.
pub struct Extracted {
    pub title: Option<String>,
    pub text: String,
    /// True when the page looks like a JavaScript application shell: almost no
    /// extractable text plus a known SPA marker (root div, hydration blob, or a
    /// `<noscript>` telling the user to enable JavaScript).
    pub js_shell: bool,
}

/// Subtrees that never contribute readable text.
const SKIP_SUBTREES: &[&str] = &[
    "script", "style", "noscript", "svg", "template", "head", "iframe", "canvas",
];

/// Tags that imply a line break around them, preserving paragraph shape.
const BLOCK_TAGS: &[&str] = &[
    "p", "div", "br", "li", "ul", "ol", "h1", "h2", "h3", "h4", "h5", "h6", "tr", "table",
    "section", "article", "header", "footer", "nav", "aside", "blockquote", "pre", "hr", "dt",
    "dd", "figcaption", "main",
];

/// Markers whose presence (with an otherwise empty page) identifies an SPA shell.
const SHELL_MARKERS: &[&str] = &[
    "__next_data__",
    "__nuxt__",
    "data-reactroot",
    "ng-version=",
    "id=\"root\"",
    "id='root'",
    "id=\"app\"",
    "id='app'",
    "id=\"__next\"",
];

/// Extract the readable text (and title) from `html`.
pub fn extract(html: &str) -> Extracted {
    let title = extract_title(html);
    let mut text = String::with_capacity(html.len() / 8);
    let mut noscript = String::new();
    let bytes = html.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] == b'<' {
            if html[i..].starts_with("<!--") {
                i = match html[i + 4..].find("-->") {
                    Some(end) => i + 4 + end + 3,
                    None => bytes.len(),
                };
                continue;
            }
            let (name, closing, tag_end) = parse_tag(html, i);
            let lower = name.to_ascii_lowercase();
            if !closing && SKIP_SUBTREES.contains(&lower.as_str()) {
                let close = format!("</{lower}");
                let rest = &html[tag_end..];
                let skip_to = find_ci(rest, &close)
                    .map(|at| {
                        let after = tag_end + at;
                        html[after..]
                            .find('>')
                            .map(|gt| after + gt + 1)
                            .unwrap_or(bytes.len())
                    })
                    .unwrap_or(bytes.len());
                if lower == "noscript" {
                    let inner_end = find_ci(rest, &close).map(|at| tag_end + at).unwrap_or(skip_to);
                    noscript.push_str(&strip_tags_flat(&html[tag_end..inner_end]));
                }
                i = skip_to;
                continue;
            }
            if BLOCK_TAGS.contains(&lower.as_str()) {
                text.push('\n');
            }
            i = tag_end;
            continue;
        }
        let (ch, next) = decode_entity(html, i);
        text.push(ch);
        i = next;
    }

    let text = collapse_whitespace(&text);
    let js_shell = looks_like_shell(html, &text, &noscript);
    Extracted {
        title,
        text,
        js_shell,
    }
}

/// `<title>` lives inside `<head>` (a skipped subtree), so it is pulled out
/// separately before the main pass.
fn extract_title(html: &str) -> Option<String> {
    let at = find_ci(html, "<title")?;
    let open_end = html[at..].find('>').map(|gt| at + gt + 1)?;
    let close = find_ci(&html[open_end..], "</title").map(|end| open_end + end)?;
    let title = collapse_whitespace(&strip_tags_flat(&html[open_end..close]))
        .replace('\n', " ")
        .trim()
        .to_string();
    (!title.is_empty()).then_some(title)
}

/// Read a tag at `html[at..]` (which starts with `<`): returns the tag name, a
/// closing-tag flag, and the byte offset just past the tag's `>`. Quoted
/// attribute values may contain `>` and are honoured.
fn parse_tag(html: &str, at: usize) -> (String, bool, usize) {
    let rest = &html[at + 1..];
    let closing = rest.starts_with('/');
    let name: String = rest
        .trim_start_matches('/')
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    let mut quote: Option<char> = None;
    for (offset, ch) in html[at..].char_indices() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch == '>' => return (name, closing, at + offset + 1),
            None => {}
        }
    }
    (name, closing, html.len())
}

/// Decode the entity starting at `html[at..]` when it is one we recognise;
/// otherwise return the literal character. Returns the decoded char and the
/// index of the next unread byte.
fn decode_entity(html: &str, at: usize) -> (char, usize) {
    let bytes = html.as_bytes();
    if bytes[at] != b'&' {
        // Guaranteed char boundary: caller iterates byte-wise but only calls
        // here on ASCII '<'/'&' checks failing, so step over one full char.
        let ch = html[at..].chars().next().unwrap_or(' ');
        return (ch, at + ch.len_utf8());
    }
    let end = html[at + 1..]
        .char_indices()
        .take(12)
        .find(|(_, c)| *c == ';')
        .map(|(idx, _)| at + 1 + idx);
    let Some(end) = end else {
        return ('&', at + 1);
    };
    let entity = &html[at + 1..end];
    let decoded = match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" | "#39" => Some('\''),
        "nbsp" => Some(' '),
        "mdash" => Some('—'),
        "ndash" => Some('–'),
        "hellip" => Some('…'),
        "copy" => Some('©'),
        _ => entity
            .strip_prefix("#x")
            .or_else(|| entity.strip_prefix("#X"))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .or_else(|| entity.strip_prefix('#').and_then(|dec| dec.parse().ok()))
            .and_then(char::from_u32),
    };
    match decoded {
        Some(ch) => (ch, end + 1),
        None => ('&', at + 1),
    }
}

/// Remove tags without any subtree/block logic — for small inner fragments
/// (titles, noscript bodies).
fn strip_tags_flat(fragment: &str) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    let bytes = fragment.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let (_, _, end) = parse_tag(fragment, i);
            i = end;
            continue;
        }
        let (ch, next) = decode_entity(fragment, i);
        out.push(ch);
        i = next;
    }
    out
}

/// Trim every line, drop runs of blank lines, and collapse intra-line spacing.
fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_run = true; // suppress leading blanks
    for line in text.lines() {
        let compact = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if compact.is_empty() {
            if !blank_run {
                out.push('\n');
                blank_run = true;
            }
        } else {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&compact);
            blank_run = false;
        }
    }
    out.trim().to_string()
}

fn looks_like_shell(html: &str, text: &str, noscript: &str) -> bool {
    if text.chars().count() >= 200 {
        return false;
    }
    if noscript.to_ascii_lowercase().contains("javascript") {
        return true;
    }
    let lower = html.to_ascii_lowercase();
    SHELL_MARKERS.iter().any(|marker| lower.contains(marker))
}

/// Case-insensitive substring find (ASCII).
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let needle = needle.to_ascii_lowercase();
    haystack
        .as_bytes()
        .windows(needle.len().max(1))
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Pull readable strings out of embedded SSR/hydration payloads: `__NEXT_DATA__`
/// script blobs and `application/ld+json` blocks. Returns prose-like string leaves
/// (≥ `min_len` chars), deduplicated in document order, joined by newlines —
/// or `None` when the page embeds nothing useful. This recovers content from many
/// "JavaScript required" pages without executing anything.
pub fn harvest_hydration_json(html: &str, min_len: usize, max_chars: usize) -> Option<String> {
    let mut collected: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut cursor = 0usize;
    let mut blobs = 0usize;
    while let Some(at) = find_ci(&html[cursor..], "<script") {
        let start = cursor + at;
        let open_end = match html[start..].find('>') {
            Some(gt) => start + gt + 1,
            None => break,
        };
        let attrs = html[start..open_end].to_ascii_lowercase();
        let close = match find_ci(&html[open_end..], "</script") {
            Some(end) => open_end + end,
            None => break,
        };
        cursor = close + 1;
        let interesting =
            attrs.contains("__next_data__") || attrs.contains("application/ld+json");
        if !interesting {
            continue;
        }
        blobs += 1;
        if blobs > 5 {
            break;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(html[open_end..close].trim())
        {
            collect_strings(&value, min_len, &mut collected, &mut seen);
        }
    }
    if collected.is_empty() {
        return None;
    }
    let mut joined = collected.join("\n");
    if joined.chars().count() > max_chars {
        joined = joined.chars().take(max_chars).collect();
    }
    Some(joined)
}

fn collect_strings(
    value: &serde_json::Value,
    min_len: usize,
    out: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    match value {
        serde_json::Value::String(s) => {
            let s = s.trim();
            // Prose, not identifiers/URLs/paths: require some length and a space.
            if s.chars().count() >= min_len
                && s.contains(' ')
                && !s.starts_with("http")
                && seen.insert(s.to_string())
            {
                out.push(s.to_string());
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_strings(item, min_len, out, seen);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values() {
                collect_strings(item, min_len, out, seen);
            }
        }
        _ => {}
    }
}

/// Cap `text` at `max_chars` characters; the flag reports whether it was cut.
pub fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }
    (text.chars().take(max_chars).collect(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_scripts_styles_and_head_but_keeps_title_and_body_text() {
        let html = r#"<html><head><title>My &amp; Page</title><style>body{color:red}</style>
            <script>var x = "<p>not text</p>";</script></head>
            <body><nav>skip-me-not</nav><h1>Heading</h1>
            <p>First paragraph with <b>bold</b> text.</p>
            <p>Second &lt;escaped&gt; &#65; &#x42;</p></body></html>"#;
        let extracted = extract(html);
        assert_eq!(extracted.title.as_deref(), Some("My & Page"));
        assert!(!extracted.text.contains("color:red"));
        assert!(!extracted.text.contains("not text"));
        assert!(extracted.text.contains("Heading"));
        assert!(extracted.text.contains("First paragraph with bold text."));
        assert!(extracted.text.contains("Second <escaped> A B"));
        // nav is a block tag, not a skipped subtree — its text survives on its own line.
        assert!(extracted.text.contains("skip-me-not"));
        assert!(!extracted.js_shell);
    }

    #[test]
    fn block_tags_produce_line_breaks_and_whitespace_collapses() {
        let html = "<div>one</div><div>two</div><p>three\n\n\n</p><br>four";
        let extracted = extract(html);
        assert_eq!(extracted.text, "one\ntwo\nthree\nfour");
    }

    #[test]
    fn quoted_gt_inside_attributes_does_not_end_the_tag() {
        let html = r#"<a href="/x?a>b" title='1>2'>link text</a>"#;
        assert_eq!(extract(html).text, "link text");
    }

    #[test]
    fn detects_spa_shells_via_markers_and_noscript() {
        let react = r#"<html><body><div id="root"></div><script src="/app.js"></script></body></html>"#;
        assert!(extract(react).js_shell);
        let noscript =
            "<html><body><noscript>Please enable JavaScript to run this app.</noscript><div></div></body></html>";
        assert!(extract(noscript).js_shell);
        let long = format!(
            "<html><body><div id=\"root\">{}</div></body></html>",
            "real server rendered words here ".repeat(20)
        );
        assert!(!extract(&long).js_shell, "plenty of text is not a shell");
    }

    #[test]
    fn harvests_prose_from_next_data_and_ld_json() {
        let html = r#"<div id="root"></div>
        <script id="__NEXT_DATA__" type="application/json">
          {"props":{"pageProps":{"article":{"headline":"A headline that is long enough to keep",
            "slug":"short","body":"The full body text of the article, also comfortably long."}}}}
        </script>
        <script type="application/ld+json">{"@type":"Article","description":"Structured description with several words."}</script>"#;
        let harvested = harvest_hydration_json(html, 30, 4000).unwrap();
        assert!(harvested.contains("A headline that is long enough to keep"));
        assert!(harvested.contains("full body text"));
        assert!(harvested.contains("Structured description"));
        assert!(!harvested.contains("short"));
        assert_eq!(harvest_hydration_json("<p>no scripts</p>", 30, 4000), None);
    }

    #[test]
    fn truncate_chars_flags_only_real_cuts() {
        assert_eq!(truncate_chars("abc", 5), ("abc".to_string(), false));
        assert_eq!(truncate_chars("abcdef", 3), ("abc".to_string(), true));
    }
}
