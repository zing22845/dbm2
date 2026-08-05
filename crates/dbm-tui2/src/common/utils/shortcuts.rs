//! Shortcut chord label formatting and key-decode helpers.
//!
//! The label helpers render human-readable shortcut chords for hints/footers
//! with uppercase modifier names (`CTRL`, `ALT`, `SHIFT`, `CMD`). The key-decode
//! helpers handle terminal quirks (Shift vs. Caps Lock, pane-jump chords) shared
//! by panes that accept typed characters, e.g. the in-pane `/` search.

use crossterm::event::{KeyEvent, KeyEventState, KeyModifiers};

/// Whether Caps Lock is active for this key event (from the key state, or the
/// tracked flag the event loop keeps when a terminal does not report it).
pub(crate) fn caps_lock_active(key: &KeyEvent, tracked_caps_lock: bool) -> bool {
    key.state.intersects(KeyEventState::CAPS_LOCK) || tracked_caps_lock
}

/// Apply `Shift XOR Caps Lock` to an ASCII letter (terminals send lowercase +
/// flags in CSI-u, so the effective case is derived from both).
pub(crate) fn apply_ascii_letter_case(c: char, shift: bool, caps_lock: bool) -> char {
    let base = c.to_ascii_lowercase();
    if shift ^ caps_lock {
        base.to_ascii_uppercase()
    } else {
        base
    }
}

/// Effective letter for editor input and pane shortcuts.
pub(crate) fn effective_ascii_letter(c: char, shift: bool, caps_lock: bool) -> char {
    if !c.is_ascii_alphabetic() {
        return c;
    }
    if !shift && !caps_lock && c.is_ascii_uppercase() {
        return c;
    }
    apply_ascii_letter_case(c, shift, caps_lock)
}

/// Pane-jump / typed-character chords may carry `Shift`; reject CTRL/ALT/META.
pub(crate) fn pane_jump_modifiers_ok(modifiers: KeyModifiers) -> bool {
    !modifiers.intersects(
        KeyModifiers::CONTROL
            | KeyModifiers::ALT
            | KeyModifiers::META
            | KeyModifiers::SUPER
            | KeyModifiers::HYPER,
    )
}

/// `CTRL+<key>` label.
pub fn hint_ctrl(key: &str) -> String {
    format!("CTRL+{key}")
}

/// `ALT+<key>` label.
pub fn hint_alt(key: &str) -> String {
    format!("ALT+{key}")
}

/// `SHIFT+<key>` label.
pub fn hint_shift(key: &str) -> String {
    format!("SHIFT+{key}")
}

/// `CMD+<key>` label.
pub fn hint_cmd(key: &str) -> String {
    format!("CMD+{key}")
}

/// Label for the platform copy shortcut: `CMD+C` on macOS, `CTRL+C` elsewhere.
pub fn copy_shortcut_label() -> String {
    #[cfg(target_os = "macos")]
    {
        hint_cmd("C")
    }
    #[cfg(not(target_os = "macos"))]
    {
        hint_ctrl("C")
    }
}

/// Label for the platform quit shortcut: `CTRL+D`.
pub fn quit_shortcut_label() -> String {
    hint_ctrl("D")
}
