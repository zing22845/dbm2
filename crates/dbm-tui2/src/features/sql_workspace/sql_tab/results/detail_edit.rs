//! Results Detail draft Save / leave-gate helpers (pure, unit-tested).

pub const DETAIL_LEAVE_WARNING: &str =
    "Unsaved changes - save (C-s) or discard (C-u) before leaving";

pub fn detail_draft_dirty(editor_text: &str, baseline: &str) -> bool {
    editor_text != baseline
}

pub fn save_button_visible(edit_active: bool) -> bool {
    edit_active
}

pub fn save_button_enabled(edit_active: bool, dirty: bool) -> bool {
    edit_active && dirty
}

/// Discard shares Save enable rules (enabled only when Edit on and dirty).
pub fn discard_button_enabled(edit_active: bool, dirty: bool) -> bool {
    save_button_enabled(edit_active, dirty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_hidden_when_edit_off() {
        assert!(!save_button_visible(false));
        assert!(!save_button_enabled(false, true));
    }

    #[test]
    fn save_disabled_when_clean() {
        assert!(save_button_visible(true));
        assert!(!save_button_enabled(true, false));
        assert!(save_button_enabled(true, true));
    }

    #[test]
    fn discard_matches_save_enable() {
        assert_eq!(
            discard_button_enabled(true, false),
            save_button_enabled(true, false)
        );
        assert_eq!(
            discard_button_enabled(true, true),
            save_button_enabled(true, true)
        );
        assert_eq!(
            discard_button_enabled(false, true),
            save_button_enabled(false, true)
        );
    }

    #[test]
    fn dirty_compares_baseline() {
        assert!(detail_draft_dirty("a", "b"));
        assert!(!detail_draft_dirty("a", "a"));
    }
}
