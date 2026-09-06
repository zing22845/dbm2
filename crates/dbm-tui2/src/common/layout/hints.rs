//! Hint text helpers that layout depends on: the shared separators, the
//! `desc: key` formatter and the footer lines whose height a pane reserves.
//!
//! These live next to the layout code (rather than in `common::view`) because
//! measuring how many rows a pane needs is layout work; the renderer only
//! consumes the resulting text.

use crate::common::utils::shortcuts::hint_ctrl;

/// Visible field separator for hint pairs.
pub const SEP: &str = "  ";

/// `"{desc}: {key_name}"`.
pub fn key(desc: &str, key_name: &str) -> String {
    format!("{desc}: {key_name}")
}

/// Join hint parts with the visible separator.
pub fn join(parts: &[&str]) -> String {
    parts.join(SEP)
}

/// A bare key name, with no modifier prefix.
pub(crate) fn lit(s: &str) -> String {
    s.to_string()
}

/// Join a list of `(desc, key_name)` pairs into one hint line.
pub(crate) fn keys(hints: &[(&str, String)]) -> String {
    hints
        .iter()
        .map(|(desc, key_name)| key(desc, key_name))
        .collect::<Vec<_>>()
        .join(SEP)
}

/// Footer for the discover dialog (the whole modal): pane navigation + the
/// discover-level actions available from any pane. `status` (e.g. scanning /
/// last error) is appended on a second line when non-empty.
pub fn discover_footer_text(status: &str) -> String {
    let hints = keys(&[
        ("Pane", hint_ctrl("j/k")),
        ("Scan", lit("s")),
        ("Close", lit("ESC")),
    ]);
    if status.is_empty() {
        hints
    } else {
        format!("{hints}\n{status}")
    }
}

/// Footer for the discover engine selector pane. `status` (e.g. the note that
/// only Postgres is available) is appended on a second line when non-empty.
pub fn discover_engine_footer_text(status: Option<&str>) -> String {
    let mut footer = keys(&[("Engine", lit("e/ENTER"))]);
    if let Some(status) = status.filter(|s| !s.is_empty()) {
        footer.push('\n');
        footer.push_str(status);
    }
    footer
}
