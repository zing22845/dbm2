//! Shared footer/hint text builders.
//!
//! The hint strings shown in the global footer and each pane's status line are
//! assembled here so they are unit-testable in isolation. The pure helpers
//! (`key`, `join`, `keys`, `pane_search_active_footer`) are feature-free; the
//! per-pane builders take the owning feature's state so rendering stays
//! presentational.

use crate::app::state::ModalKind;
use crate::common::components::search::PaneSearch;
use crate::common::utils::shortcuts::{
    copy_shortcut_label, hint_ctrl, paste_shortcut_label, quit_shortcut_label,
};
use crate::common::view::theme::Theme;

/// Visible field separator for hint pairs.
pub const SEP: &str = "  ";

/// Empty-state hint shown in the SQL workspace when no connection has an open
/// query tab, mirroring the original dbm's `workspace_empty_hint`.
pub fn sql_workspace_empty_hint() -> &'static str {
    "Select a connection in the tree — ENTER or double-click to open a workspace."
}

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
            ("Context", lit("click title")),
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
    // When the detail pane is open the original dbm also exposes the detail
    // width splitter ("Width: [/]") right after Inspect.
    let base = keys(&[
        ("Inspect", lit("ENTER")),
        if detail_open {
            ("Width", lit("[/]"))
        } else {
            ("Col width", lit(",/."))
        },
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

/// Footer for the discover engine selector pane.
pub fn discover_engine_footer_text() -> String {
    keys(&[("Engine", lit("e/ENTER"))])
}

/// Footer for the discover targets editor pane, switching on edit state.
/// `has_loopback` appends a note that loopback also runs local discovery.
pub fn discover_targets_footer_text(editing: bool, has_loopback: bool) -> String {
    if editing {
        return keys(&[("Commit", lit("ENTER")), ("Cancel", lit("ESC"))]);
    }
    let mut footer = keys(&[
        ("Edit", lit("i/ENTER")),
        ("Insert", lit("o")),
        ("Delete", lit("d")),
        ("Paste TSV/host:ports", paste_shortcut_label()),
        ("Undo", lit("u")),
        ("Redo", hint_ctrl("r")),
    ]);
    if has_loopback {
        footer.push('\n');
        footer.push_str(DISCOVER_LOOPBACK_SCAN_NOTE);
    }
    footer
}

/// Note shown under Targets when any row is loopback — Ports only constrain TCP
/// probes, not local discovery.
pub const DISCOVER_LOOPBACK_SCAN_NOTE: &str =
    "Loopback also runs local discovery (process/pid/socket); Ports only limit TCP probes.";

/// Footer for the discover results list pane.
pub fn discover_results_footer_text() -> String {
    keys(&[
        ("Select", lit("SPACE")),
        ("Register", lit("r")),
        ("Force", lit("R")),
        ("Filter", lit("u")),
    ])
}

/// Footer hints for the data-carrying modals; Discover draws its own footer
/// and returns empty.
pub fn modal_footer_text(modal: Option<&ModalKind>) -> String {
    crate::common::view::modal::modal_footer_text(modal)
}

/// Global footer hints (pane navigation + shortcuts).
///
/// Kept intentionally short (the perf readout occupies the footer's right side)
/// so only the essential pane navigation and search are shown. This is the
/// single source of truth for the global footer; `global_footer::view` renders
/// it instead of a hardcoded list.
pub fn global_footer_text(global_status: &str) -> String {
    let hints = keys(&[
        ("Pane", lit("TAB")),
        ("SubPane", hint_ctrl("h/j/k/l")),
        ("Search", lit("/")),
        ("Width", lit("[/]")),
        ("Height", lit("+/-")),
        ("Resize", lit("drag")),
        ("H-Scroll", lit("←/→")),
        ("Copy", copy_shortcut_label()),
        ("Quit", quit_shortcut_label()),
    ]);
    if global_status.is_empty() {
        hints
    } else {
        format!("{hints}\n{global_status}")
    }
}

/// Footer for the explorer instances tree, mirroring the original dbm. The
/// hints differ by row kind: an instance row shows Open/Add/Expand/Collapse,
/// a connection row shows New/Open/Add/Edit.
pub fn instances_pane_footer_text(instance_row: bool) -> String {
    if instance_row {
        keys(&[
            ("Open", lit("ENTER / Dbl-click")),
            ("Add conn", lit("a")),
            ("Expand", lit("l")),
            ("Collapse", lit("h")),
        ])
    } else {
        keys(&[
            ("New", lit("n")),
            ("Open", lit("ENTER / Dbl-click")),
            ("Add conn", lit("a")),
            ("Edit conn", lit("i")),
        ])
    }
}

/// Footer for the explorer objects tree, mirroring the original dbm.
pub fn objects_pane_footer_text() -> String {
    keys(&[
        ("Open schema", lit("ENTER")),
        ("Expand", lit("l")),
        ("Collapse", lit("h")),
        ("Refresh", lit("r")),
    ])
}

/// Footer for the instance workspace sub-pane, mirroring the original dbm:
/// the Overview pane shows Refresh/Unregister/H-Scroll; the Connections pane
/// shows Add/Edit/Delete/Test.
pub fn instance_workspace_footer_text(pane: crate::app_shell::nav::IwPane) -> String {
    match pane {
        crate::app_shell::nav::IwPane::Overview => keys(&[
            ("Refresh", lit("r")),
            ("Unregister", lit("u")),
            ("H-Scroll", lit("←/→")),
        ]),
        crate::app_shell::nav::IwPane::Connections => keys(&[
            ("Add", lit("a")),
            ("Edit", lit("i")),
            ("Delete", lit("d")),
            ("Test", lit("t")),
        ]),
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

/// Draw a pane's footer hint line into `area` (already the bottom strip of the
/// pane's inner rect). Pure `state -> view`: reads only the theme and text.
pub fn draw_pane_footer(frame: &mut ratatui::Frame, theme: &Theme, area: ratatui::layout::Rect, text: &str) {
    if text.is_empty() || area.height == 0 || area.width == 0 {
        return;
    }
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;
    let p = theme.palette();
    let style = ratatui::style::Style::default().fg(p.muted);
    let lines: Vec<Line> = text
        .split('\n')
        .map(|l| Line::from(Span::styled(l.to_string(), style)))
        .collect();
    frame.render_widget(Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }), area);
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
    fn sql_workspace_empty_hint_points_to_connection_selection() {
        let hint = sql_workspace_empty_hint();
        assert!(hint.contains("Select a connection"));
        assert!(hint.contains("open a workspace"));
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
    fn explorer_instance_footer_varies_by_row_kind() {
        let inst = instances_pane_footer_text(true);
        assert!(inst.contains("Expand: l"));
        assert!(inst.contains("Collapse: h"));
        assert!(inst.contains("Add conn: a"));
        assert!(inst.contains("Open: ENTER / Dbl-click"));
        // Connection row shows New/Edit instead of Expand/Collapse.
        let conn = instances_pane_footer_text(false);
        assert!(conn.contains("New: n"));
        assert!(conn.contains("Edit conn: i"));
        assert!(!conn.contains("Expand:"));
    }

    #[test]
    fn explorer_objects_footer_lists_open_expand_collapse_refresh() {
        let footer = objects_pane_footer_text();
        assert!(footer.contains("Open schema: ENTER"));
        assert!(footer.contains("Expand: l"));
        assert!(footer.contains("Collapse: h"));
        assert!(footer.contains("Refresh: r"));
    }

    #[test]
    fn instance_workspace_footer_varies_by_subpane() {
        use crate::app_shell::nav::IwPane;
        let ov = instance_workspace_footer_text(IwPane::Overview);
        assert!(ov.contains("Refresh: r"));
        assert!(ov.contains("Unregister: u"));
        let conn = instance_workspace_footer_text(IwPane::Connections);
        assert!(conn.contains("Add: a"));
        assert!(conn.contains("Edit: i"));
        assert!(conn.contains("Delete: d"));
        assert!(conn.contains("Test: t"));
    }

    #[test]
    fn modal_footer_with_no_modal_is_empty() {
        assert_eq!(modal_footer_text(None), "");
    }

    #[test]
    fn discover_footers_switch_on_state() {
        // Footer lists pane nav + scan/close; a status appends a line.
        let footer = discover_footer_text("");
        assert!(footer.contains("Pane: "));
        assert!(footer.contains("Scan: s"));
        assert!(footer.contains("Close: ESC"));
        assert!(!footer.contains('\n'));
        let with_status = discover_footer_text("scanning…");
        assert!(with_status.contains("\nscanning…"));
        // Targets footer shows edit keys when not editing, commit/cancel when editing.
        let edit = discover_targets_footer_text(false, false);
        assert!(edit.contains("Edit: i/ENTER"));
        assert!(edit.contains("Insert: o"));
        assert!(edit.contains("Delete: d"));
        assert!(edit.contains("Paste TSV/host:ports"));
        // A loopback row appends the local-discovery note on a second line.
        let loopback = discover_targets_footer_text(false, true);
        assert!(loopback.contains(DISCOVER_LOOPBACK_SCAN_NOTE));
        assert!(loopback.contains('\n'));
        let committing = discover_targets_footer_text(true, false);
        assert!(committing.contains("Commit: ENTER"));
        assert!(committing.contains("Cancel: ESC"));
        // Results footer lists selection / register / force-register / filter.
        let results = discover_results_footer_text();
        assert!(results.contains("Select: SPACE"));
        assert!(results.contains("Register: r"));
        assert!(results.contains("Force: R"));
        assert!(results.contains("Filter: u"));
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
