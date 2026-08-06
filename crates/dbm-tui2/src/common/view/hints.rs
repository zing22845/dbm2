//! Shared footer/hint text builders.
//!
//! The hint strings shown in the global footer and each pane's status line are
//! assembled here so they are unit-testable in isolation. The pure helpers
//! (`key`, `join`, `keys`, `pane_search_active_footer`) are feature-free; the
//! per-pane builders take the owning feature's state so rendering stays
//! presentational.

use crate::app::state::ModalKind;
use crate::common::components::search::PaneSearch;
use crate::common::utils::shortcuts::{hint_ctrl, quit_shortcut_label};

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

fn lit(s: &str) -> String {
    s.to_string()
}

/// Join a list of `(desc, key_name)` pairs into one hint line.
fn keys(hints: &[(&str, String)]) -> String {
    hints
        .iter()
        .map(|(desc, key_name)| key(desc, key_name))
        .collect::<Vec<_>>()
        .join(SEP)
}

/// The footer shown while a pane `/` search input is active.
pub fn pane_search_active_footer(extra: &[(&str, String)]) -> String {
    let mut hints = keys(&[
        ("Prev", hint_ctrl("p")),
        ("Next", hint_ctrl("n")),
        ("Apply", lit("ENTER")),
        ("Cancel", lit("ESC")),
        ("Clear", hint_ctrl("u")),
        ("Case", hint_ctrl("/")),
    ]);
    if !extra.is_empty() {
        hints = join(&[&hints, &keys(extra)]);
    }
    hints
}

/// Footer for the results detail pane (back to the table).
pub fn results_detail_footer_text() -> String {
    keys(&[("Back", lit("ESC"))])
}

/// Footer for the SQL editor pane, adapted to a single tab's editor state.
pub fn sql_pane_footer_text(
    search_active: bool,
    sql_search_active: bool,
    editor_mode: &str,
    sql_search_has_filter: bool,
) -> String {
    if search_active || sql_search_active {
        return pane_search_active_footer(&[]);
    }
    match editor_mode {
        "insert" => keys(&[
            ("History", hint_ctrl("r")),
            ("Complete", "SHIFT+TAB".into()),
            ("Normal", lit("ESC")),
            ("Run", "ALT+ENTER".into()),
        ]),
        "visual" => keys(&[("Context", lit(", / click")), ("Normal", lit("ESC"))]),
        _ => {
            let mut hints = keys(&[
                ("History", hint_ctrl("r")),
                ("Context", lit(", / click")),
                ("Insert", lit("i")),
                ("Visual", lit("v")),
                ("Run", "ALT+ENTER".into()),
            ]);
            if sql_search_has_filter {
                hints = join(&[
                    &hints,
                    &keys(&[("Next match", lit("n/N")), ("Clear filter", lit("ESC"))]),
                ]);
            }
            hints
        }
    }
}

/// Footer for the results pane: search-active vs. table/detail hints.
pub fn results_pane_footer_text(
    search_active: bool,
    detail_open: bool,
    status: &str,
) -> String {
    if search_active {
        return pane_search_active_footer(&[]);
    }
    let esc_hint = if detail_open {
        ("Close detail", lit("ESC"))
    } else {
        ("Deselect", lit("ESC"))
    };
    let base = keys(&[
        ("Inspect", lit("ENTER")),
        ("Col width", lit(",/.")),
        ("Copy Col Name", hint_ctrl("n")),
        esc_hint,
        ("Flip", lit("f/b")),
        ("Toolbar", lit("click")),
        ("Top", lit("g")),
        ("Bottom", lit("G")),
    ]);
    if status.is_empty() {
        base
    } else {
        format!("{base}\n{status}")
    }
}

/// Footer for the history list pane.
pub fn history_list_footer_text(
    search_active: bool,
    search_has_filter: bool,
    focus_return: bool,
) -> String {
    if search_active {
        return pane_search_active_footer(&[]);
    }
    let mut hints = keys(&[
        ("Move", hint_ctrl("p/n")),
        ("Apply", lit("ENTER / Dbl-click")),
        ("Jump", lit("g/G")),
    ]);
    if search_has_filter {
        hints = join(&[&hints, &keys(&[("Clear filter", lit("ESC"))])]);
    }
    if focus_return {
        hints = join(&[&hints, &keys(&[("Back to SQL", lit("ESC"))])]);
    }
    hints
}

/// Footer hints for the data-carrying modals; Discover draws its own zone
/// footer and returns empty.
pub fn modal_footer_text(modal: Option<&ModalKind>) -> String {
    crate::common::view::modal::modal_footer_text(modal)
}

/// Global footer hints (zone navigation + shortcuts).
pub fn global_footer_text(global_status: &str) -> String {
    let hints = keys(&[
        ("Zone", lit("TAB")),
        ("Pane", hint_ctrl("h/j/k/l")),
        ("Search", lit("/")),
        ("Width", lit("[/]")),
        ("Height", lit("+/-")),
        ("H-Scroll", lit("←/→")),
        ("Quit", quit_shortcut_label()),
    ]);
    if global_status.is_empty() {
        hints
    } else {
        format!("{hints}\n{global_status}")
    }
}

/// Convenience: turn a `PaneSearch` into the search-active footer, or empty.
pub fn pane_search_footer_if_active(search: &PaneSearch) -> Option<String> {
    if search.text_input_active() {
        Some(pane_search_active_footer(&[]))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_and_join_format() {
        assert_eq!(key("Run", "ALT+ENTER"), "Run: ALT+ENTER");
        assert_eq!(join(&["a: x", "b: y"]), "a: x  b: y");
    }

    #[test]
    fn pane_search_active_footer_lists_chords() {
        let footer = pane_search_active_footer(&[]);
        assert!(footer.contains(&format!("Prev: {}", hint_ctrl("p"))));
        assert!(footer.contains("Apply: ENTER"));
        assert!(footer.contains("Case: "));
    }

    #[test]
    fn sql_pane_insert_footer_differs_from_normal() {
        let insert = sql_pane_footer_text(false, false, "insert", false);
        assert!(insert.contains("Complete: SHIFT+TAB"));
        assert!(insert.contains("Normal: ESC"));
        let normal = sql_pane_footer_text(false, false, "normal", false);
        assert!(!normal.contains("Complete:"));
        assert!(normal.contains("Insert: i"));
        assert!(normal.contains("Visual: v"));
    }

    #[test]
    fn sql_pane_normal_appends_search_jump_when_filtered() {
        let filtered = sql_pane_footer_text(false, false, "normal", true);
        assert!(filtered.contains("Next match: n/N"));
        assert!(filtered.contains("Clear filter: ESC"));
        let unfiltered = sql_pane_footer_text(false, false, "normal", false);
        assert!(!unfiltered.contains("Next match:"));
    }

    #[test]
    fn sql_pane_search_active_shows_search_footer() {
        let footer = sql_pane_footer_text(false, true, "normal", false);
        assert!(footer.contains("Prev: "));
        assert!(!footer.contains("Insert: i"));
    }

    #[test]
    fn results_pane_detail_open_esc_label_changes() {
        let open = results_pane_footer_text(false, true, "");
        assert!(open.contains("Close detail: ESC"));
        let closed = results_pane_footer_text(false, false, "");
        assert!(closed.contains("Deselect: ESC"));
        let with_status = results_pane_footer_text(false, false, "updated");
        assert!(with_status.contains("\nupdated"));
    }

    #[test]
    fn history_list_focus_return_adds_hint() {
        let base = history_list_footer_text(false, false, false);
        assert!(!base.contains("Back to SQL"));
        let ret = history_list_footer_text(false, false, true);
        assert!(ret.contains("Back to SQL: ESC"));
    }

    #[test]
    fn modal_footer_for_discover_is_empty() {
        assert_eq!(modal_footer_text(Some(&ModalKind::Discover)), "");
        assert_eq!(modal_footer_text(None), "");
    }

    #[test]
    fn global_footer_includes_quit_and_status_on_second_line() {
        let base = global_footer_text("");
        assert!(base.contains("Quit: "));
        let with_status = global_footer_text("ready");
        let lines: Vec<_> = with_status.lines().collect();
        assert_eq!(lines.last().copied(), Some("ready"));
    }
}
