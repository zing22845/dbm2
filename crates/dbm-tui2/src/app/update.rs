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
use crate::app_shell::pane::{DiscoverPane, Pane};
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
        AppMsg::Explorer(_) => Some(Pane::Explorer),
        AppMsg::Discover(_) => Some(Pane::Discover(DiscoverPane::default())),
        AppMsg::Iw(_) => Some(Pane::InstanceWorkspace),
        AppMsg::Sql(_) => Some(Pane::Workspace),
        AppMsg::Perf(_) => Some(Pane::Workspace),
    }
}

/// Open the discover modal: reset its state and make it the active parent pane,
/// focused on the engine child pane.
fn open_discover(state: &mut AppState) {
    tracing::debug!("open_discover: setting focus to Discover parent pane");
    state.discover = crate::features::discover::state::DiscoverState::opened();
    state.focus = Pane::Discover(DiscoverPane::Engine);
    state.modal = None;
}

/// Close the discover modal and restore focus to the workspace parent pane.
fn close_discover(state: &mut AppState) {
    state.focus = Pane::Workspace;
    state.modal = None;
}

/// Apply a message to the global state, returning side-channel intents and
/// effects. Feature messages are only dispatched to their update when the
/// active focus zone permits keyboard input for them; shell and footer
/// messages always pass through.
pub fn update(msg: AppMsg, state: &mut AppState) -> UpdateResult {
    // When a modal (data popup) is open it owns all keyboard input, so its
    // messages bypass the focus guard. The discover parent pane likewise owns
    // all input while it is open.
    let discover_open = matches!(state.focus, Pane::Discover(_));
    if (state.modal.is_some() || discover_open) && matches!(msg, AppMsg::Discover(_)) {
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
                state.focus = pane;
                result.dirty = true;
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
            result.dirty = true;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        AppMsg::Explorer(m) => {
            // Explorer messages always touch rendered state (tree selection,
            // expansion, or a cross-feature open), so mark dirty unconditionally
            // at the end of this branch (see the `result.dirty = true` below).
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
                        result.pending.push_back(AppMsg::Sql(sql_msg));
                    }
                }
            }
            result.dirty = true;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        AppMsg::Discover(m) => {
            // The discover parent pane's child-pane focus lives on `state.focus`,
            // so a focus change (and moving focus to results on scan) is applied
            // here before/with the discover feature's content update.
            if let DiscoverMsg::Message(DiscoverMessage::Focus(sub)) = &m {
                state.focus = Pane::Discover(*sub);
            }
            if let DiscoverMsg::Message(DiscoverMessage::StartScan) = &m {
                state.focus = Pane::Discover(DiscoverPane::Results);
            }
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
            result.dirty = true;
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
            result.dirty = true;
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
            result.dirty = true;
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
            result.dirty = true;
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
