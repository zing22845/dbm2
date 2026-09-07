//! Hint text helpers that layout depends on: the shared separators, the
//! `desc: key` formatter and the footer lines whose height a pane reserves.
//!
//! These live next to the layout code (rather than in `common::view`) because
//! measuring how many rows a pane needs is layout work; the renderer only
//! consumes the resulting text.

use crate::common::utils::shortcuts::hint_ctrl;

use crate::app::state::ModalKind;
use crate::common::components::search::PaneSearch;
use crate::common::utils::shortcuts::{
    copy_shortcut_label, paste_shortcut_label, quit_shortcut_label,
};

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

/// Empty-state hint shown in the SQL workspace when the active connection has
/// no open query tab, mirroring the original dbm's `workspace_empty_hint`.
/// `has_connection` distinguishes "a connection is active but has no tabs"
/// (show the connection-specific hint) from "no connection selected at all".
pub fn sql_workspace_empty_hint(has_connection: bool) -> &'static str {
    if has_connection {
        "No query tabs for this connection — press n or ENTER in the tree."
    } else {
        "Select a connection in the tree — ENTER or double-click to open a workspace."
    }
}

/// Workspace-level footer hint for SQL tab management (belongs to the SQL
/// workspace, not to a single editor pane). (`Ctrl+T` is reserved for theme
/// toggling, so there is no open-tab shortcut.)
pub fn sql_workspace_footer_text() -> String {
    format!(
        "TAB Ops [{}]",
        [
            "New: Alt+t",
            "Close: Ctrl+w",
            "Switch: Alt+1-9",
            "Next: Alt+n",
            "Prev: Alt+p",
        ]
        .join("  ")
    )
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
    complete_table_names: bool,
) -> String {
    if search_active || sql_search_active {
        return pane_search_active_footer(&[]);
    }
    match editor_mode {
        "insert" => keys(&[
            ("History", hint_ctrl("r")),
            (
                "Tbl complete",
                format!(
                    "{} ({})",
                    if complete_table_names { "ON" } else { "OFF" },
                    "ALT+TAB"
                ),
            ),
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

/// Footer for the results pane: search-active vs. table hints.
///
/// The detail pane's own footer already advertises "Back: ESC" and the global
/// footer exposes the ["/"] pane-width hint, so neither the close-ESC nor the
/// detail-width splitter is repeated here.
pub fn results_pane_footer_text(search_active: bool, status: &str) -> String {
    if search_active {
        return pane_search_active_footer(&[]);
    }
    let base = keys(&[
        ("Inspect", lit("ENTER")),
        ("Col width", lit(",/.")),
        ("Copy Col Name", hint_ctrl("n")),
        ("Flip", lit("f/b")),
        ("Top/Bottom", lit("g/G")),
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

/// Footer for the discover targets editor pane, switching on edit state.
/// `has_loopback` appends a note that loopback also runs local discovery, and
/// `status` (the last paste/undo/redo feedback, e.g. "Paste: 1/3 added …") is
/// appended on a second line when non-empty.
pub fn discover_targets_footer_text(
    editing: bool,
    has_loopback: bool,
    status: Option<&str>,
) -> String {
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
    if let Some(status) = status.filter(|s| !s.is_empty()) {
        footer.push('\n');
        footer.push_str(status);
    }
    footer
}

/// Note shown under Targets when any row is loopback — Ports only constrain TCP
/// probes, not local discovery.
pub const DISCOVER_LOOPBACK_SCAN_NOTE: &str =
    "Loopback also runs local discovery (process/pid/socket); Ports only limit TCP probes.";

/// Row-mark legend shown under the results keys: `✓` marks a selected
/// (unregistered) instance, `×` marks one that is already registered.
pub const RESULTS_LEGEND: &str = "mark: ✓ selected · × registered";

/// Footer for the discover results list pane: a keys line plus a legend line
/// explaining the row marks (`✓` selected, `×` already registered).
pub fn discover_results_footer_text() -> String {
    format!(
        "{}\n{}",
        keys(&[
            ("Select", lit("SPACE")),
            ("Register", lit("r")),
            ("Force", lit("R")),
            ("Filter", lit("u")),
        ]),
        RESULTS_LEGEND
    )
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
/// the Overview pane shows Refresh/Unregister; the Connections pane shows
/// Add/Edit/Delete/Test.
pub fn instance_workspace_footer_text(pane: crate::app_shell::nav::IwPane) -> String {
    match pane {
        crate::app_shell::nav::IwPane::Overview => {
            keys(&[("Refresh", lit("r")), ("Unregister", lit("u"))])
        }
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
