use arboard::Clipboard;

/// Copy `text` to the system clipboard (Copy shortcuts and result copy actions).
///
/// Paste into Discover (and the editor) is handled exclusively by the
/// terminal's bracketed-paste `Event::Paste`, never by re-reading the system
/// clipboard on a key shortcut — reading on every key repeat used to spike CPU
/// and could double-paste alongside `Event::Paste`.
pub fn copy_to_system(text: &str) -> Result<(), String> {
    Clipboard::new()
        .map_err(|e| e.to_string())?
        .set_text(text.to_string())
        .map_err(|e| e.to_string())
}
