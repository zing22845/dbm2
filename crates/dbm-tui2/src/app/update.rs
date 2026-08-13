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
use crate::app::state::AppState;
use crate::app_shell::effect::ErasedEffect;
use crate::app_shell::intent::RoutableIntent;
use crate::app_shell::nav::DiscoverPane;
use crate::app_shell::pane::Pane;
use crate::features::discover::msg::{DiscoverMessage, DiscoverMsg};
use crate::features::discover::update::update as discover_update;
use crate::features::explorer::intent::ExplorerIntent;
use crate::features::explorer::msg::ExplorerMsg;
use crate::features::explorer::update::update as explorer_update;
use crate::features::global_footer::msg::FooterMsg;
use crate::features::global_footer::update::update as footer_update;
use crate::features::header::msg::{HeaderMessage, HeaderMsg};
use crate::features::header::update::update as header_update;
use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
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
    /// Whether this update round changed any state that affects rendering. The
    /// event loop repaints only when this is `true`; a `false` round (e.g. an
    /// input dropped by the focus guard, or a no-op message) skips the redraw.
    pub dirty: bool,
}

impl UpdateResult {
    pub(crate) fn new() -> Self {
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

/// Map a feature message to the `Pane` that must be active for its keyboard
/// input to be accepted. Shell and footer messages are always handled, so they
/// map to `None`.
///
/// Layout relationship: `Sql` and `Perf` live inside the main workspace region
/// and therefore share the `Workspace` parent pane. `Iw` (instance workspace)
/// occupies its own `InstanceWorkspace` pane. The `Discover` parent pane owns
/// all input while it is open.
fn focus_pane_of(msg: &AppMsg) -> Option<Pane> {
    match msg {
        // Shell, footer, and modal open/close messages are shell orchestration
        // and bypass the focus guard.
        AppMsg::Shell(_) | AppMsg::Footer(_) | AppMsg::OpenModal(_) | AppMsg::CloseModal => None,
        AppMsg::Header(_) => Some(Pane::Header),
        AppMsg::Explorer(_) => Some(Pane::Explorer(
            crate::app_shell::nav::ExplorerPane::default(),
        )),
        AppMsg::Discover(_) => Some(Pane::Discover(DiscoverPane::default())),
        AppMsg::Iw(_) => Some(Pane::InstanceWorkspace(crate::app_shell::nav::IwPane::default())),
        AppMsg::Sql(_) => Some(Pane::SQLWorkspace),
        AppMsg::Perf(_) => Some(Pane::SQLWorkspace),
    }
}

/// Open the discover modal: reset its state and make it the active parent pane,
/// focused on the engine child pane.
///
/// The user's edited targets are preserved across reopen (process lifetime),
/// matching the original dbm: only seed the loopback default when the list is
/// empty. Everything else (engine, results, scan state) resets for a fresh
/// scan session.
fn open_discover(state: &mut AppState) {
    tracing::debug!("open_discover: setting focus to Discover parent pane");
    let preserved_targets = std::mem::take(&mut state.discover.targets).targets;
    state.discover = crate::features::discover::state::DiscoverState::opened();
    if !preserved_targets.is_empty() {
        state.discover.targets.targets = preserved_targets;
    }
    state.focus = Pane::Discover(DiscoverPane::Engine);
    state.modal = None;
}

/// Close the discover modal and restore focus to the workspace parent pane.
fn close_discover(state: &mut AppState) {
    state.focus = Pane::SQLWorkspace;
    state.modal = None;
}

/// Build a `FocusChanged` shell message for the given pane.
fn focus_changed(pane: Pane) -> AppMsg {
    AppMsg::Shell(crate::app_shell::msg::ShellMsg::FocusChanged { pane })
}

/// An `AppMsg` that reloads the explorer instance tree from the store.
///
/// Used by the shell after closing the discover modal so instances registered
/// during the scan appear in the explorer immediately.
fn explorer_load_instances_msg() -> AppMsg {
    AppMsg::Explorer(crate::features::explorer::msg::ExplorerMsg::Message(
        crate::features::explorer::msg::ExplorerMessage::Instances(
            crate::features::explorer::instances::msg::InstancesMsg::Message(
                crate::features::explorer::instances::msg::InstancesMessage::Load,
            ),
        ),
    ))
}

/// Apply a message to the global state, returning side-channel intents and
/// effects.
///
/// The main message loop uses [`update_unchecked`]: focus routing is owned by
/// the input layer (`input::key_to_msg` only emits messages for the pane that
/// owns the keyboard), and *programmatic* messages — effect/action results,
/// cross-feature intents, and pending cascades — must reach a non-focused pane
/// (e.g. a background reload updating the explorer tree while focus sits on the
/// workspace). Gating those on focus would drop legitimate cross-pane updates.
///
/// This focus-gated entry exists as a defensive backstop for explicit keyboard
/// paths: it drops feature messages whose target pane does not own the focus,
/// so unfocused features never react to stray input. Shell and footer messages
/// always pass through, and an open modal / discover parent pane owns all input.
pub fn update(msg: AppMsg, state: &mut AppState) -> UpdateResult {
    // When a modal (data popup) is open it owns all keyboard input, so its
    // messages bypass the focus guard. The discover parent pane likewise owns
    // all input while it is open. A confirm action dispatched from a modal's
    // `y` key (e.g. delete connection / unregister instance) must also bypass
    // the guard: the modal may have been opened from a sub-pane (connections)
    // whose exact `Pane` doesn't equal the guard's coarse parent mapping.
    let modal_open = state.modal.is_some();
    let discover_open = matches!(state.focus, Pane::Discover(_));
    let modal_confirm = modal_open
        && matches!(
            msg,
            AppMsg::Iw(IwMsg::Message(
                IwMessage::UnregisterInstance { .. }
                    | IwMessage::Connections(
                        crate::features::instance_workspace::connections::msg::ConnectionsMsg::Message(
                            crate::features::instance_workspace::connections::msg::ConnectionsMessage::DeleteConnection { .. }
                        )
                    )
            ))
        );
    if (modal_open || discover_open) && matches!(msg, AppMsg::Discover(_)) {
        return update_unchecked(msg, state);
    }
    if modal_confirm {
        return update_unchecked(msg, state);
    }
    if let Some(pane) = focus_pane_of(&msg)
        && pane != state.focus
    {
        // The message targets a parent pane that does not currently own the
        // keyboard input, so it is dropped. This prevents unfocused features
        // from reacting to stray input.
        return UpdateResult::new();
    }
    update_unchecked(msg, state)
}

/// Apply a message regardless of the current focus pane. Used by the effect
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
        AppMsg::OpenModal(modal) => {
            state.modal = Some(modal);
            result.dirty = true;
        }
        AppMsg::CloseModal => {
            state.modal = None;
            result.dirty = true;
        }
        AppMsg::Shell(shell_msg) => match shell_msg {
            crate::app_shell::msg::ShellMsg::Quit => {
                state.should_quit = true;
            }
            crate::app_shell::msg::ShellMsg::Tick => {}
            crate::app_shell::msg::ShellMsg::FocusChanged { pane } => {
                // While the discover parent pane owns focus, no focus change is
                // allowed to move away from it (neither keyboard navigation nor
                // mouse clicks). The discover flow must complete or be closed
                // explicitly. This is the single choke point for that rule;
                // discover's own sub-pane switching uses DiscoverMessage::Focus,
                // not FocusChanged, so it is unaffected.
                if matches!(state.focus, Pane::Discover(_)) {
                    tracing::debug!(
                        to = ?pane,
                        "ignoring FocusChanged while discover owns focus"
                    );
                    return result;
                }
                // Keep the explorer feature's own sub-pane in sync with the
                // shell focus so rendering and key dispatch agree. Unlike an
                // eager reload on focus, the instance tree is only loaded at
                // startup and reloaded after registration (mirroring the
                // original dbm), so connections cached in the tree are not
                // discarded on every focus switch.
                state.focus = pane;
                result.dirty = true;
                if let Pane::Explorer(sub) = pane
                    && state.explorer.pane != sub
                {
                    state.explorer.pane = sub;
                }
                if let Pane::InstanceWorkspace(sub) = pane
                    && state.iw.pane != sub
                {
                    state.iw.pane = sub;
                }
            }
            crate::app_shell::msg::ShellMsg::ToggleTheme => {
                // Flip between the theme's dark and light palettes; the next
                // frame is drawn with the new palette automatically.
                state.theme.toggle();
                result.dirty = true;
            }
        },
        AppMsg::Header(m) => {
            // Opening a modal is shell orchestration, handled before the
            // header feature's own update so the modal state is ready for the
            // frame that follows.
            let opened_discover = if let HeaderMsg::Message(HeaderMessage::Activate) = &m
                && state.header.button == 0 {
                    open_discover(state);
                    true
                } else {
                    false
                };
            let HeaderMsg::Message(inner) = m;
            // The header feature's update is a pure by-value transition: move
            // the state out, update it, move the result back. No deep clone.
            let header = std::mem::take(&mut state.header);
            let (s, intents, effects, d) = header_update(inner, header);
            state.header = s;
            result.dirty |= d || opened_discover;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        AppMsg::Explorer(m) => {
            let ExplorerMsg::Message(inner) = m;
            // The explorer feature's update is a pure by-value transition: move
            // the state out, update it, move the result back. No deep clone.
            let explorer = std::mem::take(&mut state.explorer);
            let (s, intents, effects, mut explorer_dirty) = explorer_update(inner, explorer);
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
                        // Mark this instance as the active workspace (the
                        // `◆` marker + what the workspace region renders),
                        // matching the original dbm's `set_active_instance`.
                        state.explorer.instances.set_active_instance(*instance_idx);
                        let iw = std::mem::take(&mut state.iw);
                        let (iw2, i, e, d) = iw_update(
                            crate::features::instance_workspace::msg::IwMessage::OpenInstance {
                                instance_name,
                            },
                            iw,
                        );
                        state.iw = iw2;
                        explorer_dirty |= d;
                        result.intents.extend(i.into_iter().map(box_intent));
                        result.effects.extend(e.into_iter().map(box_effect));
                        // Switch focus to the instance workspace so the user
                        // sees it immediately instead of staying on explorer.
                        result.pending.push_back(focus_changed(Pane::InstanceWorkspace(
                            crate::app_shell::nav::IwPane::Overview,
                        )));
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
                            // Clone what the sql message needs before mutating
                            // the tree (to release the immutable `node` borrow).
                            let connection = conn.name.clone();
                            let connection_id = conn.id.clone();
                            // Mark this connection as the active workspace
                            // (the `●` marker + what the workspace region
                            // renders), matching the original dbm's
                            // `set_active_connection`. This overwrites any
                            // previously-open instance workspace so the display
                            // switches to the SQL workspace.
                            state.explorer.instances.set_active_connection(*instance_idx, *connection_idx);
                            // Enter on a connection focuses its existing tab (or
                            // opens one if none), mirroring the original dbm's
                            // `confirm_workspace_connection(force_new=false)`.
                            let sql_msg = SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                                SqlTabMessage::FocusConnectionTab {
                                    instance: instance_name,
                                    connection,
                                    connection_id,
                                    database: None,
                                    schema: None,
                                },
                            )));
                            explorer_dirty = true;
                            result.pending.push_back(AppMsg::Sql(sql_msg));
                            // Switch focus to the workspace so the user sees the
                            // active tab immediately, mirroring the original
                            // dbm's `confirm_workspace_connection` → `focus_workspace`.
                            result.pending.push_back(focus_changed(Pane::SQLWorkspace));
                        }
                    }
                }
                if let ExplorerIntent::Instances(
                    crate::features::explorer::instances::intent::InstancesIntent::NewConnectionWorkspace {
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
                            let connection = conn.name.clone();
                            let connection_id = conn.id.clone();
                            // Mark this connection as the active workspace
                            // (the `●` marker + what the workspace region
                            // renders), matching the original dbm's
                            // `set_active_connection`.
                            state.explorer.instances.set_active_connection(*instance_idx, *connection_idx);
                            // `n` on a connection always opens a fresh editor,
                            // mirroring the original dbm's
                            // `confirm_workspace_connection(force_new=true)`.
                            let sql_msg = SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                                SqlTabMessage::OpenConnectionTab {
                                    instance: instance_name,
                                    connection,
                                    connection_id,
                                    database: None,
                                    schema: None,
                                },
                            )));
                            explorer_dirty = true;
                            result.pending.push_back(AppMsg::Sql(sql_msg));
                            result.pending.push_back(focus_changed(Pane::SQLWorkspace));
                        }
                    }
                }
                // Cross-feature: opening an object (e.g. a table) in the object
                // tree opens a SQL tab scoped to that object's database and
                // schema, using the connection the tree is currently bound to.
                if let ExplorerIntent::Objects(
                    crate::features::explorer::objects::intent::ObjectsIntent::OpenObject { target },
                ) = intent
                {
                    let instance = state.explorer.objects.bound_instance.clone();
                    let connection = state.explorer.objects.bound_connection.clone();
                    if !instance.is_empty() && !connection.is_empty() {
                        let connection_id = state
                            .explorer
                            .instances
                            .connection_id_by_name(&instance, &connection)
                            .unwrap_or_default();
                        let sql_msg = SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                            SqlTabMessage::OpenConnectionTab {
                                instance,
                                connection,
                                connection_id,
                                database: Some(target.database.clone()),
                                schema: target.schema.clone(),
                            },
                        )));
                        explorer_dirty = true;
                        result.pending.push_back(AppMsg::Sql(sql_msg));
                        // Switch focus to the workspace so the user sees the
                        // newly opened tab immediately, mirroring the original
                        // dbm's `confirm_workspace_connection` → `focus_workspace`.
                        result.pending.push_back(focus_changed(Pane::SQLWorkspace));
                    }
                }
            }
            result.dirty |= explorer_dirty;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        AppMsg::Discover(m) => {
            // The discover parent pane's child-pane focus lives on `state.focus`,
            // so a focus change (and moving focus to results on scan) is applied
            // here before/with the discover feature's content update.
            let mut discover_dirty = false;
            if let DiscoverMsg::Message(DiscoverMessage::Focus(sub)) = &m {
                state.focus = Pane::Discover(*sub);
                discover_dirty = true;
            }
            if let DiscoverMsg::Message(DiscoverMessage::StartScan) = &m {
                state.focus = Pane::Discover(DiscoverPane::Results);
                discover_dirty = true;
            }
            // Closing the modal is shell orchestration, handled after the
            // discover feature's own update so the frame is ready for teardown.
            let should_close = matches!(&m, DiscoverMsg::Message(DiscoverMessage::Close));
            let DiscoverMsg::Message(inner) = m;
            // The discover feature's update is a pure by-value transition: move
            // the state out, update it, move the result back. No deep clone.
            let discover = std::mem::take(&mut state.discover);
            let (s, intents, effects, d) = discover_update(inner, discover);
            state.discover = s;
            if should_close {
                close_discover(state);
                discover_dirty = true;
                // Registering discovered instances updates the store while the
                // discover modal is open, so re-fetch the explorer instance tree
                // on close so newly registered instances show up immediately
                // instead of only after a restart.
                result.pending.push_back(explorer_load_instances_msg());
            }
            result.dirty |= d || discover_dirty;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        AppMsg::Iw(m) => {
            // A confirm-unregister / confirm-delete arrived, so the confirm
            // modal should close. The shell owns the modal, so this is shell
            // orchestration here.
            if matches!(
                &m,
                IwMsg::Message(IwMessage::UnregisterInstance { .. })
                    | IwMsg::Message(IwMessage::Connections(
                        crate::features::instance_workspace::connections::msg::ConnectionsMsg::Message(
                            crate::features::instance_workspace::connections::msg::ConnectionsMessage::DeleteConnection { .. }
                        )
                    ))
            ) {
                state.modal = None;
                result.dirty = true;
            }
            // When an unregister completes, drop out of the (now removed)
            // instance workspace, refresh the explorer tree, and return focus
            // to the explorer (matching the original dbm's post-unregister
            // `reload_tree` + focus return).
            if let IwMsg::Message(IwMessage::Unregistered { instance }) = &m {
                tracing::debug!(instance, "shell: instance unregistered; returning to explorer");
                result.pending.push_back(explorer_load_instances_msg());
                result.pending.push_back(AppMsg::Shell(
                    crate::app_shell::msg::ShellMsg::FocusChanged {
                        pane: Pane::Explorer(
                            crate::app_shell::nav::ExplorerPane::default(),
                        ),
                    },
                ));
            }
            let IwMsg::Message(inner) = m;
            // The instance workspace feature's update is a pure by-value
            // transition: move the state out, update it, move the result back.
            // No deep clone.
            let iw = std::mem::take(&mut state.iw);
            let (s, intents, effects, d) = iw_update(inner, iw);
            state.iw = s;
            result.dirty |= d;
            // A connection was added/edited/deleted inside the instance
            // workspace: refresh the explorer tree for that instance so the
            // change shows up on the left immediately (matching the original
            // dbm's `load_instance_connections` on save). This is shell-level
            // orchestration between the iw and explorer features.
            for intent in &intents {
                if let crate::features::instance_workspace::intent::IwIntent::Connections(
                    crate::features::instance_workspace::connections::intent::ConnectionsIntent::ConnectionsChanged {
                        instance_name,
                    },
                ) = intent
                {
                    let instance_idx = state
                        .explorer
                        .instances
                        .nodes
                        .iter()
                        .position(|n| {
                            n.instance
                                .as_ref()
                                .is_some_and(|i| i.name == *instance_name)
                        });
                    if let Some(instance_idx) = instance_idx {
                        result.pending.push_back(AppMsg::Explorer(
                            crate::features::explorer::msg::ExplorerMsg::Message(
                                crate::features::explorer::msg::ExplorerMessage::Instances(
                                    crate::features::explorer::instances::msg::InstancesMsg::Message(
                                        crate::features::explorer::instances::msg::InstancesMessage::RefreshConnections {
                                            instance_idx,
                                        },
                                    ),
                                ),
                            ),
                        ));
                    }
                }
            }
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        AppMsg::Sql(m) => {
            let SqlMsg::Message(inner) = m;
            // The sql feature's update is a pure by-value transition: move the
            // state out, update it, move the result back. No deep clone.
            let sql = std::mem::take(&mut state.sql);
            let (s, intents, effects, d) = sql_workspace_update(inner, sql);
            state.sql = s;
            result.dirty |= d;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        AppMsg::Footer(m) => {
            let FooterMsg::Message(inner) = m;
            // The footer feature's update is a pure by-value transition: move
            // the state out, update it, move the result back. No deep clone.
            let footer = std::mem::take(&mut state.footer);
            let (s, intents, effects, d) = footer_update(inner, footer);
            state.footer = s;
            // Keep the shell-level `global_status` mirror in sync with the
            // footer's authoritative status, so other code reading
            // `AppState::global_status` sees the latest value.
            state.global_status = state.footer.status.clone();
            result.dirty |= d;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        AppMsg::Perf(m) => {
            let PerfMsg::Message(inner) = m;
            let (s, intents, effects, d) = perf_update(inner, &mut state.perf);
            state.perf = s;
            result.dirty |= d;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
    }
    result
}

/// Apply an action produced by an effect.
///
/// Every action is converted into the message(s) it should dispatch back into
/// the router via [`action_to_app_msgs`], then applied through `update_unchecked`
/// (which bypasses the focus guard, since effect results are delivered
/// programmatically). This mirrors the message-round drain exactly, so an
/// async action received here as a `recv()` seed is never dropped or handled
/// differently from one drained in bulk.
pub fn handle_action(action: Action, state: &mut AppState) -> UpdateResult {
    let mut result = UpdateResult::new();
    for msg in crate::app::loop_mod::action_to_app_msgs(action) {
        let sub = update_unchecked(msg, state);
        result.dirty |= sub.dirty;
        result.intents.extend(sub.intents);
        result.effects.extend(sub.effects);
        result.pending.extend(sub.pending);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn explorer_load_msg() -> AppMsg {
        AppMsg::Explorer(
            crate::features::explorer::msg::ExplorerMsg::Message(
                crate::features::explorer::msg::ExplorerMessage::Instances(
                    crate::features::explorer::instances::msg::InstancesMsg::Message(
                        crate::features::explorer::instances::msg::InstancesMessage::Load,
                    ),
                ),
            ),
        )
    }

    fn focus_changed_msg(pane: Pane) -> AppMsg {
        AppMsg::Shell(crate::app_shell::msg::ShellMsg::FocusChanged { pane })
    }

    #[test]
    fn focus_changed_rejected_while_discover_owns_focus() {
        let mut state = AppState::default();
        state.focus = Pane::Discover(DiscoverPane::Engine);
        let before = state.focus;
        let result = update(
            focus_changed_msg(Pane::Explorer(
                crate::app_shell::nav::ExplorerPane::default(),
            )),
            &mut state,
        );
        // Focus stays on discover: the single choke point blocks leaving it.
        assert_eq!(state.focus, before);
        assert!(!result.dirty, "rejected focus change must not mark dirty");
    }

    #[test]
    fn focus_changed_allowed_when_not_discover() {
        let mut state = AppState::default();
        state.focus = Pane::Header;
        update(
            focus_changed_msg(Pane::SQLWorkspace),
            &mut state,
        );
        assert_eq!(state.focus, Pane::SQLWorkspace);
    }

    #[test]
    fn focus_guard_drops_explorer_load_when_focus_not_on_explorer() {
        // Default focus is the header; a guarded `update` must drop an
        // Explorer(Load) so the tree is not loaded by stray input.
        let mut state = AppState::default();
        assert_eq!(state.focus, Pane::Header);
        let result = update(explorer_load_msg(), &mut state);
        assert!(
            result.effects.is_empty(),
            "guarded update must drop explorer load while focus is the header"
        );
    }

    #[test]
    fn delete_connection_closes_confirm_modal() {
        use crate::features::instance_workspace::connections::msg::{
            ConnectionsMessage, ConnectionsMsg,
        };
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};

        let mut state = AppState::default();
        // Focus is on the instance workspace (where the delete-confirm modal
        // was opened), so the DeleteConnection message passes the focus guard.
        state.focus = Pane::InstanceWorkspace(crate::app_shell::nav::IwPane::Connections);
        state.modal = Some(crate::app::state::ModalKind::DeleteConnectionConfirm {
            instance: "inst".into(),
            connection: "conn".into(),
        });
        let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Connections(
            ConnectionsMsg::Message(ConnectionsMessage::DeleteConnection {
                instance_name: "inst".into(),
                connection_name: "conn".into(),
            }),
        )));
        update(msg, &mut state);
        assert!(state.modal.is_none(), "confirm modal must close on delete");
    }

    fn explorer_instances_msg(m: crate::features::explorer::instances::msg::InstancesMessage) -> AppMsg {
        AppMsg::Explorer(
            crate::features::explorer::msg::ExplorerMsg::Message(
                crate::features::explorer::msg::ExplorerMessage::Instances(
                    crate::features::explorer::instances::msg::InstancesMsg::Message(m),
                ),
            ),
        )
    }

    fn sample_managed_instance(name: &str) -> dbm_store::ManagedInstance {
        dbm_store::ManagedInstance {
            id: format!("id-{name}"),
            fingerprint: format!("fp-{name}"),
            name: name.to_string(),
            engine: dbm_core::Engine::Postgres,
            host: "127.0.0.1".to_string(),
            port: 5432,
            socket_path: None,
            data_dir: None,
            env_label: None,
            registered_at: "now".to_string(),
            version_full: None,
            version_short: None,
            version_checked_at: None,
            lifecycle_status: None,
            lifecycle_checked_at: None,
            lifecycle_detail: None,
        }
    }

    fn sample_connection(name: &str) -> dbm_store::InstanceConnection {
        dbm_store::InstanceConnection {
            id: format!("c-{name}"),
            instance_id: "id".to_string(),
            name: name.to_string(),
            username: "postgres".to_string(),
            database: "postgres".to_string(),
            has_password: false,
            ssl_mode: String::new(),
            env_label: None,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
            test_succeeded_at: None,
            test_failed_at: None,
        }
    }

    #[test]
    fn new_connection_tab_from_explorer_sequences_continuously() {
        use crate::features::explorer::instances::msg::InstancesMessage;

        let mut state = AppState::default();
        state.explorer.instances.set_instances(vec![sample_managed_instance("inst")]);
        // Expand the instance and load one connection so `cursor_selection`
        // resolves to a connection row.
        state.explorer.instances.nodes[0].expanded = true;
        state.explorer.instances.nodes[0].loaded = true;
        state.explorer.instances.nodes[0].connections = vec![sample_connection("c1")];
        state.explorer.instances.cursor = 1; // on the connection row
        state.focus = Pane::Explorer(crate::app_shell::nav::ExplorerPane::default());

        // Apply a message and drain the resulting `pending` queue exactly like
        // the event loop does, so intent-dispatched messages (e.g. opening a
        // tab) take effect within the same logical round.
        fn drain(state: &mut AppState, msg: AppMsg) {
            let mut queue = std::collections::VecDeque::from([msg]);
            while let Some(m) = queue.pop_front() {
                let r = update_unchecked(m, state);
                queue.extend(r.pending);
            }
        }

        // Enter on the connection opens the first tab (<sql 1>).
        drain(&mut state, explorer_instances_msg(InstancesMessage::Select));
        assert_eq!(state.sql.sql_tab.tabs.len(), 1);
        assert_eq!(state.sql.sql_tab.tabs[0].session.sequence, 1);

        // `n` always opens a fresh tab -> <sql 2>, then <sql 3>.
        drain(&mut state, explorer_instances_msg(InstancesMessage::NewConnectionTab));
        assert_eq!(state.sql.sql_tab.tabs.len(), 2);
        assert_eq!(state.sql.sql_tab.tabs[1].session.sequence, 2);

        drain(&mut state, explorer_instances_msg(InstancesMessage::NewConnectionTab));
        assert_eq!(state.sql.sql_tab.tabs.len(), 3);
        assert_eq!(state.sql.sql_tab.tabs[2].session.sequence, 3);
    }

    #[test]
    fn closing_all_tabs_leaves_sql_tab_empty() {
        use crate::features::sql_workspace::sql_tab::msg::SqlTabMessage;
        use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};

        let mut state = AppState::default();
        // Open one tab for the active connection.
        state.sql.sql_tab.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "c1-id".into(),
            None,
            None,
        );
        assert_eq!(state.sql.sql_tab.tabs.len(), 1);
        assert_eq!(state.sql.sql_tab.visible_tab_count(), 1);

        let close_tab_msg = |visible: usize| {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                crate::features::sql_workspace::sql_tab::msg::SqlTabMsg::Message(
                    SqlTabMessage::CloseTab(visible),
                ),
            )))
        };

        // Close the only visible tab (offset 0).
        let r = update_unchecked(close_tab_msg(0), &mut state);
        assert!(r.dirty, "closing the last tab must mark the view dirty");
        assert!(state.sql.sql_tab.tabs.is_empty(), "all tabs must be closed");
        assert_eq!(state.sql.sql_tab.visible_tab_count(), 0);
        // `sql_tab/view.rs` renders the empty-state hint when tabs are empty.
        assert!(state.sql.sql_tab.tabs.is_empty());
    }

    #[test]
    fn update_unchecked_allows_explorer_load_regardless_of_focus() {
        // Startup uses `update_unchecked` so the tree loads even though focus
        // is still on the header at boot.
        let mut state = AppState::default();
        assert_eq!(state.focus, Pane::Header);
        let result = update_unchecked(explorer_load_msg(), &mut state);
        assert!(
            !result.effects.is_empty(),
            "update_unchecked must emit the load effect regardless of focus"
        );
    }
}
