//! Discover feature update.

use std::time::Duration;

use dbm_discovery::{DiscoveryConfig, DiscoveryTarget};
use dbm_discovery::parse_port_spec;

use super::msg::DiscoverMessage;
use super::state::{DiscoverFocus, DiscoverState};
use super::intent::DiscoverIntent;
use super::effect::DiscoverEffect;
use super::engine;
use super::results;
use super::targets;

/// Update the discover state. Pure by-value transition: pane navigation and
/// close-confirmation are handled here; child messages are forwarded to the
/// matching sub-module (which is moved out, updated and moved back, so only the
/// touched sub-state is carried). Scan/register messages produce the matching
/// side-channel effect.
pub fn update(
    msg: DiscoverMessage,
    mut state: DiscoverState,
) -> (DiscoverState, Vec<DiscoverIntent>, Vec<DiscoverEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    match msg {
        DiscoverMessage::Focus(focus) => {
            // Moving focus away from targets discards any in-progress edit.
            if focus != DiscoverFocus::Targets {
                state.targets.discard_edit();
            }
            state.focus = focus;
        }
        DiscoverMessage::RequestClose => state.close_confirm = true,
        DiscoverMessage::CancelClose => state.close_confirm = false,
        DiscoverMessage::Close => {
            state.close_confirm = false;
        }
        DiscoverMessage::StartScan => {
            if let Some(config) = build_scan_config(&state) {
                effects.push(DiscoverEffect::StartScan { config });
                // Move focus to results so progress is visible while scanning.
                state.focus = DiscoverFocus::Results;
                state.scanning = true;
                state.last_error = None;
            }
        }
        DiscoverMessage::RegisterSelected => {
            let discovery_ids = state.results.selected_discovery_ids();
            if !discovery_ids.is_empty() {
                effects.push(DiscoverEffect::RegisterInstances { discovery_ids });
            }
        }
        DiscoverMessage::ScanProgress { .. } => {
            // Progress is purely informational; the next ScanComplete replaces
            // the results wholesale, so there is nothing to accumulate here.
            // Mark the scan as in-flight so the footer can show a live state.
            state.scanning = true;
        }
        DiscoverMessage::ScanComplete { items } => {
            state.results.set_items(items);
            state.scanning = false;
            state.last_error = None;
        }
        DiscoverMessage::ScanError { error } => {
            // Surface the failure on the results pane (a future phase may show
            // a status line); clear the stale results.
            state.results.set_items(Vec::new());
            state.scanning = false;
            tracing::warn!("discover scan failed: {error}");
            state.last_error = Some(error);
        }
        DiscoverMessage::RegisterComplete { count } => {
            tracing::info!("registered {count} discovered instance(s)");
        }
        DiscoverMessage::RegisterError { error } => {
            tracing::warn!("discover register failed: {error}");
        }
        DiscoverMessage::Engine(m) => {
            let engine::msg::EngineMsg::Message(inner) = m;
            let s = std::mem::take(&mut state.engine);
            let (s, i, _e) = engine::update::update(inner, s);
            state.engine = s;
            intents.extend(i.into_iter().map(DiscoverIntent::Engine));
        }
        DiscoverMessage::Targets(m) => {
            let targets::msg::TargetsMsg::Message(inner) = m;
            let s = std::mem::take(&mut state.targets);
            let (s, i, _e) = targets::update::update(inner, s);
            state.targets = s;
            intents.extend(i.into_iter().map(DiscoverIntent::Targets));
        }
        DiscoverMessage::Results(m) => {
            let results::msg::ResultsMsg::Message(inner) = m;
            let s = std::mem::take(&mut state.results);
            let (s, i, _e) = results::update::update(inner, s);
            state.results = s;
            intents.extend(i.into_iter().map(DiscoverIntent::Results));
        }
    }
    (state, intents, effects)
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
