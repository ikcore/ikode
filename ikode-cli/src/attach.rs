//! Inline editor attachments — collapsed pastes today, designed to grow.
//!
//! A large or multi-line paste should not be spilled character-by-character into
//! the edit buffer: it floods the framed prompt and is a nuisance to delete. The
//! `console`-style fix Claude uses is to keep the pasted text *whole* off to one
//! side and leave a single, tidy `[Pasted N lines]` placeholder in its place.
//!
//! This module is the storage + rendering model for that. The edit buffer stays a
//! flat `Vec<char>`; each attachment is represented in it by **one** sentinel
//! `char` drawn from the Unicode Private Use Area. Because a sentinel is a single
//! `char`, it is automatically a single cursor stop and a single Backspace — a
//! paste therefore deletes atomically — and several pastes simply get several
//! distinct sentinels that coexist independently. On submit the buffer is
//! [`expand`]ed: every sentinel is swapped back for its real text.
//!
//! It is deliberately **source-agnostic**. Pasted text is the first kind of
//! [`Attachment`]; an image (drag-drop, or a future clipboard-image paste) is just
//! another variant — the editor only ever touches a sentinel `char`, the
//! placeholder label, and `expand`, so adding `Attachment::Image { .. }` slots in
//! without the editor learning anything new.

use std::collections::HashMap;

use crate::image::LoadedImage;

/// First code point of the Private Use Area block handed out as sentinels. A
/// buffer `char` in `SENTINEL_BASE..=SENTINEL_END` denotes a stored attachment.
const SENTINEL_BASE: u32 = 0xE000;
/// Last code point we will hand out (the BMP PUA runs to U+F8FF). Far more
/// attachments than a single prompt could ever hold; we wrap if somehow exceeded.
const SENTINEL_END: u32 = 0xF8FF;

/// Collapse a single-line paste into a placeholder once it reaches this many
/// characters. Below it (and single-line) a paste is inserted literally, so
/// pasting a word, path, or short snippet behaves exactly like typing it.
pub const COLLAPSE_MIN_CHARS: usize = 200;

/// One inline attachment kept out of the edit buffer behind a sentinel `char`.
///
/// Extend with new variants (e.g. `Image { mime: String, bytes: Vec<u8> }`) as
/// other attachment sources appear; only [`Attachment::placeholder`] and
/// [`Attachment::expand`] need a new arm — the editor is unaffected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Attachment {
    /// A chunk of pasted text, held whole instead of inserted char-by-char.
    Pasted(String),
    /// An image (e.g. pasted from the clipboard), carried whole behind the sentinel.
    /// Its bytes are sent as a separate content block, not as expanded text — see
    /// [`expand`] (empty) and [`collect_images`].
    Image(LoadedImage),
}

impl Attachment {
    /// The short label shown in the buffer in place of the attachment, e.g.
    /// `"[Pasted 1423 lines]"` or `"[Image clipboard.png]"`.
    pub fn placeholder(&self) -> String {
        match self {
            Attachment::Pasted(text) => {
                let lines = line_count(text);
                if lines <= 1 {
                    "[Pasted text]".to_string()
                } else {
                    format!("[Pasted {lines} lines]")
                }
            }
            // The label carries "name (type, size)"; show just the name in the cell.
            Attachment::Image(img) => {
                let name = img.label.split(" (").next().unwrap_or(&img.label);
                format!("[Image {name}]")
            }
        }
    }

    /// The real text this attachment expands to when the line is submitted. An image
    /// contributes no text — its bytes travel as a separate content block — so it
    /// expands to the empty string and is gathered by [`collect_images`] instead.
    pub fn expand(&self) -> String {
        match self {
            Attachment::Pasted(text) => text.clone(),
            Attachment::Image(_) => String::new(),
        }
    }
}

/// Maps sentinel `char`s to the attachments they stand in for, scoped to one
/// edit of one line. Holds insertion-ordered ids so a deleted attachment frees
/// its sentinel and a fresh paste never collides with a live one.
#[derive(Default)]
pub struct AttachmentStore {
    items: HashMap<char, Attachment>,
    /// Offset of the next sentinel to hand out, relative to [`SENTINEL_BASE`].
    next: u32,
}

impl AttachmentStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Store `attachment` and return the sentinel `char` that represents it in
    /// the buffer. Wraps within the PUA block in the impossible event of running
    /// out, overwriting the oldest slot rather than panicking.
    pub fn intern(&mut self, attachment: Attachment) -> char {
        let span = SENTINEL_END - SENTINEL_BASE + 1;
        let code = SENTINEL_BASE + (self.next % span);
        self.next = self.next.wrapping_add(1);
        // Code points in this PUA range are always valid scalar values.
        let sentinel = char::from_u32(code).expect("PUA code point is a valid char");
        self.items.insert(sentinel, attachment);
        sentinel
    }

    /// The attachment behind a sentinel, if any.
    pub fn get(&self, sentinel: char) -> Option<&Attachment> {
        self.items.get(&sentinel)
    }

    /// Drop the attachment behind a sentinel (called when its char is deleted),
    /// returning it if present.
    pub fn remove(&mut self, sentinel: char) -> Option<Attachment> {
        self.items.remove(&sentinel)
    }

    /// Forget every attachment (e.g. when the line is cleared).
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Display width, in terminal columns, of a buffer `char`: the placeholder's
    /// length for a known sentinel, otherwise 1. Used by the editor's cursor math
    /// so a collapsed paste occupies its visible width.
    pub fn cell_width(&self, c: char) -> usize {
        match self.get(c) {
            Some(a) => a.placeholder().chars().count().max(1),
            None => 1,
        }
    }
}

/// Whether `c` falls in the sentinel range (regardless of whether a store knows
/// it). Lets the editor recognise an attachment cell without a store in hand.
pub fn is_sentinel(c: char) -> bool {
    let code = c as u32;
    (SENTINEL_BASE..=SENTINEL_END).contains(&code)
}

/// Number of lines in `text`, counting at least one. A lone trailing newline
/// does not add a phantom line (so `"a\nb"` and `"a\nb\n"` are both 2).
pub fn line_count(text: &str) -> usize {
    text.lines().count().max(1)
}

/// Clean a pasted burst for storage or inline insertion: normalise CRLF/CR to
/// LF, then drop control characters other than newline and tab. Terminals can
/// inject stray controls into a paste (ESC, NUL, bell, vertical tab, …); keeping
/// them would render oddly and, on submit, be sent verbatim to the model — a
/// plausible way to upset an API backend. Printable text, newlines and tabs are
/// preserved exactly.
pub fn sanitize_paste(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .map(|c| if c == '\r' { '\n' } else { c })
        .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
        .collect()
}

/// Whether a paste should collapse to a placeholder rather than insert inline:
/// when it spans more than one line, or a single line exceeds
/// [`COLLAPSE_MIN_CHARS`]. Short single-line pastes stay literal.
pub fn should_collapse(text: &str) -> bool {
    line_count(text) > 1 || text.chars().count() > COLLAPSE_MIN_CHARS
}

/// Rebuild the real submitted string from an edit buffer: every sentinel is
/// replaced by its attachment's expansion (an unknown sentinel — e.g. a stale
/// cell — contributes nothing), every other char is kept as-is.
pub fn expand(buf: &[char], store: &AttachmentStore) -> String {
    let mut out = String::new();
    for &c in buf {
        if let Some(a) = store.get(c) {
            out.push_str(&a.expand());
        } else if !is_sentinel(c) {
            // A normal char passes through; an unknown sentinel (e.g. a stale cell
            // whose attachment was freed) contributes nothing — never leak the raw
            // Private-Use code point into the submitted text.
            out.push(c);
        }
    }
    out
}

/// Gather the image attachments in `buf`, in buffer order, by following each image
/// sentinel to its stored [`LoadedImage`]. Pairs with [`expand`]: text comes from
/// `expand`, images from here, and together they form the multimodal user message.
/// An unknown/stale sentinel contributes nothing.
pub fn collect_images(buf: &[char], store: &AttachmentStore) -> Vec<LoadedImage> {
    buf.iter()
        .filter_map(|&c| match store.get(c) {
            Some(Attachment::Image(img)) => Some(img.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_image(name: &str) -> LoadedImage {
        LoadedImage {
            label: format!("{name} (image/png, 1 KB)"),
            media_type: "image/png".to_string(),
            data: vec![0x89, b'P', b'N', b'G'],
        }
    }

    #[test]
    fn image_placeholder_shows_name_and_expands_to_no_text() {
        let img = Attachment::Image(sample_image("clipboard.png"));
        assert_eq!(img.placeholder(), "[Image clipboard.png]");
        // An image contributes no text to the submitted string.
        assert_eq!(img.expand(), "");
    }

    #[test]
    fn collect_images_follows_sentinels_in_order() {
        let mut store = AttachmentStore::new();
        let a = store.intern(Attachment::Image(sample_image("a.png")));
        let p = store.intern(Attachment::Pasted("text".into()));
        let b = store.intern(Attachment::Image(sample_image("b.png")));
        let buf = vec!['x', a, 'y', p, b];
        let imgs = collect_images(&buf, &store);
        assert_eq!(imgs.len(), 2);
        assert_eq!(imgs[0].label, "a.png (image/png, 1 KB)");
        assert_eq!(imgs[1].label, "b.png (image/png, 1 KB)");
        // Text expansion ignores the images, keeps the pasted text and plain chars.
        assert_eq!(expand(&buf, &store), "xytext");
    }

    #[test]
    fn collapse_only_when_multiline_or_long() {
        // Short single-line paste: inserted literally, never collapsed.
        assert!(!should_collapse("a short path/to/file.rs"));
        // More than one line: collapse.
        assert!(should_collapse("line one\nline two"));
        // Long single line: collapse.
        assert!(should_collapse(&"x".repeat(COLLAPSE_MIN_CHARS + 1)));
        // Exactly at the threshold (single line) stays literal.
        assert!(!should_collapse(&"x".repeat(COLLAPSE_MIN_CHARS)));
        // Empty never collapses.
        assert!(!should_collapse(""));
    }

    #[test]
    fn sanitize_strips_controls_but_keeps_text_newlines_tabs() {
        // CRLF/CR fold to LF; newline and tab survive; ESC/NUL/bell are dropped.
        let dirty = "a\r\nb\tc\u{1b}[0m\u{0}\u{7}d";
        assert_eq!(sanitize_paste(dirty), "a\nb\tc[0md");
        // Plain printable text (e.g. the lorem-ipsum paste) is untouched.
        let clean = "Lorem ipsum is a dummy or placeholder text.";
        assert_eq!(sanitize_paste(clean), clean);
    }

    #[test]
    fn line_count_ignores_a_single_trailing_newline() {
        assert_eq!(line_count("abc"), 1);
        assert_eq!(line_count("a\nb"), 2);
        assert_eq!(line_count("a\nb\n"), 2);
        assert_eq!(line_count(""), 1);
    }

    #[test]
    fn placeholder_reads_naturally_for_one_or_many_lines() {
        assert_eq!(
            Attachment::Pasted("just one line".into()).placeholder(),
            "[Pasted text]"
        );
        let many = Attachment::Pasted("a\n".repeat(1423));
        assert_eq!(many.placeholder(), "[Pasted 1423 lines]");
    }

    #[test]
    fn sentinels_are_unique_and_recognised() {
        let mut store = AttachmentStore::new();
        let a = store.intern(Attachment::Pasted("first".into()));
        let b = store.intern(Attachment::Pasted("second".into()));
        assert_ne!(a, b, "each paste gets a distinct sentinel");
        assert!(is_sentinel(a) && is_sentinel(b));
        assert!(!is_sentinel('x'));
        assert_eq!(store.get(a).unwrap().expand(), "first");
        assert_eq!(store.get(b).unwrap().expand(), "second");
    }

    #[test]
    fn multiple_pastes_expand_in_buffer_order() {
        // Buffer: "see " <pasteA> " and " <pasteB> "!"
        let mut store = AttachmentStore::new();
        let a = store.intern(Attachment::Pasted("AAA\nAAA".into()));
        let b = store.intern(Attachment::Pasted("BBB".into()));
        let mut buf: Vec<char> = "see ".chars().collect();
        buf.push(a);
        buf.extend(" and ".chars());
        buf.push(b);
        buf.push('!');
        assert_eq!(expand(&buf, &store), "see AAA\nAAA and BBB!");
    }

    #[test]
    fn deleting_a_sentinel_frees_it_and_drops_from_expansion() {
        let mut store = AttachmentStore::new();
        let a = store.intern(Attachment::Pasted("gone".into()));
        let mut buf = vec!['x', a, 'y'];
        // Atomic delete: removing the single sentinel char removes the whole paste.
        let removed_at = buf.iter().position(|&c| c == a).unwrap();
        buf.remove(removed_at);
        store.remove(a);
        assert_eq!(expand(&buf, &store), "xy");
        // A now-unknown sentinel left in a buffer contributes nothing.
        assert_eq!(expand(&[a], &store), "");
    }

    #[test]
    fn cell_width_is_placeholder_len_for_sentinels_else_one() {
        let mut store = AttachmentStore::new();
        let a = store.intern(Attachment::Pasted("a\nb\nc".into()));
        assert_eq!(store.cell_width('z'), 1);
        assert_eq!(store.cell_width(a), "[Pasted 3 lines]".chars().count());
    }
}
