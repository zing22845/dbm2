//! Central update dispatcher.
//!
//! `update()` is the heart of the central message router. It takes the
//! global `AppMsg`, mutates the relevant feature state, and collects the
//! `Intent`s and `Effect`s produced by the feature. Child intents/effects
//! are boxed (erasing their concrete feature type) so they can be routed
//! uniformly; the router will later convert them back into `AppMsg` /
//! `Action`.

use ratatui::layout::{Rect, Size};

use crate::app::action::Action;
use crate::app::geometry::{app_explorer_rect, workspace_rect_for_hit};
pub mod chrome;
pub mod discover;
pub mod explorer;
pub mod iw;
pub mod sql;

use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::app_shell::effect::ErasedEffect;
use crate::app_shell::intent::RoutableIntent;
use crate::app_shell::nav::DiscoverPane;
use crate::app_shell::pane::Pane;
use crate::features::explorer::effect::ExplorerEffect;
use crate::features::explorer::objects::effect::ObjectsEffect;
use crate::features::global_footer::view as footer_view;
use crate::features::instance_workspace::msg::{IwMessage, IwMsg};

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
pub(super) fn box_intent(
    i: impl RoutableIntent<AppMsg> + 'static,
) -> Box<dyn RoutableIntent<AppMsg>> {
    Box::new(i)
}

/// Box an effect, erasing its concrete action type into the global `Action`.
pub(super) fn box_effect(e: impl ErasedEffect<Action> + 'static) -> Box<dyn ErasedEffect<Action>> {
    Box::new(e)
}

/// Keep the objects tree bound to the *active* connection, mirroring the
/// original dbm: the objects pane always shows the active workspace's
/// connection. When the active workspace is an instance (or nothing) the tree
/// is unbound so it shows the "open a connection to browse objects" prompt.
/// Returns a `Bind` message when the binding changed and needs a catalog load.
pub(super) fn sync_objects_binding(
    objects: &mut crate::features::explorer::objects::state::ObjectsState,
    instances: &crate::features::explorer::instances::state::InstancesState,
) -> Option<AppMsg> {
    use crate::features::explorer::instances::state::ActiveWorkspaceKind;
    match instances.active_workspace {
        Some(ActiveWorkspaceKind::Connection {
            instance_idx,
            conn_idx,
        }) => {
            let instance = instances.instance_name(instance_idx);
            let connection = instances
                .nodes
                .get(instance_idx)
                .and_then(|n| n.connections.get(conn_idx))
                .map(|c| c.name.clone());
            if let Some(connection) = connection
                && (objects.bound_instance != instance || objects.bound_connection != connection)
            {
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
pub(super) fn sync_objects_active(
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
pub(super) fn focus_pane_of(msg: &AppMsg) -> Option<Pane> {
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
        AppMsg::Iw(_) => Some(Pane::InstanceWorkspace(
            crate::app_shell::nav::IwPane::default(),
        )),
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
pub(super) fn open_discover(state: &mut AppState) {
    tracing::debug!("open_discover: setting focus to Discover parent pane");
    // An edit connection form with unsaved changes blocks leaving the instance
    // workspace entirely (including opening Discover), like the original dbm.
    if connection_form_blocks_focus_change(
        state,
        &Pane::Discover(crate::app_shell::nav::DiscoverPane::Engine),
    ) {
        block_focus_message(state);
        return;
    }
    let preserved_targets = std::mem::take(&mut state.discover.targets).targets;
    // Keep the targets/results splitter width across reopen (process lifetime).
    let preserved_splitter = state.discover.splitter;
    state.discover = crate::features::discover::state::DiscoverState::opened();
    if !preserved_targets.is_empty() {
        state.discover.targets.targets = preserved_targets;
    }
    state.discover.splitter = preserved_splitter;
    // Focus moves to Discover. The explorer sub-pane + cursor (and the sql /
    // iw states) are left untouched; closing discover returns to the Explorer
    // via `set_focus`, so the sub-pane and cursor the user had before opening
    // discover come back unchanged.
    state.focus = Pane::Discover(DiscoverPane::Engine);
    state.modal = None;
}

/// Close the discover modal and restore focus to wherever the user was before
/// it opened (falling back to the SQL workspace if nothing was captured).
pub(super) fn close_discover(state: &mut AppState) {
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
pub(super) fn focus_changed(pane: Pane) -> AppMsg {
    AppMsg::Shell(crate::app_shell::msg::ShellMsg::FocusChanged { pane })
}

/// Notice shown when an edit connection form has unsaved changes and the user
/// tries to switch focus away, mirroring the original dbm's
/// `PANE_SWITCH_BLOCKED_MSG`.
pub(super) const PANE_SWITCH_BLOCKED_MSG: &str =
    "Unsaved changes — save (Enter) or cancel (Esc) before switching pane";

/// True when an open connection *edit* form has unsaved changes and the focus
/// is about to leave the connections sub-pane. Add forms (no baseline) and
/// clean edit forms never block, matching the original dbm's
/// `connection_form_blocks_pane_switch`.
pub(super) fn connection_form_blocks_focus_change(state: &AppState, to: &Pane) -> bool {
    use crate::app_shell::nav::IwPane;
    let Some(form) = &state.iw.connections.form else {
        return false;
    };
    if form.edit_original_name.is_none() {
        return false;
    }
    if !form.is_edit_dirty() {
        return false;
    }
    !matches!(to, Pane::InstanceWorkspace(IwPane::Connections))
}

/// Render the "unsaved changes" notice on the connections footer. Used when a
/// blocked focus switch attempts to leave the form. The caller is responsible
/// for marking the `UpdateResult` dirty so the new status repaints.
pub(super) fn block_focus_message(state: &mut AppState) {
    use crate::features::instance_workspace::connections::state::ConnectionStatusKind;
    state.iw.connections.status = Some(PANE_SWITCH_BLOCKED_MSG.to_string());
    state.iw.connections.status_kind = ConnectionStatusKind::Failure;
}

/// An `AppMsg` that reloads the explorer instance tree from the store.
///
/// Used by the shell after closing the discover modal so instances registered
/// during the scan appear in the explorer immediately.
pub(super) fn explorer_load_instances_msg() -> AppMsg {
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
/// Re-derive each splitter's cached clamp bounds from the live layout.
///
/// The bounds are *derived* data: they depend on the terminal size and the live
/// Explorer / workspace layout, so they are invalidated by a resize or an app
/// splitter move. The run loop fires [`ShellMsg::RefreshSplitterBounds`] right
/// before each repaint and this keeps the keyboard `+`/`-` nudges and mouse
/// drags clamping against the current track. Nothing user-visible changes, so
/// the handler never marks the round dirty.
pub(super) fn reconcile_splitter_bounds(state: &mut AppState) {
    let size = Size::new(state.term_width, state.term_height);
    let footer_h = footer_view::footer_height(&state.footer, size.width);
    // Body height: header (3) at the top, footer at the bottom.
    let body_h = size.height.saturating_sub(3).saturating_sub(footer_h);
    // SQL tab body track: workspace inner, minus the workspace footer and the
    // tab bar. Computed with the same footer logic the workspace render uses.
    let workspace = workspace_rect_for_hit(size, 3, body_h, state);
    // Editor+history track width (the workspace inner width), matching the
    // render (live Explorer splitter width), so `[`/`]` history nudges clamp
    // against the exact editor min width.
    let sql_track_w = workspace.map(|ws| ws.width.saturating_sub(2)).unwrap_or(
        size.width
            .saturating_sub(size.width.saturating_mul(2) / 10)
            .saturating_sub(2),
    );
    let sql_body_h = workspace
        .map(|ws| {
            let inner_w = ws.width.saturating_sub(2);
            let ws_footer_h = crate::common::view::hints::footer_height(
                &crate::common::view::hints::sql_workspace_footer_text(),
                inner_w.max(1),
            );
            ws.height
                .saturating_sub(2)
                .saturating_sub(ws_footer_h)
                .saturating_sub(1)
        })
        .unwrap_or(body_h.saturating_sub(4));
    // Derive the SQL splitter bounds from the layout itself (the same function
    // the render uses), so nudge/drag clamp to the exact rendered boundary.
    for tab in &mut state.sql.sql_tab.tabs {
        let sql_area = Rect::new(0, 0, sql_track_w.max(1), sql_body_h.max(1));
        let layout = crate::features::sql_workspace::sql_tab::layout::sql_tab_layout(
            sql_area,
            tab.splitter.editor_top_height,
            tab.splitter.history_pane_width,
        );
        tab.splitter.editor_top_min = layout.editor_top_min;
        tab.splitter.editor_top_max = layout.editor_top_max.max(layout.editor_top_min);
        tab.splitter.history_min = layout.history_min;
        tab.splitter.history_max = layout.history_max.max(layout.history_min);
    }
    // Explorer instances/objects bounds, derived from the same layout the render
    // uses so nudge clamps to the exact rendered boundary.
    if let Some(explorer) = app_explorer_rect(size, 3, body_h, state) {
        let inner = Rect::new(
            explorer.x + 1,
            explorer.y + 1,
            explorer.width.saturating_sub(2),
            explorer.height.saturating_sub(2),
        );
        let layout = crate::features::explorer::splitter::view::explorer_body_layout(
            inner,
            state.explorer.splitter.instances_height,
        );
        state.explorer.splitter.instances_min = layout.instances_min;
        state.explorer.splitter.instances_max = layout.instances_max.max(layout.instances_min);
    }
    // Discover targets/results bounds, from the same popup/body the render lays.
    if let Some(ws) = workspace {
        let discover_popup = crate::common::view::modal::popup_rect(ws, 75, 75);
        let body =
            crate::features::discover::view::discover_body_area(discover_popup, &state.discover);
        let layout = crate::features::discover::splitter::view::discover_body_layout(
            body,
            state.discover.splitter.targets_height,
        );
        state.discover.splitter.targets_min = layout.targets_min;
        state.discover.splitter.targets_max = layout.targets_max.max(layout.targets_min);
    }
}

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
        AppMsg::Explorer(..) => explorer::apply(msg, state, &mut result),
        AppMsg::Discover(..) => discover::apply(msg, state, &mut result),
        AppMsg::Iw(..) => iw::apply(msg, state, &mut result),
        AppMsg::Sql(..) => sql::apply(msg, state, &mut result),
        AppMsg::OpenModal(..)
        | AppMsg::CloseModal
        | AppMsg::SetExplorerWidth(..)
        | AppMsg::Header(..)
        | AppMsg::Footer(..)
        | AppMsg::Perf(..) => chrome::apply(msg, state, &mut result),
        AppMsg::Shell(shell_msg) => match shell_msg {
            crate::app_shell::msg::ShellMsg::Quit => {
                state.should_quit = true;
            }
            crate::app_shell::msg::ShellMsg::Tick => {}
            crate::app_shell::msg::ShellMsg::TermResized { width, height } => {
                state.term_width = width;
                state.term_height = height;
                result.dirty = true;
            }
            crate::app_shell::msg::ShellMsg::RefreshSplitterBounds => {
                reconcile_splitter_bounds(state);
            }
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
                // An edit connection form with unsaved changes blocks switching
                // to any other pane (keyboard and mouse alike), mirroring the
                // original dbm. Save or cancel must occur before leaving.
                if connection_form_blocks_focus_change(state, &pane) {
                    block_focus_message(state);
                    result.dirty = true;
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
                if let Some(bind) =
                    sync_objects_binding(&mut state.explorer.objects, &state.explorer.instances)
                {
                    result.pending.push_back(bind);
                }
                if let Some(effect) = sync_objects_active(&mut state.explorer.objects, &state.sql) {
                    // Lift the objects effect into an explorer effect so it can
                    // be type-erased against the global action type.
                    result
                        .effects
                        .push(box_effect(ExplorerEffect::Objects(effect)));
                }
            }
            crate::app_shell::msg::ShellMsg::ToggleTheme => {
                // Flip between the theme's dark and light palettes; the next
                // frame is drawn with the new palette automatically.
                state.theme.toggle();
                result.dirty = true;
            }
        },
    }
    result
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_shell::nav::IwPane;

    fn explorer_load_msg() -> AppMsg {
        AppMsg::Explorer(crate::features::explorer::msg::ExplorerMsg::Message(
            crate::features::explorer::msg::ExplorerMessage::Instances(
                crate::features::explorer::instances::msg::InstancesMsg::Message(
                    crate::features::explorer::instances::msg::InstancesMessage::Load,
                ),
            ),
        ))
    }
    fn focus_changed_msg(pane: Pane) -> AppMsg {
        AppMsg::Shell(crate::app_shell::msg::ShellMsg::FocusChanged { pane })
    }
    /// Helper: open an edit connection form with unsaved changes (baseline set,
    /// then a field diverged) so `connection_form_blocks_focus_change` fires.
    fn edit_form_with_unsaved_change(state: &mut AppState) {
        use crate::features::instance_workspace::connections::state::FormField;
        state.iw.connections.connections = vec![sample_connection("c1")];
        state.iw.connections.begin_edit(0);
        // Modify a field after editing began so the form is dirty.
        let form = state.iw.connections.form.as_mut().unwrap();
        form.field = FormField::Name;
        form.name = "renamed".to_string();
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
        assert!(
            effects.is_empty(),
            "duplicate bind must not reload databases"
        );
        assert!(!dirty, "duplicate bind must not mark the view dirty");
        assert!(s.bound_connection == "c1");
    }
    #[test]
    fn focus_change_blocked_while_edit_form_has_unsaved_changes() {
        let mut state = AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Connections);
        edit_form_with_unsaved_change(&mut state);
        let before = state.focus;

        let result = update(
            focus_changed_msg(Pane::Explorer(
                crate::app_shell::nav::ExplorerPane::default(),
            )),
            &mut state,
        );
        // The focus must not move away from the dirty edit form.
        assert_eq!(
            state.focus, before,
            "dirty edit form must block pane switch"
        );
        assert!(
            result.dirty,
            "blocking must repaint so the status notice is shown"
        );
        assert_eq!(
            state.iw.connections.status.as_deref(),
            Some(PANE_SWITCH_BLOCKED_MSG)
        );
    }
    #[test]
    fn focus_change_not_blocked_by_clean_edit_form() {
        let mut state = AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Connections);
        // A clean (unmodified) edit form must not block switching away.
        state.iw.connections.connections = vec![sample_connection("c1")];
        state.iw.connections.begin_edit(0);

        update(
            focus_changed_msg(Pane::Explorer(
                crate::app_shell::nav::ExplorerPane::default(),
            )),
            &mut state,
        );
        assert_eq!(
            state.focus,
            Pane::Explorer(crate::app_shell::nav::ExplorerPane::default()),
            "clean edit form must not block pane switch"
        );
        assert!(state.iw.connections.status.is_none());
    }
    #[test]
    fn focus_change_not_blocked_by_load_form() {
        let mut state = AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Connections);
        // An add form (no baseline) is never dirty and must not block.
        state.iw.connections.begin_add();

        update(
            focus_changed_msg(Pane::Explorer(
                crate::app_shell::nav::ExplorerPane::default(),
            )),
            &mut state,
        );
        assert_eq!(
            state.focus,
            Pane::Explorer(crate::app_shell::nav::ExplorerPane::default()),
            "add form must not block pane switch"
        );
    }
    #[test]
    fn focus_history_not_dirty_when_workspace_already_focused() {
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;
        let focus_msg = |focus| {
            AppMsg::Sql(crate::features::sql_workspace::msg::SqlMsg::Message(
                crate::features::sql_workspace::msg::SqlMessage::SqlTab(
                    crate::features::sql_workspace::sql_tab::msg::SqlTabMsg::Message(
                        crate::features::sql_workspace::sql_tab::msg::SqlTabMessage::Focus(focus),
                    ),
                ),
            ))
        };

        // While the workspace already owns focus, a `Focus(History)` that
        // doesn't change the sub-pane must not dirty — otherwise clicking an
        // already-focused pane renders an identical frame (changed_cells == 0),
        // a redundant repaint. The only dirty signal should come from the
        // sql_tab itself when the sub-pane focus actually changes.
        let mut state = AppState::default();
        state.focus = Pane::SQLWorkspace;
        let result = update_unchecked(focus_msg(SqlFocus::History), &mut state);
        assert!(
            !result.dirty,
            "no-op Focus(History) while the workspace is focused must not dirty"
        );

        // From another pane the same message moves the shell focus and dirtis.
        let mut state = AppState::default();
        state.focus = Pane::Header;
        let result = update_unchecked(focus_msg(SqlFocus::History), &mut state);
        assert!(
            result.dirty,
            "Focus(History) from another pane moves the shell focus and must dirty"
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
