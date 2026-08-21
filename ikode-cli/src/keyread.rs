//! Cross-platform key reader for the interactive editor.
//!
//! Everywhere except Windows this is a thin pass-through to
//! [`console::Term::read_key`]. On Windows the `console` crate maps keys via
//! `ReadConsoleInputW` but **discards the modifier state** — so Shift+Tab,
//! Ctrl+Tab and a bare Tab all collapse to [`Key::Tab`], and `Key::BackTab` is
//! never produced (it only exists on the crate's Unix escape-sequence path).
//! That's why Shift+Tab mode-cycling silently does nothing in the VS Code
//! terminal / Windows Terminal / cmd, all of which feed input through ConPTY.
//!
//! To make Shift+Tab work faithfully we read the raw console records ourselves
//! on Windows: when a `VK_TAB` keydown carries `SHIFT_PRESSED` we surface
//! [`Key::BackTab`], otherwise we reproduce the same mapping the `console` crate
//! uses for `read_key` (no console-mode changes, so Ctrl+C keeps its current
//! signal behaviour). Every other platform is unaffected.
//!
//! Reading the raw records also lets us notice a `WINDOW_BUFFER_SIZE_EVENT` (the
//! user resized the terminal) and surface it as [`Key::Unknown`], which the
//! editor treats as a no-op keystroke — so its loop redraws the framed prompt at
//! the new width immediately instead of leaving a stale frame until the next key.

use std::io::{self, Write};

use console::{Key, Term};

/// One input event from the editor's point of view: an ordinary key, or a burst
/// of text delivered together (a paste). Batching consecutive characters into a
/// single [`Event::Paste`] lets the editor collapse a large paste into a tidy
/// placeholder instead of inserting thousands of chars one keystroke at a time
/// (see [`crate::attach`]). A lone keystroke is always an [`Event::Key`], so all
/// existing single-key behaviour (Enter submits, Ctrl+Enter newline, Shift+Tab,
/// …) is untouched.
pub enum Event {
    /// A single key press.
    Key(Key),
    /// Text that arrived as one uninterrupted burst — typically a clipboard paste.
    /// May contain `'\n'`/`'\t'`. The editor decides whether to collapse it.
    Paste(String),
}

/// Restores normal input handling when dropped. See [`full_key_fidelity`].
pub struct KeyFidelityGuard {
    /// Whether we actually enabled the mode (so drop only undoes what we did).
    enabled: bool,
}

impl Drop for KeyFidelityGuard {
    fn drop(&mut self) {
        if self.enabled {
            // Disable win32-input-mode so the terminal goes back to its normal
            // input encoding once we're done editing (e.g. during agent turns).
            print!("\x1b[?9001l");
            let _ = io::stdout().flush();
        }
    }
}

/// Request "win32-input-mode" on Windows so the connected terminal forwards
/// full-fidelity key events — most importantly the Ctrl modifier on Enter,
/// which ConPTY (Windows Terminal, VS Code, modern conhost) otherwise drops,
/// making Ctrl+Enter indistinguishable from a plain Enter.
///
/// With the mode on, `ReadConsoleInputW` sees `VK_RETURN` carrying
/// `LEFT_CTRL_PRESSED`, so the existing Ctrl+Enter → newline path fires. The
/// returned guard restores the previous behaviour on drop, so we only alter the
/// input stream while actively editing a line. No-op off Windows; terminals
/// that don't understand the private mode silently ignore the sequence.
#[cfg(windows)]
pub fn full_key_fidelity() -> KeyFidelityGuard {
    print!("\x1b[?9001h");
    let _ = io::stdout().flush();
    KeyFidelityGuard { enabled: true }
}

#[cfg(not(windows))]
pub fn full_key_fidelity() -> KeyFidelityGuard {
    KeyFidelityGuard { enabled: false }
}

/// Read one key, distinguishing Shift+Tab from Tab on Windows.
#[cfg(not(windows))]
pub fn read_key(term: &Term) -> io::Result<Key> {
    term.read_key()
}

/// Read one editor event. Off Windows we cannot cheaply peek the input queue to
/// batch a paste, so every read is a single [`Event::Key`]; a large paste still
/// works, it just inserts character-by-character as before. The Windows reader
/// (below) coalesces bursts into [`Event::Paste`].
#[cfg(not(windows))]
pub fn read_event(term: &Term) -> io::Result<Event> {
    Ok(Event::Key(read_key(term)?))
}

#[cfg(windows)]
use std::mem;
#[cfg(windows)]
use windows_sys::Win32::Foundation::HANDLE;
#[cfg(windows)]
use windows_sys::Win32::System::Console::{
    GetNumberOfConsoleInputEvents, GetStdHandle, ReadConsoleInputW, INPUT_RECORD, INPUT_RECORD_0,
    KEY_EVENT, KEY_EVENT_RECORD, STD_INPUT_HANDLE, WINDOW_BUFFER_SIZE_EVENT,
};

// Keys read ahead while probing for a paste burst but found *not* to be part of
// one (e.g. an arrow key queued right behind a character). They are emitted, in
// order, by the next `read_event` calls before any new console read — so a burst
// probe never drops or reorders a keystroke it had to look at.
#[cfg(windows)]
thread_local! {
    static PENDING: std::cell::RefCell<std::collections::VecDeque<Key>> =
        const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}

/// One console record we care about: a keydown, or a window-size change.
#[cfg(windows)]
enum Record {
    Key(KEY_EVENT_RECORD),
    Resize,
}

/// Read one input record (blocking), skipping anything that isn't a keydown or a
/// resize.
#[cfg(windows)]
fn read_record(handle: HANDLE) -> io::Result<Record> {
    loop {
        let mut record: INPUT_RECORD = unsafe { mem::zeroed() };
        let mut read: u32 = 0;
        if unsafe { ReadConsoleInputW(handle, &mut record, 1, &mut read) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if read != 1 {
            continue;
        }
        if record.EventType == WINDOW_BUFFER_SIZE_EVENT as u16 {
            return Ok(Record::Resize); // terminal resized — caller redraws
        }
        if record.EventType != KEY_EVENT as u16 {
            continue; // focus / mouse event — ignore
        }
        let key: KEY_EVENT_RECORD =
            unsafe { mem::transmute::<INPUT_RECORD_0, KEY_EVENT_RECORD>(record.Event) };
        if key.bKeyDown == 0 {
            continue; // key release — ignore
        }
        return Ok(Record::Key(key));
    }
}

/// Read the next keydown, treating a resize as the surfaced signal. Used where a
/// genuine key is required (e.g. the low half of a surrogate pair), so a resize
/// arriving mid-sequence is simply skipped rather than abandoning the sequence.
#[cfg(windows)]
fn read_key_record(handle: HANDLE) -> io::Result<KEY_EVENT_RECORD> {
    loop {
        if let Record::Key(k) = read_record(handle)? {
            return Ok(k);
        }
    }
}

/// Decode one record into an editor [`Key`] (blocking). A resize surfaces as
/// [`Key::Unknown`] so the editor redraws at the new width; a high surrogate is
/// completed from the following record.
#[cfg(windows)]
fn next_key(handle: HANDLE) -> io::Result<Key> {
    loop {
        let key = match read_record(handle)? {
            Record::Key(k) => k,
            Record::Resize => return Ok(Key::Unknown),
        };
        let unicode = unsafe { key.uChar.UnicodeChar };
        match decode_record(key.wVirtualKeyCode, key.dwControlKeyState, unicode) {
            Decoded::Emit(k) => return Ok(k),
            Decoded::Skip => continue,
            Decoded::HighSurrogate(high) => {
                let low = read_key_record(handle)?;
                let low = unsafe { low.uChar.UnicodeChar };
                match char::decode_utf16([high, low]).next() {
                    Some(Ok(c)) => return Ok(normalize_char(c)),
                    _ => continue,
                }
            }
        }
    }
}

/// How many input records are queued. Lets the burst probe read ahead *only*
/// while input is already waiting, so it never blocks on a key that isn't there.
#[cfg(windows)]
fn pending_events(handle: HANDLE) -> io::Result<u32> {
    let mut count: u32 = 0;
    if unsafe { GetNumberOfConsoleInputEvents(handle, &mut count) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(count)
}

/// Non-blocking sibling of [`next_key`]: returns the next decoded key only if one
/// is already queued, else `Ok(None)`. Every console read is gated on
/// [`pending_events`] first, so this can never block — the property that makes
/// burst-probing safe even after a lone keystroke.
#[cfg(windows)]
fn try_next_key(handle: HANDLE) -> io::Result<Option<Key>> {
    loop {
        if pending_events(handle)? == 0 {
            return Ok(None);
        }
        // A record is queued, so this single read will not block.
        let mut record: INPUT_RECORD = unsafe { mem::zeroed() };
        let mut read: u32 = 0;
        if unsafe { ReadConsoleInputW(handle, &mut record, 1, &mut read) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if read != 1 {
            return Ok(None);
        }
        if record.EventType == WINDOW_BUFFER_SIZE_EVENT as u16 {
            return Ok(Some(Key::Unknown));
        }
        if record.EventType != KEY_EVENT as u16 {
            continue; // skip non-key; loop re-checks the queue without blocking
        }
        let key: KEY_EVENT_RECORD =
            unsafe { mem::transmute::<INPUT_RECORD_0, KEY_EVENT_RECORD>(record.Event) };
        if key.bKeyDown == 0 {
            continue; // key release
        }
        let unicode = unsafe { key.uChar.UnicodeChar };
        match decode_record(key.wVirtualKeyCode, key.dwControlKeyState, unicode) {
            Decoded::Emit(k) => return Ok(Some(k)),
            Decoded::Skip => continue,
            Decoded::HighSurrogate(high) => {
                if pending_events(handle)? == 0 {
                    return Ok(None); // surrogate low half not here; give up the probe
                }
                let low = read_key_record(handle)?;
                let low = unsafe { low.uChar.UnicodeChar };
                match char::decode_utf16([high, low]).next() {
                    Some(Ok(c)) => return Ok(Some(normalize_char(c))),
                    _ => continue,
                }
            }
        }
    }
}

/// The character a key contributes to a paste burst, or `None` if it is a key
/// that cannot be part of one (Enter and Tab become `'\n'`/`'\t'`; other control
/// keys — Backspace, arrows, Escape, Ctrl+Enter's `'\n'` char, … — return `None`
/// so a lone press keeps its normal editor handling).
#[cfg(windows)]
fn key_to_text(key: &Key) -> Option<char> {
    match key {
        Key::Char(c) if !c.is_control() => Some(*c),
        Key::Enter => Some('\n'),
        Key::Tab => Some('\t'),
        _ => None,
    }
}

/// Read one key from the Windows console, recovering the Shift+Tab modifier the
/// `console` crate drops. Mirrors `console`'s own `read_single_key(false)` for
/// every other key so nothing else changes. Retained alongside [`read_event`] as
/// the single-key entry point (and to document the raw-record mapping).
#[cfg(windows)]
#[allow(dead_code)]
pub fn read_key(_term: &Term) -> io::Result<Key> {
    let handle: HANDLE = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    next_key(handle)
}

/// Read one editor event, coalescing a paste into a single [`Event::Paste`].
///
/// A real keystroke blocks for [`next_key`]. If it is a text key, we then probe
/// the queue *non-blocking* for more text already waiting behind it: a clipboard
/// paste lands as a whole block, so its characters are sitting in the queue and
/// get gathered into one string; ordinary typing has nothing queued, so the key
/// is returned unchanged. A non-text key encountered mid-probe is pushed back to
/// be emitted next, never lost. This is why a lone Escape (nothing queued) can't
/// hang the probe — `try_next_key` only reads what is already there.
#[cfg(windows)]
pub fn read_event(_term: &Term) -> io::Result<Event> {
    // Flush keys held back by an earlier burst probe before touching the console.
    if let Some(k) = PENDING.with(|p| p.borrow_mut().pop_front()) {
        return Ok(Event::Key(k));
    }

    let handle: HANDLE = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    let first = next_key(handle)?;

    // Only text can begin a burst; any other key is a lone press with its usual
    // meaning (Enter submits, Ctrl+Enter inserts a newline, arrows move, …).
    let first_ch = match key_to_text(&first) {
        Some(c) => c,
        None => return Ok(Event::Key(first)),
    };

    // Gather any text already queued behind it. `had_more` distinguishes a real
    // burst (≥2 chars → Paste) from a single keystroke (→ Key, unchanged).
    let mut text = String::new();
    let mut had_more = false;
    loop {
        match try_next_key(handle)? {
            None => break,
            Some(k) => match key_to_text(&k) {
                Some(ch) => {
                    if !had_more {
                        text.push(first_ch);
                        had_more = true;
                    }
                    text.push(ch);
                }
                None => {
                    // A real key (arrow, Enter-as-submit, …) sits behind the text:
                    // stop the burst here and emit this key on the next read.
                    PENDING.with(|p| p.borrow_mut().push_back(k));
                    break;
                }
            },
        }
    }

    if had_more {
        Ok(Event::Paste(text))
    } else {
        Ok(Event::Key(first))
    }
}

/// What a single keydown record decodes to, independent of any I/O.
#[cfg(windows)]
enum Decoded {
    /// A finished key the editor can act on.
    Emit(Key),
    /// Nothing the editor cares about (a bare modifier, an undecodable char) —
    /// the reader should loop for the next event.
    Skip,
    /// A UTF-16 high surrogate; the reader must read the low half to finish it.
    HighSurrogate(u16),
}

/// Pure classification of one keydown record into a [`Decoded`] outcome, given
/// the virtual-key code, control-key state, and UTF-16 char unit. This is the
/// testable heart of the Windows reader — it owns the Shift+Tab and Ctrl+Enter
/// recovery the `console` crate can't do, with no console handle in sight.
#[cfg(windows)]
fn decode_record(vk: u16, control_key_state: u32, unicode: u16) -> Decoded {
    use windows_sys::Win32::System::Console::{
        LEFT_CTRL_PRESSED, RIGHT_CTRL_PRESSED, SHIFT_PRESSED,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT,
        VK_TAB, VK_UP,
    };
    const CTRL_PRESSED: u32 = LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED;

    // The whole reason this module exists: Shift+Tab, which the library would
    // otherwise report as a plain Tab.
    if vk == VK_TAB && control_key_state & SHIFT_PRESSED != 0 {
        return Decoded::Emit(Key::BackTab);
    }

    // Ctrl+Enter inserts a literal newline (multi-line input) instead of
    // submitting. The console layer can't tell it from Enter, but the raw
    // control-key state can — once the terminal forwards it (see
    // `full_key_fidelity`) — so surface it as a newline char the editor inserts.
    // Plain Enter still submits.
    if vk == VK_RETURN && control_key_state & CTRL_PRESSED != 0 {
        return Decoded::Emit(Key::Char('\n'));
    }

    if unicode == 0 {
        // A non-character key. Map the ones the editor uses; for anything else
        // (e.g. a bare Shift/Ctrl/Alt press) skip so we never hand back a key the
        // editor would just ignore.
        return match vk {
            VK_LEFT => Decoded::Emit(Key::ArrowLeft),
            VK_RIGHT => Decoded::Emit(Key::ArrowRight),
            VK_UP => Decoded::Emit(Key::ArrowUp),
            VK_DOWN => Decoded::Emit(Key::ArrowDown),
            VK_RETURN => Decoded::Emit(Key::Enter),
            VK_ESCAPE => Decoded::Emit(Key::Escape),
            VK_BACK => Decoded::Emit(Key::Backspace),
            VK_TAB => Decoded::Emit(Key::Tab),
            VK_HOME => Decoded::Emit(Key::Home),
            VK_END => Decoded::Emit(Key::End),
            VK_DELETE => Decoded::Emit(Key::Del),
            _ => Decoded::Skip,
        };
    }

    // A character, in UTF-16. A high surrogate is only half a code point.
    if (0xD800..=0xDBFF).contains(&unicode) {
        return Decoded::HighSurrogate(unicode);
    }
    match char::decode_utf16([unicode]).next() {
        Some(Ok(c)) => Decoded::Emit(normalize_char(c)),
        _ => Decoded::Skip,
    }
}

/// Normalise a decoded character the way the `console` crate does, so control
/// characters map to their named keys rather than literal chars.
#[cfg(windows)]
fn normalize_char(ch: char) -> Key {
    match ch {
        '\r' => Key::Enter,
        '\t' => Key::Tab,
        '\u{8}' => Key::Backspace,
        '\u{1b}' => Key::Escape,
        c => Key::Char(c),
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Console::{
        LEFT_CTRL_PRESSED, RIGHT_CTRL_PRESSED, SHIFT_PRESSED,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_RETURN, VK_TAB};

    /// Collapse a [`Decoded`] to a comparable shape (console::Key isn't always
    /// convenient to match, and we never expect a surrogate in these cases).
    fn emit(d: Decoded) -> Option<Key> {
        match d {
            Decoded::Emit(k) => Some(k),
            _ => None,
        }
    }

    #[test]
    fn ctrl_enter_becomes_a_newline_char() {
        // Ctrl+Enter, whichever Ctrl, with the usual CR char unit — must decode
        // to a literal '\n' the editor inserts, not a submit.
        for ctrl in [LEFT_CTRL_PRESSED, RIGHT_CTRL_PRESSED] {
            assert_eq!(
                emit(decode_record(VK_RETURN, ctrl, 0x0D)),
                Some(Key::Char('\n')),
                "ctrl=0x{ctrl:08X}"
            );
        }
    }

    #[test]
    fn plain_enter_still_submits() {
        // No modifier: Enter stays Enter (the editor submits on this).
        assert_eq!(emit(decode_record(VK_RETURN, 0, 0x0D)), Some(Key::Enter));
        // And via the char path (uChar=CR, vk irrelevant) it normalises to Enter.
        assert_eq!(emit(decode_record(0, 0, 0x0D)), Some(Key::Enter));
    }

    #[test]
    fn ctrl_enter_with_lf_char_is_also_a_newline() {
        // Some terminals deliver Ctrl+Enter as an LF char unit; that path also
        // yields a newline (here even without the ctrl bit, via normalisation).
        assert_eq!(
            emit(decode_record(VK_RETURN, 0, 0x0A)),
            Some(Key::Char('\n'))
        );
    }

    #[test]
    fn shift_tab_is_backtab() {
        assert_eq!(
            emit(decode_record(VK_TAB, SHIFT_PRESSED, 0)),
            Some(Key::BackTab)
        );
    }

    #[test]
    fn ordinary_char_passes_through() {
        assert_eq!(emit(decode_record(0, 0, 'a' as u16)), Some(Key::Char('a')));
    }

    #[test]
    fn bare_modifier_is_skipped() {
        // A keydown with no char and an unmapped vk (e.g. a lone Ctrl) is skipped.
        assert!(matches!(
            decode_record(0, LEFT_CTRL_PRESSED, 0),
            Decoded::Skip
        ));
    }

    #[test]
    fn high_surrogate_defers() {
        assert!(matches!(
            decode_record(0, 0, 0xD83D),
            Decoded::HighSurrogate(0xD83D)
        ));
    }
}
