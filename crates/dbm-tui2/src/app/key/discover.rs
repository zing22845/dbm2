//! Discover parent-pane key bindings, routed by the close-confirm flag and
//! the active discover child pane (engine / targets / results).
//!
//! Part of the split keyboard layer; the entry points are
//! re-exported from the parent [`super`] module.

use super::modal::confirm_yes_no_key;
use crate::app::msg::AppMsg;
use crate::app_shell::nav::DiscoverPane;
use crate::app_shell::nav::{PaneDir, pane_dir_from_key};
use crate::features::discover::engine::msg::{EngineMessage, EngineMsg};
use crate::features::discover::msg::{DiscoverMessage, DiscoverMsg};
use crate::features::discover::results::msg::{ResultsMessage, ResultsMsg};
use crate::features::discover::state::DiscoverState;
use crate::features::discover::targets::msg::{TargetsMessage, TargetsMsg};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Discover key bindings, dispatched by the close-confirm flag and the active
/// discover child pane.
pub(crate) fn discover_key(
    key: KeyEvent,
    sub: DiscoverPane,
    state: &DiscoverState,
) -> Option<AppMsg> {
    let code = key.code;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // Close-confirmation uses the shared Yes/No confirm dialog: `y`/`Y`
    // confirms, `n`/`N` cancels — the same confirm shortcut as every other
    // confirm dialog (see `confirm_yes_no_key`).
    if state.close_confirm {
        return confirm_yes_no_key(
            key,
            || Some(discover(DiscoverMessage::Close)),
            || Some(discover(DiscoverMessage::CancelClose)),
        );
    }

    // While editing a target cell, route every key to the targets editor so
    // `Esc` cancels the edit instead of triggering the discover close-confirm
    // dialog, and text/navigation keys edit the cell rather than firing
    // discover-level actions.
    if sub == DiscoverPane::Targets && state.targets.editing {
        return targets_pane_key(key, state);
    }

    // Pane-move chords (Ctrl+nav) take precedence. The discover panes are
    // stacked vertically (engine / targets / results), so only Up/Down (j/k)
    // move between them — matching the footer hint. Left/Right (h/l) do not.
    // Non-navigation Ctrl chords (e.g. Ctrl+r = redo in the targets pane) must
    // fall through to the per-pane handler instead of being swallowed here.
    if ctrl && let Some(dir) = pane_dir_from_key(&key) {
        return match dir {
            PaneDir::Down => Some(discover(DiscoverMessage::Focus(sub.next()))),
            PaneDir::Up => Some(discover(DiscoverMessage::Focus(sub.prev()))),
            _ => None,
        };
    }

    match code {
        KeyCode::Esc => Some(discover(DiscoverMessage::RequestClose)),
        // While a scan is in flight, `c` cancels it (stops at the next host
        // boundary); this mirrors the original dbm's `c: cancel` hint.
        KeyCode::Char('c') if state.scanning => Some(discover(DiscoverMessage::CancelScan)),
        // Scan / register are discover-level actions available from any pane.
        // `r` registers normally (blocks on precheck warnings); `R` force-
        // registers (bypasses warnings, errors still block). Ctrl+r / Ctrl+Shift+r
        // are redo in the targets pane, so register only fires without Ctrl.
        KeyCode::Char('s') => Some(discover(DiscoverMessage::StartScan)),
        KeyCode::Char('r') if !ctrl => {
            tracing::debug!(pane = ?sub, "discover key: register (force=false)");
            Some(discover(DiscoverMessage::RegisterSelected { force: false }))
        }
        KeyCode::Char('R') if !ctrl => {
            tracing::debug!(pane = ?sub, "discover key: force-register (force=true)");
            Some(discover(DiscoverMessage::RegisterSelected { force: true }))
        }
        // Horizontal-splitter adjust (`+` / `-`): `+` grows the focused pane
        // (the targets editor on top or the results list below), `-` shrinks it.
        // Only meaningful while a splitter pane is focused; the engine selector
        // is above the splitter, so `+` / `-` there fall through. While a target
        // cell is being edited these are literal input, handled by the targets
        // pane above.
        KeyCode::Char('+') if sub != DiscoverPane::Engine => {
            Some(discover(DiscoverMessage::NudgeTargetsHeight {
                plus: true,
                top_focused: sub == DiscoverPane::Targets,
            }))
        }
        KeyCode::Char('-') if sub != DiscoverPane::Engine => {
            Some(discover(DiscoverMessage::NudgeTargetsHeight {
                plus: false,
                top_focused: sub == DiscoverPane::Targets,
            }))
        }
        _ => match sub {
            DiscoverPane::Engine => match code {
                // `e`/`Enter` would switch the engine if there were more than
                // one; today it is a no-op, so surface that note on the engine
                // footer instead of silently doing nothing.
                KeyCode::Enter | KeyCode::Char('e') => Some(discover(DiscoverMessage::Engine(
                    EngineMsg::Message(EngineMessage::ShowOnlyEngineNote),
                ))),
                _ => None,
            },
            DiscoverPane::Targets => targets_pane_key(key, state),
            DiscoverPane::Results => results_pane_key(key),
        },
    }
}

fn targets_pane_key(key: KeyEvent, state: &DiscoverState) -> Option<AppMsg> {
    let code = key.code;
    if state.targets.editing {
        return match code {
            KeyCode::Esc => Some(targets(TargetsMessage::CancelEdit)),
            KeyCode::Enter => Some(targets(TargetsMessage::CommitEdit)),
            KeyCode::Backspace => Some(targets(TargetsMessage::EditBackspace)),
            KeyCode::Left => Some(targets(TargetsMessage::EditCursorLeft)),
            KeyCode::Right => Some(targets(TargetsMessage::EditCursorRight)),
            KeyCode::Char(c) if !c.is_control() => Some(targets(TargetsMessage::EditChar(c))),
            _ => None,
        };
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match code {
        KeyCode::Char('i') | KeyCode::Enter => Some(targets(TargetsMessage::BeginEdit)),
        KeyCode::Char('o') => Some(targets(TargetsMessage::AddRow)),
        KeyCode::Char('d') | KeyCode::Delete => Some(targets(TargetsMessage::DeleteRow)),
        KeyCode::Char('u') => Some(targets(TargetsMessage::Undo)),
        KeyCode::Char('r') if ctrl => Some(targets(TargetsMessage::Redo)),
        KeyCode::Up | KeyCode::Char('k') => Some(targets(TargetsMessage::MoveUp)),
        KeyCode::Down | KeyCode::Char('j') => Some(targets(TargetsMessage::MoveDown)),
        KeyCode::Left | KeyCode::Char('h') => Some(targets(TargetsMessage::MoveColHost)),
        KeyCode::Right | KeyCode::Char('l') => Some(targets(TargetsMessage::MoveColPorts)),
        _ => None,
    }
}

fn results_pane_key(key: KeyEvent) -> Option<AppMsg> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(results(ResultsMessage::MoveUp)),
        KeyCode::Down | KeyCode::Char('j') => Some(results(ResultsMessage::MoveDown)),
        KeyCode::Char(' ') => Some(results(ResultsMessage::ToggleSelect)),
        KeyCode::Char('u') => Some(results(ResultsMessage::ToggleUnregisteredFilter)),
        _ => None,
    }
}

fn discover(msg: DiscoverMessage) -> AppMsg {
    AppMsg::Discover(DiscoverMsg::Message(msg))
}

fn targets(msg: TargetsMessage) -> AppMsg {
    AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Targets(
        TargetsMsg::Message(msg),
    )))
}

fn results(msg: ResultsMessage) -> AppMsg {
    AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Results(
        ResultsMsg::Message(msg),
    )))
}

#[cfg(test)]
mod tests {

    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }
    #[test]
    fn discover_ctrl_j_k_switches_pane_vertically() {
        use crate::features::discover::state::DiscoverState;
        let state = DiscoverState::opened();
        // ctrl+j (Down) moves Engine -> Targets.
        let down = discover_key(
            key(KeyCode::Char('j'), KeyModifiers::CONTROL),
            DiscoverPane::Engine,
            &state,
        )
        .expect("ctrl+j should switch discover pane");
        assert!(matches!(
            down,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Focus(
                DiscoverPane::Targets
            )))
        ));
        // ctrl+k (Up) from Engine wraps to Results.
        let up = discover_key(
            key(KeyCode::Char('k'), KeyModifiers::CONTROL),
            DiscoverPane::Engine,
            &state,
        )
        .expect("ctrl+k should switch discover pane");
        assert!(matches!(
            up,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Focus(
                DiscoverPane::Results
            )))
        ));
    }

    #[test]
    fn discover_engine_e_enter_shows_only_engine_note() {
        use crate::features::discover::engine::msg::{EngineMessage, EngineMsg};
        use crate::features::discover::state::DiscoverState;
        let state = DiscoverState::opened();
        // `e` on the engine pane is not a focus move (it is already focused);
        // it surfaces the "only Postgres" note instead of silently doing nothing.
        let e = discover_key(
            key(KeyCode::Char('e'), KeyModifiers::NONE),
            DiscoverPane::Engine,
            &state,
        )
        .expect("e on engine pane should dispatch the engine note");
        assert!(matches!(
            e,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Engine(
                EngineMsg::Message(EngineMessage::ShowOnlyEngineNote)
            )))
        ));
        // Enter behaves the same way.
        let enter = discover_key(
            key(KeyCode::Enter, KeyModifiers::NONE),
            DiscoverPane::Engine,
            &state,
        )
        .expect("Enter on engine pane should dispatch the engine note");
        assert!(matches!(
            enter,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Engine(
                EngineMsg::Message(EngineMessage::ShowOnlyEngineNote)
            )))
        ));
    }

    #[test]
    fn discover_ctrl_r_in_targets_redoes() {
        use crate::features::discover::state::DiscoverState;
        use crate::features::discover::targets::msg::{TargetsMessage, TargetsMsg};
        let state = DiscoverState::opened();
        // Ctrl+r must reach the targets pane's Redo, not be swallowed by the
        // discover Ctrl-navigation branch (non-nav Ctrl chords fall through).
        let msg = discover_key(
            key(KeyCode::Char('r'), KeyModifiers::CONTROL),
            DiscoverPane::Targets,
            &state,
        )
        .expect("ctrl+r in targets should dispatch Redo");
        assert!(matches!(
            msg,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Targets(
                TargetsMsg::Message(TargetsMessage::Redo)
            )))
        ));
    }

    #[test]
    fn discover_r_register_and_r_force_register() {
        use crate::features::discover::state::DiscoverState;
        let state = DiscoverState::opened();
        // `r` registers normally (no force).
        let reg = discover_key(
            key(KeyCode::Char('r'), KeyModifiers::NONE),
            DiscoverPane::Results,
            &state,
        )
        .expect("r should register");
        assert!(matches!(
            reg,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::RegisterSelected {
                force: false
            }))
        ));
        // `R` force-registers (bypasses warnings).
        let force = discover_key(
            key(KeyCode::Char('R'), KeyModifiers::NONE),
            DiscoverPane::Results,
            &state,
        )
        .expect("R should force-register");
        assert!(matches!(
            force,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::RegisterSelected {
                force: true
            }))
        ));
    }

    #[test]
    fn discover_c_cancels_scan_only_while_scanning() {
        use crate::features::discover::state::DiscoverState;
        let mut state = DiscoverState::opened();
        // Not scanning: `c` is not consumed by discover.
        assert!(
            discover_key(
                key(KeyCode::Char('c'), KeyModifiers::NONE),
                DiscoverPane::Results,
                &state
            )
            .is_none()
        );
        // Mark a scan in flight; `c` now cancels it.
        state.scanning = true;
        let cancel = discover_key(
            key(KeyCode::Char('c'), KeyModifiers::NONE),
            DiscoverPane::Results,
            &state,
        )
        .expect("c while scanning should cancel");
        assert!(matches!(
            cancel,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::CancelScan))
        ));
    }
}
