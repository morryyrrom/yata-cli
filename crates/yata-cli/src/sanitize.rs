//! Escaping of strings that come from policy directories before they are written to a terminal.
//!
//! A rule can return any text as a message, and Rego errors quote the source. Control
//! characters in such text (ANSI escape sequences, carriage returns that overwrite lines, ...)
//! could rewrite what the user sees, so they are escaped.

use std::borrow::Cow;
use std::fmt::Write;

/// Whether `c` could be used to fake what a terminal or a log viewer shows.
fn needs_escape(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061c}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{206f}'
        )
}

/// Escapes control characters (C0 and C1 controls and DEL, including ESC and newlines), the line
/// and paragraph separators (U+2028, U+2029) and the bidirectional controls (U+200E, U+200F,
/// U+202A-U+202E, U+2066-U+2069, U+061C and the deprecated format controls U+206A-U+206F) as
/// `\u{1b}`, so that one value always fits on one line. Other characters are kept as they are.
pub fn escape_controls(text: &str) -> Cow<'_, str> {
    if !text.chars().any(needs_escape) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if needs_escape(c) {
            // Writing to a `String` cannot fail.
            let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_borrowed() {
        assert!(matches!(
            escape_controls("abc \u{e9} \u{1f600}"),
            Cow::Borrowed(_)
        ));
        assert_eq!(escape_controls(""), "");
    }

    #[test]
    fn escapes_c0_c1_and_del() {
        assert_eq!(escape_controls("a\u{1b}[31mb"), "a\\u{1b}[31mb");
        assert_eq!(escape_controls("a\nb\r\nc"), "a\\u{a}b\\u{d}\\u{a}c");
        assert_eq!(
            escape_controls("\u{0}\u{7f}\u{9b}\u{9f}"),
            "\\u{0}\\u{7f}\\u{9b}\\u{9f}"
        );
        assert_eq!(escape_controls("\t"), "\\u{9}");
    }

    #[test]
    fn result_has_no_control_characters() {
        let all: String = (0u32..0x3000).filter_map(char::from_u32).collect();
        assert!(!escape_controls(&all).chars().any(needs_escape));
    }

    #[test]
    fn escapes_line_separators_and_bidirectional_controls() {
        assert_eq!(
            escape_controls("a\u{2028}b\u{2029}c"),
            "a\\u{2028}b\\u{2029}c"
        );
        assert_eq!(
            escape_controls("\u{200e}\u{200f}\u{202a}\u{202e}\u{2066}\u{2069}"),
            "\\u{200e}\\u{200f}\\u{202a}\\u{202e}\\u{2066}\\u{2069}"
        );
        // The neighbours of the ranges are kept.
        assert_eq!(
            escape_controls("\u{200d}\u{202f}\u{2065}\u{2070}\u{061b}\u{061d}"),
            "\u{200d}\u{202f}\u{2065}\u{2070}\u{061b}\u{061d}"
        );
        assert_eq!(
            escape_controls("\u{061c}\u{206a}\u{206f}"),
            "\\u{61c}\\u{206a}\\u{206f}"
        );
    }

    #[test]
    fn printable_non_ascii_is_kept() {
        assert_eq!(escape_controls("\u{3042}\u{a0}"), "\u{3042}\u{a0}");
    }
}
