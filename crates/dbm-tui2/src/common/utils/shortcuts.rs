//! Shortcut chord label formatting.
//!
//! These helpers render human-readable shortcut labels for hints/footers.
//! Modifier names are always uppercase (`CTRL`, `ALT`, `SHIFT`, `CMD`).

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
