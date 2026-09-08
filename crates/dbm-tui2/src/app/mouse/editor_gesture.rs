//! Shared plumbing for SQL-editor mouse text selection (Down/Drag/Up).
//!
//! The three gesture handlers live in separate files (`press.rs`, `drag.rs`),
//! so the common steps live here: run the event through edtui's own mouse
//! handler on a scratch editor copy (using the rendered hit area the run loop
//! fed back) and package the resulting cursor/mode/selection as an
//! [`EditorMessage::MouseGesture`] for the active tab.

use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};

use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
use crate::features::sql_workspace::sql_tab::editor::mouse;
use crate::features::sql_workspace::sql_tab::editor::msg::{EditorMessage, EditorMsg};
use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};

/// Build the mouse event edtui expects from a decoded gesture.
fn mouse_event(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::empty(),
    }
}

/// Decode a Down/Drag/Up gesture on the active tab's editor text and return
/// the message that applies it. `None` when there is no active tab or no
/// rendered editor hit region (e.g. the first frames before any draw).
///
/// Callers gate *Down* events to the editor text area first; Drag/Up run
/// unconditionally while `sql_editor_selecting` is set (edtui itself ignores
/// events that leave the text area, freezing the selection).
pub(super) fn editor_gesture_msg(
    state: &AppState,
    kind: MouseEventKind,
    x: u16,
    y: u16,
    double_click: bool,
) -> Option<AppMsg> {
    let tab = state.sql.sql_tab.active_tab()?;
    let hit = state.sql_editor_mouse_area?;
    let event = mouse_event(kind, x, y);
    let outcome = mouse::apply_mouse_event(
        &tab.editor.handler,
        &tab.editor.editor,
        &event,
        hit,
        double_click,
    );
    let tab_id = tab.session.id;
    Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
        SqlTabMsg::Message(SqlTabMessage::Editor {
            tab_id,
            msg: EditorMsg::Message(EditorMessage::MouseGesture { outcome }),
        }),
    ))))
}

/// Decode a Down/Drag/Up gesture on the active tab's **results detail cell
/// editor** (the second embedded edtui instance, live only while an edit
/// session is active and the editor has focus) and return the message that
/// applies it.
///
/// Same mechanics as [`editor_gesture_msg`] — edtui runs against a scratch copy
/// positioned with the rendered hit region — but the outcome travels as a
/// `DetailMessage::MouseGesture`, and `None` is returned when no detail editor
/// is focused (or before the first draw fed back its hit region).
pub(super) fn detail_gesture_msg(
    state: &AppState,
    kind: MouseEventKind,
    x: u16,
    y: u16,
    double_click: bool,
) -> Option<AppMsg> {
    use crate::features::sql_workspace::sql_tab::results::detail::msg::{DetailMessage, DetailMsg};
    use crate::features::sql_workspace::sql_tab::results::msg::{ResultsMessage, ResultsMsg};

    let tab = state.sql.sql_tab.active_tab()?;
    let host = tab.results.detail.editor.as_ref()?;
    let hit = state.results_detail_mouse_area?;
    let event = mouse_event(kind, x, y);
    let outcome = mouse::apply_mouse_event(&host.handler, &host.editor, &event, hit, double_click);
    let tab_id = tab.session.id;
    Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
        SqlTabMsg::Message(SqlTabMessage::Results {
            tab_id,
            msg: ResultsMsg::Message(ResultsMessage::Detail(DetailMsg::Message(
                DetailMessage::MouseGesture { outcome },
            ))),
        }),
    ))))
}
