//! Reading images (and text) from the Windows clipboard for chat attachments.
//!
//! A copied image lives in the clipboard as binary data, not as keystrokes — so,
//! unlike a text paste, it can't be captured from the input stream; we have to ask
//! the OS for it. We read the registered **"PNG"** clipboard format, which the
//! common screenshot sources (Snipping Tool, Snip & Sketch, browser "copy image")
//! populate, so the bytes are already a PNG the providers accept — no decoding and
//! no extra dependency. A raw PrintScreen (device bitmap only) is intentionally out
//! of scope; supporting it would need a DIB→PNG encoder.
//!
//! Off Windows these are graceful no-ops: an explanatory error / no text.

use crate::image::{loaded_from_bytes, LoadedImage};

/// Read an image off the clipboard as a validated [`LoadedImage`], or an error
/// describing why nothing was attached (no image present, unsupported, too large).
#[cfg(windows)]
pub fn read_clipboard_image() -> Result<LoadedImage, String> {
    let bytes = read_format_bytes_by_name("PNG")
        .ok_or_else(|| "no image found on the clipboard (copy a screenshot first)".to_string())?;
    loaded_from_bytes(bytes, "clipboard.png")
}

#[cfg(not(windows))]
pub fn read_clipboard_image() -> Result<LoadedImage, String> {
    Err("clipboard image paste is only supported on Windows".to_string())
}

/// Read UTF-16 text off the clipboard — the fallback for Ctrl+V when there's no
/// image to attach, so paste still does the expected thing. `None` if absent.
#[cfg(windows)]
pub fn read_clipboard_text() -> Option<String> {
    const CF_UNICODETEXT: u32 = 13;
    let bytes = read_format_bytes(CF_UNICODETEXT)?;
    // The data is UTF-16LE, NUL-terminated; stop at the first NUL unit.
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .take_while(|&u| u != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

#[cfg(not(windows))]
pub fn read_clipboard_text() -> Option<String> {
    None
}

/// Resolve a named clipboard format (e.g. `"PNG"`) and read its bytes, if present.
#[cfg(windows)]
fn read_format_bytes_by_name(name: &str) -> Option<Vec<u8>> {
    use windows_sys::Win32::System::DataExchange::RegisterClipboardFormatW;
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a valid NUL-terminated UTF-16 string for the call's duration.
    let fmt = unsafe { RegisterClipboardFormatW(wide.as_ptr()) };
    if fmt == 0 {
        return None;
    }
    read_format_bytes(fmt)
}

/// Copy the raw bytes of a clipboard format out as an owned `Vec`, or `None` if the
/// format isn't present / can't be locked. Opens and always closes the clipboard.
#[cfg(windows)]
fn read_format_bytes(format: u32) -> Option<Vec<u8>> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    // SAFETY: each call is checked; the clipboard is closed on every exit path, and
    // the locked handle is unlocked before returning. The pointer/size from
    // `GlobalLock`/`GlobalSize` describe a buffer the OS owns for the lock's scope.
    unsafe {
        if IsClipboardFormatAvailable(format) == 0 {
            return None;
        }
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let bytes = (|| {
            let handle = GetClipboardData(format);
            if handle.is_null() {
                return None;
            }
            let ptr = GlobalLock(handle);
            if ptr.is_null() {
                return None;
            }
            let size = GlobalSize(handle);
            let out = std::slice::from_raw_parts(ptr as *const u8, size).to_vec();
            GlobalUnlock(handle);
            Some(out)
        })();
        CloseClipboard();
        bytes
    }
}
