//! Explorer instances/objects tree key bindings.
//!
//! Part of the split keyboard layer; the entry points are
//! re-exported from the parent [`super`] module.

use crate::app::msg::AppMsg;
use crate::app_shell::msg::ShellMsg;
use crate::app_shell::pane::Pane;
use crate::features::explorer::instances::msg::{InstancesMessage, InstancesMsg};
use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
use crate::features::explorer::objects::msg::{ObjectsMessage, ObjectsMsg};
use crate::features::explorer::state::ExplorerPane;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
/// Explorer key bindings, dispatched by the active explorer child pane `sub`.
/// `Tab` (and Ctrl+Up/Down via the shell) moves between the instances and
/// objects panes; that flows through a shell `FocusChanged` so the shell focus
/// and the explorer feature's sub-pane stay in sync.
pub(crate) fn explorer_key(key: KeyEvent, sub: ExplorerPane, term_width: u16) -> Option<AppMsg> {
    // `Tab` toggles between the instances and objects panes.
    if key.code == KeyCode::Tab {
        let next = match sub {
            ExplorerPane::Instances => ExplorerPane::Objects,
            ExplorerPane::Objects => ExplorerPane::Instances,
        };
        return Some(AppMsg::Shell(ShellMsg::FocusChanged {
            pane: Pane::Explorer(next),
        }));
    }
    // Horizontal-splitter adjust (`+` / `-`): `+` grows the focused pane (the
    // instances tree on top or the objects tree below), `-` shrinks it. `+`/`-`
    // are not used by the trees (they expand/collapse with `l`/`h`), so they are
    // safe to reserve for the splitter.
    if !key.modifiers.contains(KeyModifiers::CONTROL) {
        let plus = match key.code {
            KeyCode::Char('+') => Some(true),
            KeyCode::Char('-') => Some(false),
            _ => None,
        };
        if let Some(plus) = plus {
            return Some(explorer(ExplorerMessage::NudgeInstancesHeight { plus }));
        }
    }
    match sub {
        ExplorerPane::Instances => instances_key(key, term_width),
        ExplorerPane::Objects => objects_key(key, term_width),
    }
}

/// Objects pane keys: navigate the object tree. `l`/`Enter` toggle/expand the
/// cursor's row (or open/activate it), `h` collapses, and Left/Right scroll
/// horizontally — matching the original dbm.
fn objects_key(key: KeyEvent, term_width: u16) -> Option<AppMsg> {
    let msg = match key.code {
        KeyCode::Up | KeyCode::Char('k') => ObjectsMessage::MoveUp,
        KeyCode::Down | KeyCode::Char('j') => ObjectsMessage::MoveDown,
        // `l` expands the cursor's row (database, schema, or group), matching the
        // instances pane: it only expands, never collapses (`h` collapses).
        // Enter selects: it activates a schema or opens an object, and toggles
        // expandable rows.
        KeyCode::Char('l') => ObjectsMessage::Expand,
        KeyCode::Enter => ObjectsMessage::Select,
        KeyCode::Char('h') => ObjectsMessage::Collapse,
        KeyCode::Right => ObjectsMessage::ScrollHorizontal {
            delta: 1,
            term_width,
        },
        KeyCode::Left => ObjectsMessage::ScrollHorizontal {
            delta: -1,
            term_width,
        },
        _ => return None,
    };
    Some(explorer(ExplorerMessage::Objects(ObjectsMsg::Message(msg))))
}

/// Instances pane keys: navigate the connection tree. Expansion is bound to
/// `l`/`h` and Left/Right to horizontal scroll, matching the original dbm.
fn instances_key(key: KeyEvent, term_width: u16) -> Option<AppMsg> {
    let msg = match key.code {
        KeyCode::Up | KeyCode::Char('k') => InstancesMessage::MoveUp,
        KeyCode::Down | KeyCode::Char('j') => InstancesMessage::MoveDown,
        // Enter on a connection focuses its already-open tab (or opens one).
        KeyCode::Enter => InstancesMessage::Select,
        // `n` on a connection always opens a fresh SQL editor.
        KeyCode::Char('n') => InstancesMessage::NewConnectionTab,
        KeyCode::Char('l') => InstancesMessage::Expand,
        KeyCode::Char('h') => InstancesMessage::Collapse,
        KeyCode::Char('a') => InstancesMessage::AddConnection,
        KeyCode::Char('i') => InstancesMessage::EditConnection,
        KeyCode::Right => InstancesMessage::ScrollHorizontal {
            delta: 1,
            term_width,
        },
        KeyCode::Left => InstancesMessage::ScrollHorizontal {
            delta: -1,
            term_width,
        },
        _ => return None,
    };
    Some(explorer(ExplorerMessage::Instances(InstancesMsg::Message(
        msg,
    ))))
}

fn explorer(msg: ExplorerMessage) -> AppMsg {
    AppMsg::Explorer(ExplorerMsg::Message(msg))
}

#[cfg(test)]
mod tests {

    use super::*;

    use crate::app::key::key_to_msg;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }
    #[test]
    fn explorer_instances_l_h_expand_collapse_and_arrows_scroll() {
        use crate::features::explorer::instances::msg::InstancesMessage;
        use crate::features::explorer::msg::ExplorerMessage;

        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::Instances);

        // `l` expands, `h` collapses, arrows scroll, Enter selects, `n` opens a
        // fresh editor (original dbm bindings).
        for (code, expect) in [
            (KeyCode::Char('l'), InstancesMessage::Expand),
            (KeyCode::Char('h'), InstancesMessage::Collapse),
            (
                KeyCode::Right,
                InstancesMessage::ScrollHorizontal {
                    delta: 1,
                    term_width: 0,
                },
            ),
            (
                KeyCode::Left,
                InstancesMessage::ScrollHorizontal {
                    delta: -1,
                    term_width: 0,
                },
            ),
            (KeyCode::Enter, InstancesMessage::Select),
            (KeyCode::Char('n'), InstancesMessage::NewConnectionTab),
        ] {
            let msg = key_to_msg(key(code, KeyModifiers::NONE), &state).expect("explorer key");
            let got = match msg {
                AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
                    InstancesMsg::Message(m),
                ))) => m,
                other => panic!("unexpected msg for {code:?}: {other:?}"),
            };
            let match_kind = matches!(
                (&got, &expect),
                (InstancesMessage::Expand, InstancesMessage::Expand)
                    | (InstancesMessage::Collapse, InstancesMessage::Collapse)
                    | (
                        InstancesMessage::ScrollHorizontal { .. },
                        InstancesMessage::ScrollHorizontal { .. }
                    )
                    | (InstancesMessage::Select, InstancesMessage::Select)
                    | (
                        InstancesMessage::NewConnectionTab,
                        InstancesMessage::NewConnectionTab
                    )
            );
            assert!(
                match_kind,
                "for {code:?}: got {got:?}, expected kind {expect:?}"
            );
        }
    }

    #[test]
    fn explorer_objects_h_collapses_and_arrows_scroll() {
        use crate::features::explorer::msg::ExplorerMessage;
        use crate::features::explorer::objects::msg::ObjectsMessage;

        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::Objects);

        for (code, expect) in [
            (KeyCode::Char('l'), ObjectsMessage::Expand),
            (KeyCode::Char('h'), ObjectsMessage::Collapse),
            (
                KeyCode::Right,
                ObjectsMessage::ScrollHorizontal {
                    delta: 1,
                    term_width: 0,
                },
            ),
            (
                KeyCode::Left,
                ObjectsMessage::ScrollHorizontal {
                    delta: -1,
                    term_width: 0,
                },
            ),
            (KeyCode::Enter, ObjectsMessage::Select),
        ] {
            let msg = key_to_msg(key(code, KeyModifiers::NONE), &state).expect("explorer key");
            let got = match msg {
                AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Objects(
                    ObjectsMsg::Message(m),
                ))) => m,
                other => panic!("unexpected msg for {code:?}: {other:?}"),
            };
            // Compare discriminant (messages carry payload).
            let match_kind = matches!(
                (&got, &expect),
                (ObjectsMessage::Collapse, ObjectsMessage::Collapse)
                    | (
                        ObjectsMessage::ScrollHorizontal { .. },
                        ObjectsMessage::ScrollHorizontal { .. }
                    )
                    | (ObjectsMessage::Select, ObjectsMessage::Select)
                    | (ObjectsMessage::Expand, ObjectsMessage::Expand)
            );
            assert!(
                match_kind,
                "for {code:?}: got {got:?}, expected kind {expect:?}"
            );
        }
    }
}
