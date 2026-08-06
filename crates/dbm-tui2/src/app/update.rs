//! Central update dispatcher.
//!
//! `update()` is the heart of the central message router. It takes the
//! global `AppMsg`, mutates the relevant feature state, and collects the
//! `Intent`s and `Effect`s produced by the feature. Child intents/effects
//! are boxed (erasing their concrete feature type) so they can be routed
//! uniformly; the router will later convert them back into `AppMsg` /
//! `Action`.

use crate::app::action::Action;
use crate::app::msg::AppMsg;
use crate::app::state::{AppState, ModalKind};
use crate::app_shell::effect::ErasedEffect;
use crate::app_shell::focus::FocusZone;
use crate::app_shell::intent::RoutableIntent;
use crate::features::discover::msg::{DiscoverMessage, DiscoverMsg};
use crate::features::discover::update::update as discover_update;
use crate::features::explorer::intent::ExplorerIntent;
use crate::features::explorer::msg::ExplorerMsg;
use crate::features::explorer::update::update as explorer_update;
use crate::features::global_footer::msg::FooterMsg;
use crate::features::global_footer::update::update as footer_update;
use crate::features::header::msg::{HeaderMessage, HeaderMsg};
use crate::features::header::update::update as header_update;
use crate::features::instance_workspace::msg::IwMsg;
use crate::features::instance_workspace::update::update as iw_update;
use crate::features::perf_monitor::msg::PerfMsg;
use crate::features::perf_monitor::update::update as perf_update;
use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
use crate::features::sql_workspace::update::update as sql_workspace_update;

/// Result of a single update pass: side-channel intents and effects.
#[derive(Default)]
pub struct UpdateResult {
    /// Intents to be routed into the message router.
    pub intents: Vec<Box<dyn RoutableIntent<AppMsg>>>,
    /// Effects to be executed by the effect runner.
    pub effects: Vec<Box<dyn ErasedEffect<Action>>>,
    /// Messages to be enqueued for a later pass (shell-level orchestration).
    pub pending: std::collections::VecDeque<AppMsg>,
}

impl UpdateResult {
    fn new() -> Self {
        Self::default()
    }
}

/// Box an intent, erasing its concrete type into the global message router.
fn box_intent(i: impl RoutableIntent<AppMsg> + 'static) -> Box<dyn RoutableIntent<AppMsg>> {
    Box::new(i)
}

/// Box an effect, erasing its concrete action type into the global `Action`.
fn box_effect(e: impl ErasedEffect<Action> + 'static) -> Box<dyn ErasedEffect<Action>> {
    Box::new(e)
}

/// Map a feature message to the `FocusZone` that must be active for its
/// keyboard input to be accepted. Shell and footer messages are always
/// handled, so they map to `None`.
///
/// Layout relationship: `Discover`, `Sql` and `Perf` all live inside the
/// main workspace region and therefore share the `SQLWorkspace` focus zone.
/// `Iw` (instance workspace) occupies its own `InstanceWorkspace` zone. If a
/// future layout moves `Discover` into a popup or a separate pane, adjust its
/// mapping here.
fn focus_zone_of(msg: &AppMsg) -> Option<FocusZone> {
    match msg {
        AppMsg::Shell(_) | AppMsg::Footer(_) => None,
        AppMsg::Header(_) => Some(FocusZone::Header),
        AppMsg::Explorer(_) => Some(FocusZone::Explorer),
        AppMsg::Discover(_) => Some(FocusZone::SQLWorkspace),
        AppMsg::Iw(_) => Some(FocusZone::InstanceWorkspace),
        AppMsg::Sql(_) => Some(FocusZone::SQLWorkspace),
        AppMsg::Perf(_) => Some(FocusZone::SQLWorkspace),
    }
}

/// Open the discover modal: reset its state and show it as the active modal.
fn open_discover(state: &mut AppState) {
    tracing::debug!("open_discover: setting modal to Discover");
    state.discover = crate::features::discover::state::DiscoverState::opened();
    state.modal = Some(ModalKind::Discover);
}

/// Close the discover modal.
fn close_discover(state: &mut AppState) {
    state.modal = None;
}

/// Apply a message to the global state, returning side-channel intents and
/// effects. Feature messages are only dispatched to their update when the
/// active focus zone permits keyboard input for them; shell and footer
/// messages always pass through.
pub fn update(msg: AppMsg, state: &mut AppState) -> UpdateResult {
    // When a modal is open it owns all keyboard input, so its messages bypass
    // the focus guard.
    if state.modal.is_some() && matches!(msg, AppMsg::Discover(_)) {
        return update_unchecked(msg, state);
    }
    if let Some(zone) = focus_zone_of(&msg)
        && zone != state.focus
    {
        // The message targets a region that does not currently own the
        // keyboard input, so it is dropped. This prevents unfocused
        // features from reacting to stray input.
        return UpdateResult::new();
    }
    update_unchecked(msg, state)
}

/// Apply a message regardless of the current focus zone. Used by the effect
/// action dispatcher and intent router, where delivery is programmatic and
/// must not be gated by focus.
pub fn update_unchecked(msg: AppMsg, state: &mut AppState) -> UpdateResult {
    let mut result = UpdateResult::new();
    // Each feature's envelope (`XMsg`) is intentionally single-variant:
    // `XMsg::Message(inner)`. Concrete events live in the inner `XMessage`
    // enum (which may have many variants) and are dispatched inside that
    // feature's own `update`. This keeps the outer envelope stable so the
    // `let XMsg::Message(inner) = m` destructuring below is irrefutable.
    // Do NOT add variants to `XMsg`; extend `XMessage` instead.
    match msg {
        AppMsg::Shell(shell_msg) => match shell_msg {
            crate::app_shell::msg::ShellMsg::Quit => {
                state.should_quit = true;
            }
            crate::app_shell::msg::ShellMsg::Tick => {}
            crate::app_shell::msg::ShellMsg::FocusChanged { zone } => {
                state.focus = zone;
            }
            crate::app_shell::msg::ShellMsg::ToggleTheme => {
                // Flip between the theme's dark and light palettes; the next
                // frame is drawn with the new palette automatically.
                state.theme.toggle();
            }
        },
        AppMsg::Header(m) => {
            // Opening a modal is shell orchestration, handled before the
            // header feature's own update so the modal state is ready for the
            // frame that follows.
            if let HeaderMsg::Message(HeaderMessage::Activate) = &m
                && state.header.button == 0 {
                    open_discover(state);
                }
            let HeaderMsg::Message(inner) = m;
            // The header feature's update is a pure by-value transition: move
            // the state out, update it, move the result back. No deep clone.
            let header = std::mem::take(&mut state.header);
            let (s, intents, effects) = header_update(inner, header);
            state.header = s;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        AppMsg::Explorer(m) => {
            let ExplorerMsg::Message(inner) = m;
            // The explorer feature's update is a pure by-value transition: move
            // the state out, update it, move the result back. No deep clone.
            let explorer = std::mem::take(&mut state.explorer);
            let (s, intents, effects) = explorer_update(inner, explorer);
            state.explorer = s;
            // Cross-feature: selecting an instance in the explorer opens the
            // instance workspace for it. This is shell-level orchestration that
            // dispatches an iw message based on the explorer's intent.
            for intent in &intents {
                if let ExplorerIntent::Instances(
                    crate::features::explorer::instances::intent::InstancesIntent::OpenInstanceWorkspace { instance_idx },
                ) = intent
                {
                    let instance_name = state
                        .explorer
                        .instances
                        .nodes
                        .get(*instance_idx)
                        .and_then(|n| n.instance.as_ref())
                        .map(|i| i.name.clone())
                        .unwrap_or_default();
                    if !instance_name.is_empty() {
                        let iw = std::mem::take(&mut state.iw);
                        let (iw2, i, e) = iw_update(
                            crate::features::instance_workspace::msg::IwMessage::OpenInstance {
                                instance_name,
                            },
                            iw,
                        );
                        state.iw = iw2;
                        result.intents.extend(i.into_iter().map(box_intent));
                        result.effects.extend(e.into_iter().map(box_effect));
                    }
                }
                if let ExplorerIntent::Instances(
                    crate::features::explorer::instances::intent::InstancesIntent::OpenConnectionWorkspace {
                        instance_idx,
                        connection_idx,
                    },
                ) = intent
                {
                    let node = state.explorer.instances.nodes.get(*instance_idx);
                    if let Some(node) = node {
                        let instance_name = node
                            .instance
                            .as_ref()
                            .map(|i| i.name.clone())
                            .unwrap_or_default();
                        if let Some(conn) = node.connections.get(*connection_idx) {
                            let sql_msg = SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                                SqlTabMessage::OpenConnectionTab {
                                    instance: instance_name,
                                    connection: conn.name.clone(),
                                    connection_id: conn.id.clone(),
                                    database: None,
                                    schema: None,
                                },
                            )));
                            result.pending.push_back(AppMsg::Sql(sql_msg));
                        }
                    }
                }
            }
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        AppMsg::Discover(m) => {
            // Closing the modal is shell orchestration, handled after the
            // discover feature's own update so the frame is ready for teardown.
            let should_close = matches!(&m, DiscoverMsg::Message(DiscoverMessage::Close));
            let DiscoverMsg::Message(inner) = m;
            // The discover feature's update is a pure by-value transition: move
            // the state out, update it, move the result back. No deep clone.
            let discover = std::mem::take(&mut state.discover);
            let (s, intents, effects) = discover_update(inner, discover);
            state.discover = s;
            if should_close {
                close_discover(state);
            }
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        AppMsg::Iw(m) => {
            let IwMsg::Message(inner) = m;
            // The instance workspace feature's update is a pure by-value
            // transition: move the state out, update it, move the result back.
            // No deep clone.
            let iw = std::mem::take(&mut state.iw);
            let (s, intents, effects) = iw_update(inner, iw);
            state.iw = s;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        AppMsg::Sql(m) => {
            let SqlMsg::Message(inner) = m;
            // The sql feature's update is a pure by-value transition: move the
            // state out, update it, move the result back. No deep clone.
            let sql = std::mem::take(&mut state.sql);
            let (s, intents, effects) = sql_workspace_update(inner, sql);
            state.sql = s;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        AppMsg::Footer(m) => {
            let FooterMsg::Message(inner) = m;
            // The footer feature's update is a pure by-value transition: move
            // the state out, update it, move the result back. No deep clone.
            let footer = std::mem::take(&mut state.footer);
            let (s, intents, effects) = footer_update(inner, footer);
            state.footer = s;
            // Keep the shell-level `global_status` mirror in sync with the
            // footer's authoritative status, so other code reading
            // `AppState::global_status` sees the latest value.
            state.global_status = state.footer.status.clone();
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        AppMsg::Perf(m) => {
            let PerfMsg::Message(inner) = m;
            let (s, intents, effects) = perf_update(inner, &mut state.perf);
            state.perf = s;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
    }
    result
}

/// Apply an action produced by an effect. `Dispatch` feeds a message back
/// into the router (bypassing the focus guard, since effect results are
/// delivered programmatically); `Shell` handles shell commands.
pub fn handle_action(action: Action, state: &mut AppState) -> UpdateResult {
    match action {
        Action::Dispatch(msg) => update_unchecked(msg, state),
        Action::Shell(shell) => match shell {
            crate::app_shell::action::ShellAction::Quit => {
                state.should_quit = true;
                UpdateResult::new()
            }
        },
        // Feature-specific actions are not yet handled: effects that emit
        // them are absent in the skeleton. They are intentionally dropped
        // (rather than panicking) so the loop stays resilient.
        // TODO: implement per-feature action handling once business effects
        // are introduced (e.g. HeaderAction::DataLoaded, SqlAction::QueryDone).
        Action::Header(_)
        | Action::Explorer(_)
        | Action::Discover(_)
        | Action::Iw(_)
        | Action::Sql(_)
        | Action::Footer(_)
        | Action::Perf(_) => UpdateResult::new(),
    }
}
