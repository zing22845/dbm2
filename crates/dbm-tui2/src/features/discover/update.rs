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
            if focus != crate::app_shell::pane::DiscoverPane::Targets {
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
        DiscoverMessage::RegisterSelected => {
            let discovery_ids = state.results.selected_discovery_ids();
            if !discovery_ids.is_empty() {
                effects.push(DiscoverEffect::RegisterInstances { discovery_ids });
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
            false
        }
        DiscoverMessage::RegisterError { error } => {
            tracing::warn!("discover register failed: {error}");
            false
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
