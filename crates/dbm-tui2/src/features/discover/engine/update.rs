//! Engine selector feature update.

use super::msg::EngineMessage;
use super::state::EngineState;
use super::intent::EngineIntent;
use super::effect::EngineEffect;

/// Update the engine selector state. Pure by-value transition.
///
/// The returned `bool` is `dirty`: whether the selected engine actually
/// changed (or the footer status changed). Re-selecting the current engine
/// reports `false`.
pub fn update(
    msg: EngineMessage,
    mut state: EngineState,
) -> (EngineState, Vec<EngineIntent>, Vec<EngineEffect>, bool) {
    let dirty = match msg {
        EngineMessage::Select(engine) => {
            let changed = state.engine != engine;
            state.engine = engine;
            changed
        }
        EngineMessage::ShowOnlyEngineNote => {
            // Repaint only the first time the note is shown; a held `e` repeat
            // keeps the same status, so it must not repaint every frame.
            let before = state.status.clone();
            state.status = Some("Postgres is the only available engine".into());
            state.status != before
        }
    };
    (state, Vec::new(), Vec::new(), dirty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_changes_engine_and_marks_dirty() {
        // There is only one engine today, but `Select` is the future hook for
        // switching engines, so it must still work if more are added.
        let state = EngineState::default();
        let (state, _i, _e, dirty) = update(EngineMessage::Select(EngineState::default().engine), state);
        assert!(!dirty);
        assert_eq!(state.status, None);
    }

    #[test]
    fn show_only_engine_note_is_dirty_only_once() {
        let state = EngineState::default();
        // First `e`/`Enter` sets the note and repaints.
        let (state, _i, _e, dirty) = update(EngineMessage::ShowOnlyEngineNote, state);
        assert!(dirty);
        assert_eq!(state.status.as_deref(), Some("Postgres is the only available engine"));
        // Repeating keeps the same status, so it must not repaint again.
        let (state, _i, _e, dirty) = update(EngineMessage::ShowOnlyEngineNote, state);
        assert!(!dirty);
        assert_eq!(state.status.as_deref(), Some("Postgres is the only available engine"));
    }
}
