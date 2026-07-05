//! Optional Slint glue (`feature = "slint"`).
//!
//! Two jobs only: turn the core's RGBA buffer into a `slint::Image`, and turn a
//! Slint `KeyEvent` (text + modifiers) into terminal input bytes. Everything
//! here is deliberately tiny so the slint version stays loosely pinned and
//! unentangled from the framework-agnostic core.

use slint::platform::Key;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

/// Convert an RGBA8 buffer (as produced by [`crate::Terminal::render`]) into a
/// `slint::Image`. Copies once into a `SharedPixelBuffer`.
pub fn rgba_to_image(rgba: &[u8], width: u32, height: u32) -> Image {
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(rgba, width, height);
    Image::from_rgba8(buffer)
}

fn is_key(text: &str, key: Key) -> bool {
    slint::SharedString::from(key).as_str() == text
}

/// VT escape sequence for named/navigation keys, or `None` if `text` is not one.
///
/// Slint encodes these as specific code points (e.g. Return is `U+000A`, arrows
/// live in the `U+F70x` private-use range); matching them here keeps the raw
/// code points from leaking to the PTY.
fn named_key(text: &str) -> Option<&'static [u8]> {
    let seq: &[u8] = if is_key(text, Key::Return) {
        b"\r"
    } else if is_key(text, Key::Backspace) {
        b"\x7f"
    } else if is_key(text, Key::Tab) {
        b"\t"
    } else if is_key(text, Key::Escape) {
        b"\x1b"
    } else if is_key(text, Key::UpArrow) {
        b"\x1b[A"
    } else if is_key(text, Key::DownArrow) {
        b"\x1b[B"
    } else if is_key(text, Key::RightArrow) {
        b"\x1b[C"
    } else if is_key(text, Key::LeftArrow) {
        b"\x1b[D"
    } else if is_key(text, Key::Home) {
        b"\x1b[H"
    } else if is_key(text, Key::End) {
        b"\x1b[F"
    } else if is_key(text, Key::Delete) {
        b"\x1b[3~"
    } else if is_key(text, Key::PageUp) {
        b"\x1b[5~"
    } else if is_key(text, Key::PageDown) {
        b"\x1b[6~"
    } else {
        return None;
    };
    Some(seq)
}

/// Ctrl + `c` → the corresponding C0 control byte, mirroring a real terminal
/// (Ctrl-A..Z → 0x01..0x1A, plus the usual punctuation controls). Returns
/// `None` when there is no sensible control mapping.
fn ctrl_byte(c: char) -> Option<u8> {
    match c {
        'a'..='z' => Some(c as u8 - b'a' + 1),
        'A'..='Z' => Some(c as u8 - b'A' + 1),
        '@' | ' ' | '2' => Some(0x00),
        '[' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '7' | '/' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        '3' => Some(0x1b),
        _ => None,
    }
}

/// Map a Slint key press to the byte sequence a terminal expects.
///
/// - Named/navigation keys become their VT escape sequences.
/// - Ctrl + letter/punctuation becomes the matching C0 control byte.
/// - Alt prefixes the output with ESC (the common "meta sends escape" convention).
/// - Bare modifier presses and other unhandled special keys are dropped (`None`),
///   so their raw code points never reach the PTY.
/// - Anything else (printable ASCII, IME-committed CJK) is forwarded as UTF-8.
pub fn key_to_bytes(text: &str, ctrl: bool, alt: bool) -> Option<Vec<u8>> {
    if let Some(seq) = named_key(text) {
        return Some(seq.to_vec());
    }

    if text.is_empty() {
        return None;
    }

    // Reject leftover special-key code points: unhandled C0 controls (bare
    // Shift/Control/Alt/... land here), DEL, and the private-use key block.
    let first = text.chars().next().unwrap();
    if (first.is_control() || ('\u{F700}'..='\u{F8FF}').contains(&first))
        && !(ctrl && first.is_ascii_graphic())
    {
        return None;
    }

    // Ctrl + single character → C0 control byte.
    if ctrl {
        let mut chars = text.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            if let Some(b) = ctrl_byte(c) {
                return Some(if alt { vec![0x1b, b] } else { vec![b] });
            }
        }
    }

    // Printable text (including IME-committed CJK).
    let mut bytes = text.as_bytes().to_vec();
    if alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}
