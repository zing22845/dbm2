//! Instance workspace feature update.

use super::msg::IwMessage;
use super::state::IwState;
use super::intent::IwIntent;
use super::effect::IwEffect;
use super::connections;
use super::overview;

/// Update the instance workspace state. Pure by-value transition: opening an
/// instance triggers the overview + connections loads; child messages are
/// forwarded to the matching sub-module (moved out, updated, moved back).
pub fn update(
    msg: IwMessage,
    mut state: IwState,
) -> (IwState, Vec<IwIntent>, Vec<IwEffect>, bool) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    let dirty = match msg {
        IwMessage::UnregisterInstance { instance } => {
            effects.push(IwEffect::UnregisterInstance { instance });
            false
        }
        IwMessage::Unregistered { instance } => {
            // The instance is gone; reset the workspace so the shell can show
            // the SQL workspace again. The shell also refreshes the explorer
            // tree and returns focus there.
            tracing::debug!(instance, "iw: instance unregistered, resetting workspace");
            state.instance_name.clear();
            state.overview = overview::state::OverviewState::default();
            state.connections = connections::state::ConnectionsState::default();
            true
        }
        IwMessage::OpenInstance { instance_name } => {
            let changed = state.instance_name != instance_name;
            state.instance_name = instance_name.clone();
            // Load the overview and connections for the freshly opened instance
            // by dispatching child Load messages.
            let (ov, _oi, oe, od) = overview::update::update(
                overview::msg::OverviewMessage::Load {
                    instance_name: instance_name.clone(),
                },
                std::mem::take(&mut state.overview),
            );
            state.overview = ov;
            effects.extend(oe.into_iter().map(IwEffect::Overview));
            let (cn, _ci, ce, cd) = connections::update::update(
                connections::msg::ConnectionsMessage::Load {
                    instance_name: instance_name.clone(),
                },
                std::mem::take(&mut state.connections),
            );
            state.connections = cn;
            effects.extend(ce.into_iter().map(IwEffect::Connections));
            changed || od || cd
        }
        IwMessage::Refresh { instance_name } => {
            // Refresh re-probes lifecycle and reloads both the overview data and
            // the connections, matching the original dbm. The 1s cooldown
            // (checked in the input layer) is set here so a held `r` does not
            // fire a refresh per auto-repeat. A held `r` repaints only when the
            // status actually changes (the first refresh shows "Refreshed");
            // later refreshes with an unchanged status do not redraw (no waste).
            use std::time::{Duration, Instant};
            let status_changed = state.status.as_deref() != Some("Refreshed");
            state.refresh_cooldown_until = Some(Instant::now() + Duration::from_secs(1));
            state.status = Some("Refreshed".into());
            effects.push(IwEffect::Refresh {
                instance_name: instance_name.clone(),
            });
            let (ov, _oi, oe, _od) = overview::update::update(
                overview::msg::OverviewMessage::Reload,
                std::mem::take(&mut state.overview),
            );
            state.overview = ov;
            effects.extend(oe.into_iter().map(IwEffect::Overview));
            let (cn, _ci, ce, _cd) = connections::update::update(
                connections::msg::ConnectionsMessage::Reload,
                std::mem::take(&mut state.connections),
            );
            state.connections = cn;
            effects.extend(ce.into_iter().map(IwEffect::Connections));
            status_changed
        }
        IwMessage::Overview(m) => {
            let overview::msg::OverviewMsg::Message(inner) = m;
            let s = std::mem::take(&mut state.overview);
            let (s, i, e, d) = overview::update::update(inner, s);
            state.overview = s;
            intents.extend(i.into_iter().map(IwIntent::Overview));
            effects.extend(e.into_iter().map(IwEffect::Overview));
            d
        }
        IwMessage::Connections(m) => {
            let connections::msg::ConnectionsMsg::Message(inner) = m;
            let s = std::mem::take(&mut state.connections);
            let (s, i, e, d) = connections::update::update(inner, s);
            state.connections = s;
            intents.extend(i.into_iter().map(IwIntent::Connections));
            effects.extend(e.into_iter().map(IwEffect::Connections));
            d
        }
    };
    (state, intents, effects, dirty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unregister_instance_emits_effect() {
        let state = IwState::default();
        let (_s, _i, effects, dirty) = update(
            IwMessage::UnregisterInstance { instance: "inst-a".into() },
            state,
        );
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            IwEffect::UnregisterInstance { instance } => {
                assert_eq!(instance, "inst-a");
            }
            other => panic!("expected UnregisterInstance effect, got {other:?}"),
        }
        assert!(!dirty, "unregister itself does not repaint locally");
    }

    #[test]
    fn unregistered_resets_workspace() {
        let mut state = IwState::default();
        state.instance_name = "inst-a".to_string();
        let (s, _i, _e, dirty) = update(
            IwMessage::Unregistered { instance: "inst-a".into() },
            state,
        );
        assert!(s.instance_name.is_empty(), "workspace reset after unregister");
        assert!(dirty);
    }

    #[test]
    fn refresh_sets_cooldown_status_and_reload_effects() {
        let mut state = IwState::default();
        state.instance_name = "inst-a".to_string();
        let (s, _i, effects, dirty) = update(
            IwMessage::Refresh {
                instance_name: "inst-a".into(),
            },
            state,
        );
        // Status set for the footer, cooldown armed, and a refresh repaint.
        assert_eq!(s.status.as_deref(), Some("Refreshed"));
        assert!(s.refresh_cooldown_until.is_some());
        assert!(dirty);
        // Effects: lifecycle probe + overview reload + connections reload.
        assert!(effects
            .iter()
            .any(|e| matches!(e, IwEffect::Refresh { instance_name } if instance_name == "inst-a")));
        assert!(effects.iter().any(|e| matches!(e, IwEffect::Overview(_))));
        assert!(effects
            .iter()
            .any(|e| matches!(e, IwEffect::Connections(_))));
    }
}
