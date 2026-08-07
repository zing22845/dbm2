//! Discover feature update.

use std::time::Duration;

use dbm_discovery::{DiscoveryConfig, DiscoveryTarget};
use dbm_discovery::parse_port_spec;

use super::msg::DiscoverMessage;
use super::state::DiscoverState;
use super::intent::DiscoverIntent;
use super::effect::DiscoverEffect;
use super::engine;
use super::results;
use super::targets;

/// Update the discover state. Pure by-value transition: close-confirmation and
/// scan bookkeeping are handled here; child messages are forwarded to the
/// matching sub-module (which is moved out, updated and moved back, so only the
/// touched sub-state is carried). Scan/register messages produce the matching
/// side-channel effect.
///
/// Focus for the discover child panes lives on the shell's `Pane::Discover`, so
/// it is updated there (in `app/update.rs`); this update only reacts to content.
pub fn update(
    msg: DiscoverMessage,
    mut state: DiscoverState,
) -> (DiscoverState, Vec<DiscoverIntent>, Vec<DiscoverEffect>, bool) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    let dirty = match msg {
        DiscoverMessage::Focus(focus) => {
            // Moving focus away from targets discards any in-progress edit.
            if focus != crate::app_shell::nav::DiscoverPane::Targets {
                let was_editing = state.targets.editing;
                state.targets.discard_edit();
                was_editing
            } else {
                false
            }
        }
        DiscoverMessage::RequestClose => {
            let changed = !state.close_confirm;
            state.close_confirm = true;
            changed
        }
        DiscoverMessage::CancelClose => {
            let changed = state.close_confirm;
            state.close_confirm = false;
            changed
        }
        DiscoverMessage::Close => {
            let changed = state.close_confirm;
            state.close_confirm = false;
            changed
        }
        DiscoverMessage::StartScan => {
            if let Some(config) = build_scan_config(&state) {
                effects.push(DiscoverEffect::StartScan { config });
                state.scanning = true;
                state.last_error = None;
                true
            } else {
                false
            }
        }
        DiscoverMessage::RegisterSelected { force } => {
            let discovery_ids = state.results.selected_discovery_ids();
            tracing::debug!(
                force,
                selected = discovery_ids.len(),
                discovery_ids = ?discovery_ids,
                "RegisterSelected: emitting RegisterInstances"
            );
            if !discovery_ids.is_empty() {
                effects.push(DiscoverEffect::RegisterInstances {
                    discovery_ids,
                    force,
                });
            } else {
                tracing::warn!(
                    "RegisterSelected: no selected instances, no effect emitted"
                );
            }
            false
        }
        DiscoverMessage::ScanProgress { .. } => {
            // Progress is purely informational; the next ScanComplete replaces
            // the results wholesale, so there is nothing to accumulate here.
            // Mark the scan as in-flight so the footer can show a live state.
            let changed = !state.scanning;
            state.scanning = true;
            changed
        }
        DiscoverMessage::ScanComplete { items } => {
            state.results.set_items(items);
            state.scanning = false;
            state.last_error = None;
            true
        }
        DiscoverMessage::ScanError { error } => {
            // Surface the failure on the results pane (a future phase may show
            // a status line); clear the stale results.
            state.results.set_items(Vec::new());
            state.scanning = false;
            tracing::warn!("discover scan failed: {error}");
            state.last_error = Some(error);
            true
        }
        DiscoverMessage::RegisterComplete { count } => {
            tracing::info!("registered {count} discovered instance(s)");
            state.register_message = Some(format!("registered {count} instance(s)"));
            true
        }
        DiscoverMessage::RegisterError { error } => {
            tracing::warn!("discover register failed: {error}");
            state.register_message = Some(format!("register failed: {error}"));
            true
        }
        DiscoverMessage::Engine(m) => {
            let engine::msg::EngineMsg::Message(inner) = m;
            let s = std::mem::take(&mut state.engine);
            let (s, i, _e, d) = engine::update::update(inner, s);
            state.engine = s;
            intents.extend(i.into_iter().map(DiscoverIntent::Engine));
            d
        }
        DiscoverMessage::Targets(m) => {
            let targets::msg::TargetsMsg::Message(inner) = m;
            let s = std::mem::take(&mut state.targets);
            let (s, i, _e, d) = targets::update::update(inner, s);
            state.targets = s;
            intents.extend(i.into_iter().map(DiscoverIntent::Targets));
            d
        }
        DiscoverMessage::Results(m) => {
            let results::msg::ResultsMsg::Message(inner) = m;
            let s = std::mem::take(&mut state.results);
            let (s, i, _e, d) = results::update::update(inner, s);
            state.results = s;
            intents.extend(i.into_iter().map(DiscoverIntent::Results));
            d
        }
    };
    (state, intents, effects, dirty)
}

/// Build a `DiscoveryConfig` from the current targets and engine. Returns
/// `None` when any target is invalid (empty host/ports), matching the "must be
/// complete" rule of the targets editor.
fn build_scan_config(state: &DiscoverState) -> Option<DiscoveryConfig> {
    let mut targets = Vec::new();
    for row in &state.targets.targets {
        let host = row.host.trim();
        if host.is_empty() {
            return None;
        }
        let ports = parse_port_spec(&row.ports_spec).ok()?;
        targets.push(DiscoveryTarget {
            host: host.to_string(),
            ports,
        });
    }
    if targets.is_empty() {
        return None;
    }
    Some(DiscoveryConfig {
        targets,
        hosts: Vec::new(),
        ports: Vec::new(),
        max_duration: Duration::from_secs(10),
        engine: state.engine.engine,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbm_core::Engine;
    use dbm_discovery::{Confidence, DiscoveredInstance, DiscoverySource, InstanceRunStatus};

    fn sample_instance(discovery_id: &str) -> DiscoveredInstance {
        DiscoveredInstance {
            discovery_id: discovery_id.into(),
            fingerprint: format!("fp-{discovery_id}"),
            engine: Engine::Postgres,
            host: "127.0.0.1".into(),
            port: 5432,
            socket_path: None,
            data_dir: None,
            systemd_unit: None,
            version: None,
            status: InstanceRunStatus::Running,
            sources: vec![DiscoverySource::Port],
            confidence: Confidence::Medium,
            already_registered: false,
            registered_instance_id: None,
            scanned_at: "now".into(),
        }
    }

    #[test]
    fn register_selected_emits_effect_with_selected_ids_and_force() {
        let mut state = DiscoverState::opened();
        state.results.items = vec![
            sample_instance("dsc-1"),
            sample_instance("dsc-2"),
            sample_instance("dsc-3"),
        ];
        // Select the first and third rows (cursor 0 and 2).
        state.results.selected = vec![0, 2];

        let (_state, _intents, effects, _dirty) = update(
            DiscoverMessage::RegisterSelected { force: true },
            state,
        );

        assert_eq!(effects.len(), 1);
        match &effects[0] {
            DiscoverEffect::RegisterInstances { discovery_ids, force } => {
                assert_eq!(discovery_ids, &["dsc-1".to_string(), "dsc-3".to_string()]);
                assert!(*force);
            }
            other => panic!("expected RegisterInstances effect, got {other:?}"),
        }
    }

    #[test]
    fn register_selected_with_no_selection_emits_no_effect() {
        let mut state = DiscoverState::opened();
        state.results.items = vec![sample_instance("dsc-1")];
        state.results.selected = Vec::new();

        let (_state, _intents, effects, _dirty) = update(
            DiscoverMessage::RegisterSelected { force: false },
            state,
        );

        assert!(effects.is_empty(), "no selection must not emit an effect");
    }

    #[test]
    fn start_scan_emits_scan_effect() {
        let state = DiscoverState::opened();
        let (_state, _intents, effects, _dirty) =
            update(DiscoverMessage::StartScan, state);
        assert_eq!(effects.len(), 1);
        assert!(
            matches!(&effects[0], DiscoverEffect::StartScan { config: _ }),
            "expected StartScan effect, got {:?}",
            effects[0]
        );
    }

    #[test]
    fn results_filter_defaults_to_unregistered_only() {
        let state = DiscoverState::opened();
        assert!(state.results.unregistered_only);
    }

    fn inst(id: &str, registered: bool) -> DiscoveredInstance {
        DiscoveredInstance {
            discovery_id: id.into(),
            fingerprint: format!("fp-{id}"),
            engine: Engine::Postgres,
            host: "127.0.0.1".into(),
            port: 5432,
            socket_path: None,
            data_dir: None,
            systemd_unit: None,
            version: None,
            status: InstanceRunStatus::Running,
            sources: vec![DiscoverySource::Port],
            confidence: Confidence::Medium,
            already_registered: registered,
            registered_instance_id: registered.then(|| "inst-1".to_string()),
            scanned_at: "now".into(),
        }
    }

    #[test]
    fn results_visible_indices_respect_filter() {
        let mut state = DiscoverState::opened();
        state.results.items = vec![inst("a", false), inst("b", true), inst("c", false)];

        // Default filter (unregistered_only) hides the registered one.
        assert!(state.results.unregistered_only);
        assert_eq!(state.results.visible_indices(), vec![0, 2]);
        assert_eq!(state.results.row_count(), 2);

        // Toggling off shows the full list.
        assert!(state.results.toggle_unregistered_filter());
        assert!(!state.results.unregistered_only);
        assert_eq!(state.results.visible_indices(), vec![0, 1, 2]);
        assert_eq!(state.results.row_count(), 3);

        // Toggling back on hides it again.
        assert!(state.results.toggle_unregistered_filter());
        assert_eq!(state.results.visible_indices(), vec![0, 2]);
    }

    #[test]
    fn toggle_select_uses_underlying_items_index() {
        let mut state = DiscoverState::opened();
        state.results.items = vec![inst("a", false), inst("b", true), inst("c", false)];
        // cursor at visible position 0 -> items index 0 ("a").
        assert!(state.results.toggle_select());
        assert_eq!(state.results.selected, vec![0]);
        assert_eq!(
            state.results.selected_discovery_ids(),
            vec!["a".to_string()]
        );

        // move to visible position 1 -> items index 2 ("c").
        assert!(state.results.move_down());
        assert!(state.results.toggle_select());
        assert_eq!(state.results.selected, vec![0, 2]);
        assert_eq!(
            state.results.selected_discovery_ids(),
            vec!["a".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn register_complete_records_message_and_is_dirty() {
        let state = DiscoverState::opened();
        let (state, _intents, _effects, dirty) =
            update(DiscoverMessage::RegisterComplete { count: 2 }, state);
        assert_eq!(state.register_message.as_deref(), Some("registered 2 instance(s)"));
        assert!(dirty);
    }

    #[test]
    fn register_error_records_message_and_is_dirty() {
        let state = DiscoverState::opened();
        let (state, _intents, _effects, dirty) = update(
            DiscoverMessage::RegisterError { error: "boom".into() },
            state,
        );
        assert_eq!(
            state.register_message.as_deref(),
            Some("register failed: boom")
        );
        assert!(dirty);
    }
}
