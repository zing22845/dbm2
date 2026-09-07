//! Mouse-gesture decoding for the SQL editor.
//!
//! edtui's own mouse handler performs the (wrap-aware) terminal→buffer
//! coordinate conversion, but it reads the editor's `screen_area` + viewport,
//! which its renderer only refreshes on the `&mut` editor it draws. dbm2
//! renders a *copy* of the editor each frame to keep the render pass pure, so
//! the live editor's internal view is never current.
//!
//! To reuse edtui's conversion without duplicating it, this module runs the
//! event against a **scratch copy** of the editor with the rendered hit area
//! fed in (see [`crate::common::editor::EditorMouseHitArea`]), then carries the
//! resulting cursor/mode/selection up to `update` as a message. The pointer
//! layer (which owns the hit geometry) never mutates the real editor, and
//! `update` never needs terminal geometry — it just applies the outcome.

use crossterm::event::{MouseEvent, MouseEventKind};
use edtui::{EditorEventHandler, EditorMode, Index2, Selection};

use crate::common::editor::{EditorMouseHitArea, fix_insert_mode_click_cursor};

/// The parts of an editor's state a mouse Down/Drag/Up can change. Derived
/// straight from a scratch copy after edtui handled the event, so the real
/// editor lands in exactly the state edtui would have produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseGestureOutcome {
    pub cursor: Index2,
    pub mode: EditorMode,
    pub selection: Option<Selection>,
}

/// Run one mouse event through edtui's handler on a scratch copy of `editor`,
/// positioned with the rendered [`EditorMouseHitArea`].
///
/// Events outside the text area are ignored by edtui (its handler bounds-checks
/// every event), so a drag that leaves the editor freezes the selection and an
/// `Up` outside keeps the last in-area position — matching the original dbm.
///
/// `double_click` applies the word-select emulation after a Down (edtui has no
/// native double-click word select; mirrors the original dbm's
/// `handle_editor_like_mouse`).
#[must_use]
pub fn apply_mouse_event(
    handler: &EditorEventHandler,
    editor: &edtui::EditorState,
    event: &MouseEvent,
    hit: EditorMouseHitArea,
    double_click: bool,
) -> MouseGestureOutcome {
    let mut probe = editor.clone();
    probe.set_mouse_screen_area(hit.text_area);
    let (x, _) = probe.viewport_offset();
    probe.set_viewport_offset(x, hit.viewport_y);
    handler.on_mouse_event(*event, &mut probe);
    // Insert-mode click placement: clicking right of the end of a line must
    // land the cursor after the last char (so appending text works), which
    // edtui's own clamping alone does not provide.
    fix_insert_mode_click_cursor(&mut probe, event, hit.text_area);
    if double_click && matches!(event.kind, MouseEventKind::Down(_)) {
        probe.execute(edtui::actions::SelectInnerWord);
        probe.mode = EditorMode::Visual;
    }
    MouseGestureOutcome {
        cursor: probe.cursor,
        mode: probe.mode,
        selection: probe.selection,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::editor::new_editor;
    use crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::layout::Rect;

    const HIT: EditorMouseHitArea = EditorMouseHitArea {
        text_area: Rect {
            x: 4,
            y: 2,
            width: 60,
            height: 10,
        },
        viewport_y: 0,
    };

    fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: crossterm::event::KeyModifiers::empty(),
        }
    }

    #[test]
    fn click_places_cursor_at_column() {
        let editor = new_editor("select 1");
        let handler = crate::common::editor::new_editor_handler();
        // "select 1": click on the cell of char col 2 ('l').
        let outcome = apply_mouse_event(
            &handler,
            &editor,
            &mouse(
                MouseEventKind::Down(MouseButton::Left),
                HIT.text_area.x + 2,
                HIT.text_area.y + 1,
            ),
            HIT,
            false,
        );
        assert_eq!(outcome.cursor, Index2::new(0, 2));
        assert!(outcome.selection.is_none());
    }

    #[test]
    fn drag_selects_the_range_between_click_and_drag() {
        let editor = new_editor("select 1");
        let handler = crate::common::editor::new_editor_handler();

        // Progress the same way the real flow does: Down first, then Drag.
        let mut scratch = editor.clone();
        scratch.set_mouse_screen_area(HIT.text_area);
        let (x, _) = scratch.viewport_offset();
        scratch.set_viewport_offset(x, HIT.viewport_y);
        handler.on_mouse_event(
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                HIT.text_area.x + 2,
                HIT.text_area.y + 1,
            ),
            &mut scratch,
        );
        assert_eq!(scratch.cursor, Index2::new(0, 2));

        let outcome = apply_mouse_event(
            &handler,
            &scratch,
            &mouse(
                MouseEventKind::Drag(MouseButton::Left),
                HIT.text_area.x + 5,
                HIT.text_area.y + 1,
            ),
            HIT,
            false,
        );
        assert_eq!(outcome.mode, EditorMode::Visual, "drag must enter Visual");
        let sel = outcome.selection.expect("drag must create a selection");
        assert_eq!(sel.start, Index2::new(0, 2), "anchor stays at the click");
        assert_eq!(
            sel.end,
            Index2::new(0, 5),
            "selection end follows the pointer"
        );
        let text = sel.copy_from(&editor.lines).to_string();
        assert_eq!(text, "lect", "copies the buffer chars between the columns");
    }

    #[test]
    fn release_keeps_the_visual_selection() {
        let editor = new_editor("select 1");
        let handler = crate::common::editor::new_editor_handler();
        let mut scratch = editor.clone();
        scratch.set_mouse_screen_area(HIT.text_area);
        let (x, _) = scratch.viewport_offset();
        scratch.set_viewport_offset(x, HIT.viewport_y);
        handler.on_mouse_event(
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                HIT.text_area.x + 2,
                HIT.text_area.y + 1,
            ),
            &mut scratch,
        );
        handler.on_mouse_event(
            mouse(
                MouseEventKind::Drag(MouseButton::Left),
                HIT.text_area.x + 5,
                HIT.text_area.y + 1,
            ),
            &mut scratch,
        );
        let outcome = apply_mouse_event(
            &handler,
            &scratch,
            &mouse(
                MouseEventKind::Up(MouseButton::Left),
                HIT.text_area.x + 5,
                HIT.text_area.y + 1,
            ),
            HIT,
            false,
        );
        assert_eq!(outcome.mode, EditorMode::Visual);
        assert!(
            outcome.selection.is_some(),
            "release must keep the selection for copy"
        );
    }
}
