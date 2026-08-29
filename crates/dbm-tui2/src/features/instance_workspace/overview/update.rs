//! Instance overview feature update.

use super::msg::OverviewMessage;
use super::state::OverviewState;
use super::intent::OverviewIntent;
use super::effect::OverviewEffect;

/// Update the overview panel state. Pure by-value transition.
///
/// The returned `bool` is `dirty`: whether the rendered overview changed.
/// `Reload` only re-fetches (the later `Loaded` marks dirty); `Loaded` always
/// sets the instance.
pub fn update(
    msg: OverviewMessage,
    mut state: OverviewState,
) -> (OverviewState, Vec<OverviewIntent>, Vec<OverviewEffect>, bool) {
    match msg {
        OverviewMessage::Load { instance_name } => {
            let changed = state.instance_name != instance_name;
            state.instance_name = instance_name.clone();
            (
                state,
                Vec::new(),
                vec![OverviewEffect::LoadInstance { instance_name }],
                changed,
            )
        }
        OverviewMessage::Reload => {
            let instance_name = state.instance_name.clone();
            (
                state,
                Vec::new(),
                vec![OverviewEffect::LoadInstance { instance_name }],
                false,
            )
        }
        OverviewMessage::Loaded { instance } => {
            let dirty = state.instance.as_ref() != Some(instance.as_ref());
            state.instance = Some(*instance);
            (state, Vec::new(), Vec::new(), dirty)
        }
        OverviewMessage::MoveCursor(delta) => {
            // The overview has a fixed set of rows regardless of connection
            // count, so the row limit is the rows for a loaded instance.
            let len = state
                .instance
                .as_ref()
                .map_or(0, |i| super::view::overview_rows(i, 0).len());
            if len == 0 {
                return (state, Vec::new(), Vec::new(), false);
            }
            let prev = state.cursor;
            state.cursor = ((state.cursor as i64) + (delta as i64))
                .clamp(0, (len - 1) as i64) as usize;
            let dirty = state.cursor != prev;
            if dirty {
                state.scroll_locked = false;
            }
            (state, Vec::new(), Vec::new(), dirty)
        }
        OverviewMessage::SetVScroll { position } => {
            let len = state
                .instance
                .as_ref()
                .map_or(0, |i| super::view::overview_rows(i, 0).len());
            let prev = state.scroll;
            state.scroll = position.min(len.saturating_sub(1));
            state.scroll_locked = true;
            let dirty = state.scroll != prev;
            (state, Vec::new(), Vec::new(), dirty)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbm_core::Engine;
    use dbm_store::ManagedInstance;

    fn instance_state() -> OverviewState {
        let inst = ManagedInstance {
            id: "id-1".into(),
            fingerprint: "fp-1".into(),
            name: "postgres".into(),
            engine: Engine::Postgres,
            host: "127.0.0.1".into(),
            port: 5432,
            socket_path: None,
            data_dir: None,
            env_label: None,
            registered_at: "2026-01-01".into(),
            version_full: None,
            version_short: None,
            version_checked_at: None,
            lifecycle_status: None,
            lifecycle_checked_at: None,
            lifecycle_detail: None,
        };
        OverviewState {
            instance: Some(inst),
            ..Default::default()
        }
    }

    #[test]
    fn move_cursor_clamps_and_marks_dirty() {
        let state = instance_state();
        let (state, _i, _e, dirty) = update(OverviewMessage::MoveCursor(1), state);
        assert!(dirty);
        assert_eq!(state.cursor, 1);
        // Moving past the last row clamps to the last index.
        let (state, _i, _e, dirty) = update(OverviewMessage::MoveCursor(1000), state);
        assert!(dirty);
        assert_eq!(state.cursor, 16);
        // Moving up from the bottom back to the top is still dirty.
        let (state, _i, _e, dirty) = update(OverviewMessage::MoveCursor(-1000), state);
        assert!(dirty);
        assert_eq!(state.cursor, 0);
        // Moving up again at the top is a no-op -> not dirty.
        let (state, _i, _e, dirty) = update(OverviewMessage::MoveCursor(-1), state);
        assert!(!dirty);
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn move_cursor_without_instance_is_noop() {
        let state = OverviewState::default();
        let (state, _i, _e, dirty) = update(OverviewMessage::MoveCursor(1), state);
        assert!(!dirty);
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn loaded_identical_instance_does_not_repaint() {
        let inst = dbm_store::ManagedInstance {
            id: "id-1".into(),
            fingerprint: "fp-1".into(),
            name: "postgres".into(),
            engine: Engine::Postgres,
            host: "127.0.0.1".into(),
            port: 5432,
            socket_path: None,
            data_dir: None,
            env_label: None,
            registered_at: "2026-01-01".into(),
            version_full: None,
            version_short: None,
            version_checked_at: None,
            lifecycle_status: None,
            lifecycle_checked_at: None,
            lifecycle_detail: None,
        };
        let state = OverviewState {
            instance: Some(inst.clone()),
            ..Default::default()
        };
        // Re-loading the identical instance (refresh with unchanged data) must
        // NOT repaint — otherwise a held `r` redraws every second.
        let (state, _i, _e, dirty) = update(
            OverviewMessage::Loaded {
                instance: Box::new(inst),
            },
            state,
        );
        assert!(!dirty);
        assert!(state.instance.is_some());
    }
}
