//! Top-level keyboard dispatch: turn a raw `KeyEvent` (or a bracketed-paste
//! payload) into a feature message by the active focus pane.
//!
//! Part of the split keyboard layer; the entry points are
//! re-exported from the parent [`super`] module.

use super::discover::discover_key;
use super::explorer::explorer_key;
use super::header::header_key;
use super::iw::iw_key;
use super::modal::modal_key;
use super::nav::{explorer_width_nudge, pane_jump_from_key, switch_pane_by_dir, switch_subpane};
use super::sql::sql_key;
use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::app_shell::nav::DiscoverPane;
use crate::app_shell::nav::pane_dir_from_key;
use crate::app_shell::pane::Pane;
use crate::features::discover::msg::{DiscoverMessage, DiscoverMsg};
use crate::features::discover::targets::msg::{TargetsMessage, TargetsMsg};
use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
use crate::features::sql_workspace::sql_tab::editor::msg::{EditorMessage, EditorMsg};
use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
use crate::features::sql_workspace::sql_tab::state::SqlFocus;
use crossterm::event::KeyEvent;

/// Map a key to a feature message. A modal (if open) consumes all keys;
/// otherwise the active focus pane routes the key.
///
/// Returns `None` when nothing consumed the key (a no-op). Global shortcuts
/// (quit, theme toggle) are handled by the run loop and not routed here.
pub fn key_to_msg(key: KeyEvent, state: &AppState) -> Option<AppMsg> {
    // The discover parent pane owns all input while open, including Ctrl+nav
    // (which switches its engine / targets / results sub-panes) and Esc to
    // close it. Check it first so Ctrl+h/j/k/l never fall through to top-level
    // pane navigation while discover is focused.
    if let Pane::Discover(sub) = state.focus {
        return discover_key(key, sub, &state.discover);
    }
    // The platform copy chord is resolved by `copy_selection_msg` before pane
    // routing, mirroring the original dbm's global `copy_active_selection`: a
    // copy chord is *global* — it copies whichever editor holds a selection no
    // matter which sub-pane has focus — so it cannot live inside one feature's
    // key map. That helper is also the single place that enumerates the
    // candidate editors, so a new copyable pane is wired in one spot.
    if state.modal.is_none()
        && crate::common::utils::shortcuts::is_copy_shortcut(&key)
        && let Some(msg) = copy_selection_msg(state)
    {
        return Some(msg);
    }
    // Uppercase letter jumps (`[S]`/`[I]`/`[O]`/`[H]`/`[R]` in the pane titles)
    // move focus to the matching pane, mirroring the original dbm. They are
    // blocked while typing in the SQL editor (insert mode), so `S`/`H`/`R` do
    // not fire mid-edit; and never while a modal is open.
    if state.modal.is_none()
        && let Some(msg) = pane_jump_from_key(key, state)
    {
        return Some(msg);
    }
    // Pane navigation (Ctrl+h/j/k/l / Ctrl+arrows). Inside the SQL workspace a
    // move that stays within its sub-panes (editor / results / history) is
    // handled first — mirroring the original dbm's `resolve_move`, where the
    // workspace's own neighbor map wins and only a move off the workspace
    // boundary falls through to shell-level pane switching. So History → Left
    // lands on the editor, and only editor → Left leaves to the explorer.
    if state.modal.is_none()
        && let Some(dir) = pane_dir_from_key(&key)
    {
        if state.focus == Pane::SQLWorkspace
            && let Some(msg) = switch_subpane(dir, &state.sql)
        {
            return Some(msg);
        }
        if let Some(msg) = switch_pane_by_dir(
            dir,
            state.focus,
            state.instance_workspace_open(),
            state.explorer.pane,
        ) {
            return Some(msg);
        }
        return None;
    }
    match &state.modal {
        // Data-carrying popups: route their keys here (esc/n close, y/enter
        // confirms and dispatches the owning feature's action).
        Some(modal) => modal_key(key, modal, state),
        None => match state.focus {
            // Outside the SQL workspace, `[` / `]` resize the app-level
            // Explorer / workspace splitter. Inside the SQL workspace they
            // resize the history/detail splitters instead (handled in `sql_key`).
            Pane::Header => explorer_width_nudge(key, state).or_else(|| header_key(key)),
            Pane::Explorer(sub) => explorer_width_nudge(key, state)
                .or_else(|| explorer_key(key, sub, state.term_width)),
            Pane::InstanceWorkspace(sub) => {
                explorer_width_nudge(key, state).or_else(|| iw_key(key, sub, &state.iw))
            }
            Pane::SQLWorkspace => sql_key(key, &state.sql),
            // Discover is handled above (owns all input while open).
            Pane::Discover(_) => None,
        },
    }
}

/// The message for the platform copy chord, if some editor currently holds a
/// selection.
///
/// Candidates are probed in the order the original dbm's `copy_active_selection`
/// used. Today only the SQL workspace's editor is an `edtui` buffer with a
/// selection (the discover targets cell editor and the results detail pane have
/// none), so a new copyable pane is added here — one place, no per-feature key
/// maps to touch.
fn copy_selection_msg(state: &AppState) -> Option<AppMsg> {
    if state.focus != Pane::SQLWorkspace {
        return None;
    }
    let tab = state.sql.sql_tab.active_tab()?;
    if tab.editor.editor.selection.is_some() {
        return Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
            SqlTabMsg::Message(SqlTabMessage::Editor {
                tab_id: tab.session.id,
                msg: EditorMsg::Message(EditorMessage::CopySelection),
            }),
        ))));
    }
    // No SQL-editor selection: the results detail cell editor may hold one
    // (mirroring the original dbm, which probes the detail editor second).
    if tab
        .results
        .detail
        .editor
        .as_ref()
        .is_some_and(|host| host.editor.selection.is_some())
    {
        use crate::features::sql_workspace::sql_tab::results::msg::{ResultsMessage, ResultsMsg};
        return Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
            SqlTabMsg::Message(SqlTabMessage::Results {
                tab_id: tab.session.id,
                msg: ResultsMsg::Message(ResultsMessage::CopyDetailSelection),
            }),
        ))));
    }
    None
}

/// Route a bracketed-paste payload to the focused editor cell. The discover
/// targets editor (TSV host:ports rows or text into the in-progress cell) and
/// the SQL editor both accept pasted text; anything else is a no-op.
pub fn paste_to_msg(contents: &str, state: &AppState) -> Option<AppMsg> {
    if let Pane::Discover(sub) = state.focus {
        // Inside the discover parent pane: only the targets editor accepts paste.
        if sub == DiscoverPane::Targets {
            return Some(AppMsg::Discover(DiscoverMsg::Message(
                DiscoverMessage::Targets(TargetsMsg::Message(TargetsMessage::Paste(
                    contents.to_string(),
                ))),
            )));
        }
        return None;
    }
    if state.modal.is_some() {
        return None;
    }
    // SQL editor focused (no modal): paste into the active tab's buffer.
    let tab_id = state.sql.sql_tab.active_tab?;
    if state.focus == Pane::SQLWorkspace
        && state
            .sql
            .sql_tab
            .tabs
            .get(tab_id)
            .is_some_and(|t| t.focus == SqlFocus::Editor)
    {
        return Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
            SqlTabMsg::Message(SqlTabMessage::Editor {
                tab_id,
                msg: EditorMsg::Message(EditorMessage::Paste {
                    text: contents.to_string(),
                }),
            }),
        ))));
    }
    None
}

#[cfg(test)]
mod tests {

    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    /// An `AppState` with one open SQL tab (the default has none, since tabs
    /// are only created when a connection is selected). The tab is bound to a
    /// connection so it appears in the tab bar.
    fn app_state_with_tab() -> crate::app::state::AppState {
        let mut state = crate::app::state::AppState::default();
        state.sql.sql_tab.open_connection_tab(
            "local".into(),
            "main-db".into(),
            "c1".into(),
            None,
            None,
            None,
        );
        state
    }
    #[test]
    fn ctrl_j_without_control_is_not_a_pane_move() {
        let state = crate::app::state::AppState::default();
        // Plain 'j' is not a pane-move chord (no Ctrl), so it should not switch
        // the pane; the header key handler does not consume it either.
        assert!(key_to_msg(key(KeyCode::Char('j'), KeyModifiers::NONE), &state).is_none());
    }

    #[test]
    fn key_to_msg_discover_ctrl_j_switches_subpane_not_pane_nav() {
        // Regression: while discover owns focus, Ctrl+j must reach discover_key
        // (sub-pane switch) rather than be swallowed by top-level pane navigation.
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Discover(DiscoverPane::Engine);
        let msg = key_to_msg(key(KeyCode::Char('j'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+j while discover focused should switch sub-pane");
        assert!(matches!(
            msg,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Focus(
                DiscoverPane::Targets
            )))
        ));
    }

    #[test]
    fn copy_chord_without_selection_is_not_claimed() {
        // No selection → the global gate must not swallow the chord; it falls
        // through to the focused pane's own key map.
        let mut state = app_state_with_tab();
        state.focus = Pane::SQLWorkspace;
        state.sql.sql_tab.tabs[0].focus = SqlFocus::Editor;
        #[cfg(not(target_os = "macos"))]
        let copy = key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        #[cfg(target_os = "macos")]
        let copy = key(KeyCode::Char('c'), KeyModifiers::SUPER);
        let msg = key_to_msg(copy, &state);
        assert!(
            !matches!(
                msg,
                Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                    SqlTabMsg::Message(SqlTabMessage::Editor {
                        msg: EditorMsg::Message(EditorMessage::CopySelection),
                        ..
                    },)
                ))))
            ),
            "copy with no selection must not emit CopySelection"
        );
    }

    #[test]
    fn copy_chord_copies_editor_selection_from_any_subpane_focus() {
        let mut state = app_state_with_tab();
        state.focus = Pane::SQLWorkspace;
        // The mouse-made selection lives on the editor even while another
        // sub-pane (results) holds focus: the global copy gate must still copy.
        state.sql.sql_tab.tabs[0].focus = SqlFocus::Results;
        state.sql.sql_tab.tabs[0].editor.editor.selection = Some(edtui::Selection::new(
            edtui::Index2::new(0, 1),
            edtui::Index2::new(0, 3),
        ));
        #[cfg(not(target_os = "macos"))]
        let copy = key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        #[cfg(target_os = "macos")]
        let copy = key(KeyCode::Char('c'), KeyModifiers::SUPER);
        let msg = key_to_msg(copy, &state).expect("copy chord must be handled");
        assert!(
            matches!(
                msg,
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Editor {
                        msg: EditorMsg::Message(EditorMessage::CopySelection),
                        ..
                    },
                ))))
            ),
            "expected CopySelection, got {msg:?}"
        );
    }

    #[test]
    fn paste_routes_to_discover_targets_and_sql_editor() {
        // Discover targets focused inside the discover parent pane -> targets paste.
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Discover(DiscoverPane::Targets);
        let msg = paste_to_msg("1.2.3.4\t5432\n", &state).expect("targets paste should route");
        assert!(matches!(
            msg,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Targets(
                TargetsMsg::Message(TargetsMessage::Paste(_))
            )))
        ));

        // SQL editor focused (no modal) -> editor paste.
        let mut state = app_state_with_tab();
        state.focus = Pane::SQLWorkspace;
        state.sql.sql_tab.tabs[0].focus = SqlFocus::Editor;
        let msg = paste_to_msg("SELECT 1", &state).expect("editor paste should route");
        assert!(matches!(
            msg,
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg: EditorMsg::Message(EditorMessage::Paste { .. }),
                    ..
                }
            ))))
        ));

        // No focused paste target -> no-op.
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Header;
        assert!(paste_to_msg("x", &state).is_none());
    }
}
