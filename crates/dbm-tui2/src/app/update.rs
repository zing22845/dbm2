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
use crate::features::explorer::effect::ExplorerEffect;
use crate::features::explorer::instances::effect::InstancesEffect;
use crate::features::explorer::intent::ExplorerIntent;
use crate::features::explorer::msg::ExplorerMsg;
use crate::features::explorer::objects::effect::ObjectsEffect;
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

/// Keep the objects tree bound to the *active* connection, mirroring the
/// original dbm: the objects pane always shows the active workspace's
/// connection. When the active workspace is an instance (or nothing) the tree
/// is unbound so it shows the "open a connection to browse objects" prompt.
/// Returns a `Bind` message when the binding changed and needs a catalog load.
fn sync_objects_binding(
    objects: &mut crate::features::explorer::objects::state::ObjectsState,
    instances: &crate::features::explorer::instances::state::InstancesState,
) -> Option<AppMsg> {
    use crate::features::explorer::instances::state::ActiveWorkspaceKind;
    match instances.active_workspace {
        Some(ActiveWorkspaceKind::Connection { instance_idx, conn_idx }) => {
            let instance = instances.instance_name(instance_idx);
            let connection = instances
                .nodes
                .get(instance_idx)
                .and_then(|n| n.connections.get(conn_idx))
                .map(|c| c.name.clone());
            if let Some(connection) = connection {
                if objects.bound_instance != instance || objects.bound_connection != connection {
                    return Some(AppMsg::Explorer(
                        crate::features::explorer::msg::ExplorerMsg::Message(
                            crate::features::explorer::msg::ExplorerMessage::Objects(
                                crate::features::explorer::objects::msg::ObjectsMsg::Message(
                                    crate::features::explorer::objects::msg::ObjectsMessage::Bind {
                                        instance,
                                        connection,
                                    },
                                ),
                            ),
                        ),
                    ));
                }
            }
            None
        }
        _ => {
            objects.clear_binding();
            None
        }
    }
}

/// Sync the objects tree's active schema to the active SQL tab's database and
/// schema, but only when the tab is bound to the same instance/connection the
/// objects tree shows. The active schema (and its parent database) is forced
/// expanded and cannot be collapsed (original dbm). With no matching tab the
/// active state is cleared.
/// Sync the objects tree's active database/schema with the active SQL tab. The
/// active database is set immediately (force-expanded), but the active schema
/// is deferred until the database's schemas load — if the schema no longer
/// exists, it is degraded to no active schema. Returns an optional
/// [`ObjectsEffect::LoadSchemas`] when the newly active database's schemas
/// still need to be fetched. Callers must push the returned effect (so a
/// force-expanded active database behaves like a manually-expanded one).
fn sync_objects_active(
    objects: &mut crate::features::explorer::objects::state::ObjectsState,
    sql: &crate::features::sql_workspace::state::SqlState,
) -> Option<ObjectsEffect> {
    let mut db = None;
    let mut schema = None;
    if let Some(tab) = sql.sql_tab.active_tab() {
        let tab_instance = tab.session.instance.as_deref().unwrap_or_default();
        let tab_connection = tab
            .session
            .connection
            .as_deref()
            .or(tab.session.connection_id.as_deref())
            .unwrap_or_default();
        // Only when the tab is bound to the same connection the objects tree
        // shows (matching the original dbm's `active_tab_context`).
        if !objects.bound_instance.is_empty()
            && tab_instance == objects.bound_instance
            && tab_connection == objects.bound_connection
        {
            db = tab.session.database.clone();
            schema = tab.session.schema.clone();
        }
    }
    let needs_load = objects.defer_active(db, schema);
    // If the active database's schemas still need to be fetched, request them
    // so the deferred schema can be validated once they load.
    if needs_load
        && !objects.bound_instance.is_empty()
        && !objects.bound_connection.is_empty()
        && let Some(db) = objects.active_db.clone()
    {
        return Some(ObjectsEffect::LoadSchemas {
            instance: objects.bound_instance.clone(),
            connection: objects.bound_connection.clone(),
            database: db,
        });
    }
    None
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
        AppMsg::Shell(_)
        | AppMsg::Footer(_)
        | AppMsg::OpenModal(_)
        | AppMsg::CloseModal
        | AppMsg::SetExplorerWidth(_) => None,
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
    // Focus moves to Discover. The explorer sub-pane + cursor (and the sql /
    // iw states) are left untouched; closing discover returns to the Explorer
    // via `set_focus`, so the sub-pane and cursor the user had before opening
    // discover come back unchanged.
    state.focus = Pane::Discover(DiscoverPane::Engine);
    state.modal = None;
}

/// Close the discover modal and restore focus to wherever the user was before
/// it opened (falling back to the SQL workspace if nothing was captured).
fn close_discover(state: &mut AppState) {
    // Match the original dbm: closing discover hands focus to the Explorer.
    // The explorer sub-pane (instances/objects) and its cursor live in the
    // explorer feature state, which the discover modal never touches, so they
    // are preserved automatically — the user lands back on the same
    // sub-pane + cursor they had before opening discover. This is
    // deterministic and avoids the desync/overview issues of trying to
    // reconstruct the pre-discover focus.
    state.set_focus(Pane::Explorer(state.explorer.pane));
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
        AppMsg::SetExplorerWidth(width) => {
            state.splitter.set_explorer_pane_width(width);
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
                // Route the focus change through the single choke point so the
                // feature sub-panes stay in lockstep with the shell focus.
                // Unlike an eager reload on focus, the instance tree is only
                // loaded at startup and reloaded after registration (mirroring
                // the original dbm), so connections cached in the tree are not
                // discarded on every focus switch.
                state.set_focus(pane);
                result.dirty = true;
                // Keep the objects tree's binding + active schema synced to the
                // active SQL tab on focus changes too (not just SQL edits), so
                // entering or leaving the workspace reflects the current state.
                if let Some(bind) = sync_objects_binding(
                    &mut state.explorer.objects,
                    &state.explorer.instances,
                ) {
                    result.pending.push_back(bind);
                }
                if let Some(effect) = sync_objects_active(
                    &mut state.explorer.objects,
                    &state.sql,
                ) {
                    // Lift the objects effect into an explorer effect so it can
                    // be type-erased against the global action type.
                    result.effects.push(box_effect(ExplorerEffect::Objects(effect)));
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
                        // active-row highlight + what the workspace region
                        // renders), matching the original dbm's
                        // `set_active_instance`. This force-expands the node, so
                        // keep the expanded state consistent with the
                        // connection-load state: if the instance was collapsed
                        // and its connections are not loaded yet, fetch them now.
                        state.explorer.instances.set_active_instance(*instance_idx);
                        if state.explorer.instances.nodes.get(*instance_idx).is_some_and(|n| {
                            n.expanded && !n.loaded
                        }) {
                            result.effects.push(box_effect(ExplorerEffect::Instances(
                                InstancesEffect::LoadConnections {
                                    instance_idx: *instance_idx,
                                    instance_name: instance_name.clone(),
                                },
                            )));
                        }
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
                            // The connection's configured default database is the
                            // fallback context when the connection has no tab yet.
                            let default_database = if conn.database.is_empty() {
                                Some("postgres".to_string())
                            } else {
                                Some(conn.database.clone())
                            };
                            // Mark this connection as the active workspace
                            // (the active-row highlight + what the workspace
                            // region renders), matching the original dbm's
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
                                    default_database,
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
                            // The connection's configured default database is the
                            // fallback context when the connection has no tab yet.
                            let default_database = if conn.database.is_empty() {
                                Some("postgres".to_string())
                            } else {
                                Some(conn.database.clone())
                            };
                            // Mark this connection as the active workspace
                            // (the active-row highlight + what the workspace
                            // region renders), matching the original dbm's
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
                                    default_database,
                                },
                            )));
                            explorer_dirty = true;
                            result.pending.push_back(AppMsg::Sql(sql_msg));
                            result.pending.push_back(focus_changed(Pane::SQLWorkspace));
                        }
                    }
                }
                // Cross-feature: opening an object (e.g. a table) from the object
                // tree. Mirroring the original dbm's double-click behavior:
                //   - a table/view/matview runs a `SELECT * FROM "schema"."table"`
                //     data query in the connection's active SQL tab (opening it
                //     if needed) and focuses the Results pane;
                //   - other objects (procedure/function/sequence) open a new SQL
                //     tab scoped to the object's database/schema.
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
                        let sql_msg = match target.kind {
                            crate::features::explorer::objects::state::ObjectKind::Tables
                            | crate::features::explorer::objects::state::ObjectKind::Views
                            | crate::features::explorer::objects::state::ObjectKind::Matviews => {
                                SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                                    SqlTabMessage::RunTableQuery {
                                        instance,
                                        connection,
                                        connection_id,
                                        database: Some(target.database.clone()),
                                        schema: target.schema.clone(),
                                        table: target.name.clone(),
                                        table_schema: target.schema.clone(),
                                    },
                                )))
                            }
                            _ => SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                                SqlTabMessage::OpenConnectionTab {
                                    instance,
                                    connection,
                                    connection_id,
                                    database: Some(target.database.clone()),
                                    schema: target.schema.clone(),
                                    // An explicit database is passed, so the
                                    // default fallback is never used.
                                    default_database: None,
                                },
                            ))),
                        };
                        explorer_dirty = true;
                        result.pending.push_back(AppMsg::Sql(sql_msg));
                        // Switch focus to the workspace so the user sees the
                        // active tab / results immediately, mirroring the
                        // original dbm's `confirm_workspace_connection` →
                        // `focus_workspace`.
                        result.pending.push_back(focus_changed(Pane::SQLWorkspace));
                    }
                }
                // Cross-feature: Enter on a schema row applies it as the active
                // database/schema of the bound SQL tab (mirroring the original
                // dbm's `apply_objects_schema`). The active schema is already
                // highlighted/forced-expanded by the objects update itself.
                if let ExplorerIntent::Objects(
                    crate::features::explorer::objects::intent::ObjectsIntent::ApplySchema {
                        database,
                        name,
                    },
                ) = intent
                {
                    let tab_id = state
                        .sql
                        .sql_tab
                        .active_tab()
                        .map(|t| t.session.id);
                    if let Some(tab_id) = tab_id {
                        let sql_msg = SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                            SqlTabMessage::ApplyContext {
                                tab_id,
                                database: database.clone(),
                                schema: name.clone(),
                            },
                        )));
                        explorer_dirty = true;
                        result.pending.push_back(AppMsg::Sql(sql_msg));
                    }
                }
            }
            result.dirty |= explorer_dirty;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
            // Keep the objects tree's binding + active schema in sync with the
            // active SQL tab / connection. Explorer-driven activation — e.g.
            // `ConnectionsLoaded` refining a restored active connection, or a
            // connection selected in the tree — changes `active_workspace`, so
            // the objects tree must rebind here too, not only on SQL/focus
            // changes. `Bind` is idempotent, so a redundant bind is a no-op.
            if let Some(bind) = sync_objects_binding(
                &mut state.explorer.objects,
                &state.explorer.instances,
            ) {
                result.pending.push_back(bind);
            }
            if let Some(effect) = sync_objects_active(
                &mut state.explorer.objects,
                &state.sql,
            ) {
                result.effects.push(box_effect(ExplorerEffect::Objects(effect)));
            }
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
            // A successful register writes to the store while the discover modal
            // stays open; reload the explorer instance tree right away so the
            // newly registered instance appears on the left immediately, without
            // waiting for the modal to close.
            let should_reload_instances = matches!(
                &m,
                DiscoverMsg::Message(DiscoverMessage::RegisterComplete { .. })
            );
            let DiscoverMsg::Message(inner) = m;
            // The discover feature's update is a pure by-value transition: move
            // the state out, update it, move the result back. No deep clone.
            let discover = std::mem::take(&mut state.discover);
            let (s, intents, effects, d) = discover_update(inner, discover);
            state.discover = s;
            if should_close {
                close_discover(state);
                discover_dirty = true;
                // A close also re-fetches the explorer instance tree so any
                // instances registered before closing still show up (a safety
                // net for the immediate reload below).
                result.pending.push_back(explorer_load_instances_msg());
            } else if should_reload_instances {
                // Registering succeeded while the modal is open: reload the
                // explorer instance tree now so the new instance appears on the
                // left immediately (the user does not have to close discover
                // to see it).
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
            // A `SqlTabMessage::Focus` (from the uppercase S/H/R pane-jump
            // shortcuts) also moves the shell focus into the SQL workspace, so
            // jumping from the explorer/header lands on the editor/results/
            // history sub-pane rather than leaving the shell focus behind.
            if matches!(
                inner,
                SqlMessage::SqlTab(SqlTabMsg::Message(SqlTabMessage::Focus(_)))
            ) {
                state.focus = Pane::SQLWorkspace;
                result.dirty = true;
            }
            // The sql feature's update is a pure by-value transition: move the
            // state out, update it, move the result back. No deep clone.
            let sql = std::mem::take(&mut state.sql);
            let (s, intents, effects, d) = sql_workspace_update(inner, sql);
            state.sql = s;
            result.dirty |= d;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
            // Keep the objects tree's binding + active schema in sync with the
            // active SQL tab / connection. The active path is forced expanded
            // and cannot be collapsed (original dbm).
            if let Some(bind) = sync_objects_binding(
                &mut state.explorer.objects,
                &state.explorer.instances,
            ) {
                result.pending.push_back(bind);
            }
            if let Some(effect) = sync_objects_active(
                &mut state.explorer.objects,
                &state.sql,
            ) {
                result.effects.push(box_effect(ExplorerEffect::Objects(effect)));
            }
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
    fn sync_objects_binding_follows_active_connection() {
        use crate::features::explorer::instances::state::InstancesState;
        let mut objects = crate::features::explorer::objects::state::ObjectsState::default();
        let mut instances = InstancesState::default();
        instances.set_instances(vec![sample_managed_instance("inst")]);
        instances.nodes[0].loaded = true;
        instances.nodes[0].connections = vec![sample_connection("c1")];

        // Active workspace is a connection -> bind message returned (binding
        // differs from empty).
        instances.set_active_connection(0, 0);
        let bind = sync_objects_binding(&mut objects, &instances)
            .expect("active connection should request a bind");
        assert!(matches!(
            bind,
            AppMsg::Explorer(crate::features::explorer::msg::ExplorerMsg::Message(
                crate::features::explorer::msg::ExplorerMessage::Objects(
                    crate::features::explorer::objects::msg::ObjectsMsg::Message(
                        crate::features::explorer::objects::msg::ObjectsMessage::Bind { instance, connection }
                    )
                )
            )) if instance == "inst" && connection == "c1"
        ));

        // After bind is applied, the same active connection yields no rebind.
        objects.sync_binding("inst".into(), "c1".into());
        assert!(sync_objects_binding(&mut objects, &instances).is_none());

        // Active workspace is an instance -> objects are unbound (prompt shown).
        instances.set_active_instance(0);
        assert!(sync_objects_binding(&mut objects, &instances).is_none());
        assert!(objects.bound_connection.is_empty(), "instance-active must unbind objects");
    }

    #[test]
    fn connections_loaded_activation_binds_objects_tree() {
        use crate::features::explorer::instances::msg::InstancesMessage;
        use crate::features::explorer::instances::state::ActiveWorkspaceKind;

        let mut state = AppState::default();
        state.explorer.instances.set_instances(vec![sample_managed_instance("inst")]);
        state.explorer.instances.nodes[0].expanded = true;
        // A saved connection-active pending restore (set by `apply_snapshot`);
        // loading this instance's connections refines the active workspace onto
        // the connection. This is the exact session-restore path.
        state.explorer.instances.restore_active_connection =
            Some(("inst".to_string(), "c1".to_string()));

        // An explorer-driven activation: loading the instance's connections
        // refines the active workspace onto the connection (this is the path
        // the session restore / ConnectionsLoaded action takes). It must not
        // only highlight the row but also bind the objects tree.
        let r = update_unchecked(
            explorer_instances_msg(InstancesMessage::ConnectionsLoaded {
                instance_idx: 0,
                connections: vec![sample_connection("c1")],
            }),
            &mut state,
        );
        assert_eq!(
            state.explorer.instances.active_workspace,
            Some(ActiveWorkspaceKind::Connection { instance_idx: 0, conn_idx: 0 })
        );
        // The pending queue must carry a Bind for the active connection.
        let has_bind = r.pending.iter().any(|m| matches!(
            m,
            AppMsg::Explorer(crate::features::explorer::msg::ExplorerMsg::Message(
                crate::features::explorer::msg::ExplorerMessage::Objects(
                    crate::features::explorer::objects::msg::ObjectsMsg::Message(
                        crate::features::explorer::objects::msg::ObjectsMessage::Bind {
                            instance, connection
                        }
                    )
                )
            )) if instance == "inst" && connection == "c1"
        ));
        assert!(has_bind, "explorer-driven activation must request an objects bind");
    }

    #[test]
    fn duplicate_bind_is_idempotent_and_loads_once() {
        let s = crate::features::explorer::objects::state::ObjectsState::default();
        // First bind rebinds and resets the catalog.
        let (s, _i, effects, _d) = crate::features::explorer::objects::update::update(
            crate::features::explorer::objects::msg::ObjectsMessage::Bind {
                instance: "inst".into(),
                connection: "c1".into(),
            },
            s,
        );
        assert!(s.bound_connection == "c1");
        assert_eq!(effects.len(), 1, "first bind loads databases once");

        // A duplicate bind for the same connection is a no-op: no reload.
        let (s, _i, effects, dirty) = crate::features::explorer::objects::update::update(
            crate::features::explorer::objects::msg::ObjectsMessage::Bind {
                instance: "inst".into(),
                connection: "c1".into(),
            },
            s,
        );
        assert!(effects.is_empty(), "duplicate bind must not reload databases");
        assert!(!dirty, "duplicate bind must not mark the view dirty");
        assert!(s.bound_connection == "c1");
    }

    #[test]
    fn sql_subpane_focus_survives_round_trip_to_explorer() {
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;
        let mut state = AppState::default();
        state.focus = Pane::SQLWorkspace;
        // Open a tab and focus History inside the workspace.
        state.sql.sql_tab.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            None,
            None,
            None,
        );
        state.sql.sql_tab.tabs[0].focus = SqlFocus::History;

        // Leave to the explorer, then come back to the SQL workspace.
        update_unchecked(
            focus_changed_msg(Pane::Explorer(
                crate::app_shell::nav::ExplorerPane::default(),
            )),
            &mut state,
        );
        update_unchecked(focus_changed_msg(Pane::SQLWorkspace), &mut state);

        // The sub-pane focus is remembered per tab (shell FocusChanged only
        // moves `state.focus`, never `tab.focus`), so History is still active.
        assert_eq!(state.focus, Pane::SQLWorkspace);
        assert_eq!(
            state.sql.sql_tab.tabs[0].focus,
            SqlFocus::History,
            "sub-pane focus must survive leaving and re-entering the workspace"
        );
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
    fn discover_close_returns_to_explorer_preserving_subpane_and_cursor() {
        use crate::app_shell::nav::ExplorerPane;

        let mut state = AppState::default();
        // User works on the explorer's objects sub-pane (cursor moved down).
        state.set_focus(Pane::Explorer(ExplorerPane::Objects));
        state.explorer.instances.cursor = 3;

        // Open discover (matches the header-activation path); the explorer
        // sub-pane and cursor are left untouched.
        open_discover(&mut state);
        assert_eq!(state.focus, Pane::Discover(DiscoverPane::Engine));

        // Closing discover hands focus back to the Explorer (as the original
        // dbm does), preserving the sub-pane and cursor — not resetting to an
        // overview or the header.
        close_discover(&mut state);
        assert_eq!(
            state.focus,
            Pane::Explorer(ExplorerPane::Objects),
            "closing discover returns to the explorer sub-pane the user left"
        );
        assert_eq!(state.explorer.pane, ExplorerPane::Objects);
        assert_eq!(state.explorer.instances.cursor, 3);
    }

    #[test]
    fn discover_close_returns_to_explorer_instances_by_default() {
        use crate::app_shell::nav::ExplorerPane;

        // Even if discover is closed without ever focusing a workspace pane
        // (e.g. right after startup on the header), focus lands on the
        // Explorer's instances sub-pane rather than the SQL workspace.
        let mut state = AppState::default();
        state.focus = Pane::Discover(DiscoverPane::Engine);
        close_discover(&mut state);
        assert_eq!(state.focus, Pane::Explorer(ExplorerPane::Instances));
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

    #[test]
    fn set_explorer_width_updates_and_clamps_the_splitter() {
        let mut state = AppState::default();
        // `SetExplorerWidth` bypasses the focus guard (app-level state).
        update(AppMsg::SetExplorerWidth(40), &mut state);
        assert_eq!(state.splitter.explorer_pane_width, 40);
        // Out-of-range values are clamped on the way in.
        update(AppMsg::SetExplorerWidth(9999), &mut state);
        assert_eq!(
            state.splitter.explorer_pane_width,
            crate::app::splitter::state::MAX_EXPLORER_WIDTH
        );
        update(AppMsg::SetExplorerWidth(0), &mut state);
        assert_eq!(
            state.splitter.explorer_pane_width,
            crate::app::splitter::state::MIN_EXPLORER_WIDTH
        );
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
    fn opening_a_collapsed_unloaded_instance_loads_its_connections() {
        use crate::features::explorer::instances::msg::InstancesMessage;
        use crate::features::explorer::instances::state::ActiveWorkspaceKind;

        let mut state = AppState::default();
        state.explorer.instances.set_instances(vec![sample_managed_instance("inst")]);
        // Collapsed and not loaded (right after startup).
        state.explorer.instances.nodes[0].expanded = false;
        state.explorer.instances.nodes[0].loaded = false;
        state.focus = Pane::Explorer(crate::app_shell::nav::ExplorerPane::default());

        // Selecting the collapsed instance force-expands it (active workspace)
        // and must keep the expanded/loaded state consistent by requesting its
        // connections (a LoadConnections effect).
        let r = update_unchecked(
            explorer_instances_msg(InstancesMessage::Select),
            &mut state,
        );
        assert_eq!(
            state.explorer.instances.active_workspace,
            Some(ActiveWorkspaceKind::Instance(0))
        );
        assert!(state.explorer.instances.nodes[0].expanded);
        assert!(
            !r.effects.is_empty(),
            "force-expanding an unloaded instance must request its connections"
        );
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
