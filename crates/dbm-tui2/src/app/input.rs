//! Keyboard input forwarding.
//!
//! The run loop reads raw key events; global shortcuts are handled there, and
//! everything else is handed to [`key_to_msg`], which maps a key to a feature
//! message. When a modal is open it owns all keys; otherwise the key is routed
//! by the active focus pane. This keeps key parsing centralised in one place
//! (per feature) instead of leaking into each feature's `update`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app_shell::msg::ShellMsg;
use crate::app_shell::nav::{DiscoverPane, IwPane};
use crate::app_shell::pane::Pane;
use crate::app_shell::nav::{pane_dir_from_key, PaneDir};
use crate::features::discover::msg::{DiscoverMessage, DiscoverMsg};
use crate::features::discover::results::msg::{ResultsMessage, ResultsMsg};
use crate::features::discover::state::DiscoverState;
use crate::features::discover::targets::msg::{TargetsMessage, TargetsMsg};
use crate::features::explorer::instances::msg::{InstancesMessage, InstancesMsg};
use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
use crate::features::explorer::objects::msg::{ObjectsMessage, ObjectsMsg};
use crate::features::explorer::state::ExplorerPane;
use crate::features::header::msg::{HeaderMessage, HeaderMsg};
use crate::features::instance_workspace::connections::msg::{ConnectionsMessage, ConnectionsMsg};
use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
use crate::features::instance_workspace::state::IwState;
use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
use crate::features::sql_workspace::state::SqlState;
use crate::features::sql_workspace::sql_tab::editor::context_picker::state::PickerColumn;
use crate::features::sql_workspace::sql_tab::editor::context_picker::msg::{ContextPickerMessage, ContextPickerMsg};
use crate::features::sql_workspace::sql_tab::editor::msg::{EditorMessage, EditorMsg};
use crate::features::sql_workspace::sql_tab::editor::sql_completion::msg::{SqlCompletionMessage, SqlCompletionMsg};
use crate::features::sql_workspace::sql_tab::history::msg::{HistoryMessage, HistoryMsg};
use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
use crate::features::sql_workspace::sql_tab::results::msg::{ResultsMessage as SqlResultsMessage, ResultsMsg as SqlResultsMsg};
use crate::features::sql_workspace::sql_tab::state::SqlFocus;

use super::msg::AppMsg;
use super::state::ModalKind;

/// Map a key to a feature message. A modal (if open) consumes all keys;
/// otherwise the active focus pane routes the key.
///
/// Returns `None` when nothing consumed the key (a no-op). Global shortcuts
/// (quit, theme toggle) are handled by the run loop and not routed here.
pub fn key_to_msg(key: KeyEvent, state: &super::state::AppState) -> Option<AppMsg> {
    // The discover parent pane owns all input while open, including Ctrl+nav
    // (which switches its engine / targets / results sub-panes) and Esc to
    // close it. Check it first so Ctrl+h/j/k/l never fall through to top-level
    // pane navigation while discover is focused.
    if let Pane::Discover(sub) = state.focus {
        return discover_key(key, sub, &state.discover);
    }
    // Pane navigation (Ctrl+h/j/k/l / Ctrl+arrows) is shell-level: it
    // moves the focus pane regardless of the currently focused pane. Check it
    // before modal/focus routing, so it always works. Top-level pane movement
    // wins (so Ctrl+h from the workspace leaves to the explorer); within-workspace
    // sub-pane moves only apply to directions that do not leave the workspace.
    if state.modal.is_none()
        && let Some(dir) = pane_dir_from_key(&key)
    {
        if let Some(msg) = switch_pane_by_dir(dir, state.focus, !state.iw.instance_name.is_empty())
        {
            return Some(msg);
        }
        // Inside the SQL workspace, Ctrl+nav moves the sub-pane focus
        // (editor / results / history) when the move stays within the workspace.
        if state.focus == Pane::SQLWorkspace {
            return switch_subpane(dir, &state.sql);
        }
        return None;
    }
    match &state.modal {
        // Data-carrying popups: route their keys here (esc/n close, y/enter
        // confirms and dispatches the owning feature's action).
        Some(modal) => modal_key(key, modal, state),
        None => match state.focus {
            Pane::Header => header_key(key),
            Pane::Explorer(sub) => explorer_key(key, sub, state.term_width),
            Pane::InstanceWorkspace(sub) => iw_key(key, sub, &state.iw),
            Pane::SQLWorkspace => sql_key(key, &state.sql),
            // Discover is handled above (owns all input while open).
            Pane::Discover(_) => None,
        },
    }
}

/// Move the focus pane one step in `dir`. The explorer is a parent pane whose
/// child sub-pane (instances / objects) is the focused region, so Ctrl+nav
/// cycles within it and crosses to its neighbors (header above, workspace to
/// the right), mirroring the `Discover` parent pane. Workspace/instance leave
/// left to the explorer and up to the header.
///
/// `instance_open` tells whether an instance workspace is currently shown in
/// the workspace region (i.e. `state.iw.instance_name` is non-empty). Moving
/// right from the explorer must land on the *displayed* workspace: the instance
/// workspace when an instance is open, otherwise the SQL workspace — otherwise
/// the focus (SQLWorkspace) no longer matches what is on screen, and Ctrl+nav
/// inside the instance workspace stops working.
fn switch_pane_by_dir(
    dir: crate::app_shell::nav::PaneDir,
    focus: Pane,
    instance_open: bool,
) -> Option<AppMsg> {
    use crate::app_shell::nav::{ExplorerPane, IwPane, PaneDir as D};
    let pane = match (focus, dir) {
        // Header moves down into the explorer (instances by default).
        (Pane::Header, D::Down) => Pane::Explorer(ExplorerPane::default()),
        // Inside the explorer, Up/Down cycle instances <-> objects; Up from
        // instances leaves to the header.
        (Pane::Explorer(ExplorerPane::Instances), D::Up) => Pane::Header,
        (Pane::Explorer(ExplorerPane::Instances), D::Down) => Pane::Explorer(ExplorerPane::Objects),
        (Pane::Explorer(ExplorerPane::Objects), D::Up) => Pane::Explorer(ExplorerPane::Instances),
        // Explorer moves right into the displayed workspace (instance if open).
        (Pane::Explorer(_), D::Right) if instance_open => {
            Pane::InstanceWorkspace(IwPane::Overview)
        }
        (Pane::Explorer(_), D::Right) => Pane::SQLWorkspace,
        // Inside the instance workspace, Left/Right move overview <-> connections
        // (matching the original dbm's manager panes): Connections left ->
        // Overview, Overview left -> explorer (leave), Overview right ->
        // Connections, Connections right stays (no wrap).
        (Pane::InstanceWorkspace(IwPane::Connections), D::Left) => {
            Pane::InstanceWorkspace(IwPane::Overview)
        }
        (Pane::InstanceWorkspace(IwPane::Overview), D::Left) => {
            Pane::Explorer(ExplorerPane::default())
        }
        (Pane::InstanceWorkspace(IwPane::Overview), D::Right) => {
            Pane::InstanceWorkspace(IwPane::Connections)
        }
        (Pane::InstanceWorkspace(IwPane::Connections), D::Right) => {
            Pane::InstanceWorkspace(IwPane::Connections)
        }
        // Workspace leaves left to the explorer and up to the header.
        (Pane::SQLWorkspace, D::Left) => Pane::Explorer(ExplorerPane::default()),
        (Pane::SQLWorkspace, D::Up) => Pane::Header,
        _ => return None,
    };
    tracing::debug!(from = ?focus, to = ?pane, "pane switch via Ctrl+nav");
    Some(AppMsg::Shell(ShellMsg::FocusChanged { pane }))
}

/// Move the active tab's sub-pane focus one step in `dir`, according to the
/// SQL tab's layout (editor on the left; results above history on the right):
/// editor → results via right/down; results ↔ history via down/up; back to the
/// editor via left/up from the right pane. Emits a `SqlTabMessage::Focus` so
/// the change flows through `update`.
fn switch_subpane(dir: crate::app_shell::nav::PaneDir, sql: &SqlState) -> Option<AppMsg> {
    use crate::app_shell::nav::PaneDir;
    use crate::features::sql_workspace::sql_tab::state::SqlFocus;

    let tab = sql.sql_tab.tabs.get(sql.sql_tab.active_tab)?;
    let focus = match (tab.focus, dir) {
        // Editor: right or down enters the results pane (right, top).
        (SqlFocus::Editor, PaneDir::Right | PaneDir::Down) => SqlFocus::Results,
        // Results: down to history, up back to the editor.
        (SqlFocus::Results, PaneDir::Down) => SqlFocus::History,
        (SqlFocus::Results, PaneDir::Up | PaneDir::Left) => SqlFocus::Editor,
        // History: up to results, left back to the editor.
        (SqlFocus::History, PaneDir::Up) => SqlFocus::Results,
        (SqlFocus::History, PaneDir::Left) => SqlFocus::Editor,
        _ => return None,
    };
    Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
        SqlTabMessage::Focus(focus),
    )))))
}

/// Route a bracketed-paste payload to the focused editor cell. The discover
/// targets editor (TSV host:ports rows or text into the in-progress cell) and
/// the SQL editor both accept pasted text; anything else is a no-op.
pub fn paste_to_msg(contents: &str, state: &super::state::AppState) -> Option<AppMsg> {
    if let Pane::Discover(sub) = state.focus {
        // Inside the discover parent pane: only the targets editor accepts paste.
        if sub == DiscoverPane::Targets {
            return Some(AppMsg::Discover(DiscoverMsg::Message(
                DiscoverMessage::Targets(TargetsMsg::Message(TargetsMessage::Paste(
                    contents.to_string(),
                ))),
            )));
        }
        return None;
    }
    if state.modal.is_some() {
        return None;
    }
    // SQL editor focused (no modal): paste into the active tab's buffer.
    let tab_id = state.sql.sql_tab.active_tab;
    if state.focus == Pane::SQLWorkspace
        && state.sql.sql_tab.tabs.get(tab_id).is_some_and(|t| t.focus == SqlFocus::Editor)
    {
        return Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
            SqlTabMessage::Editor {
                tab_id,
                msg: EditorMsg::Message(EditorMessage::Paste {
                    text: contents.to_string(),
                }),
            },
        )))));
    }
    None
}

/// Shared key handling for a Yes/No confirm dialog: `y`/`Y` confirms (running
/// `on_yes`), `n`/`N` cancels (running `on_no`), anything else is unhandled.
/// Both the app-level confirm modals and the discover close-confirmation route
/// their keys through this so the confirm shortcut is defined in one place.
fn confirm_yes_no_key(
    key: KeyEvent,
    on_yes: impl FnOnce() -> Option<AppMsg>,
    on_no: impl FnOnce() -> Option<AppMsg>,
) -> Option<AppMsg> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => on_yes(),
        KeyCode::Char('n') | KeyCode::Char('N') => on_no(),
        _ => None,
    }
}

/// The action dispatched when a confirm modal's `Yes`/`y` is triggered (by key
/// or by clicking the Yes button). The shell closes the modal itself when it
/// sees the dispatched message (e.g. `DeleteConnection` / `UnregisterInstance`),
/// or the action runner does (e.g. a `Commit`). `None` for non-confirm modals.
pub fn confirm_yes_msg(modal: &ModalKind, state: &super::state::AppState) -> Option<AppMsg> {
    use crate::features::sql_workspace::sql_tab::results::msg::ResultsMessage as R;
    match modal {
        ModalKind::ResultsEditCommitPreview { .. } => {
            // Confirm the commit: dispatch Commit to the active tab's results
            // (the modal closes when the commit completes, via `CommitResult`).
            state
                .sql
                .sql_tab
                .tabs
                .get(state.sql.sql_tab.active_tab)
                .map(|t| t.session.id)
                .map(|id| sql_results(R::Commit, id))
        }
        // Confirm deleting a connection: dispatch the delete to the connections
        // panel (the shell closes the modal when it sees DeleteConnection).
        ModalKind::DeleteConnectionConfirm { instance, connection } => {
            Some(confirm_delete_connection(instance.clone(), connection.clone()))
        }
        // Confirm unregistering the current instance: dispatch the unregister
        // (the shell closes the modal when it sees UnregisterInstance).
        ModalKind::UnregisterInstanceConfirm { instance } => {
            Some(close_and_unregister(instance.clone()))
        }
        _ => None,
    }
}

/// Keys for the data-carrying popups (row-limit picker / page input / confirm
/// / commit preview). `Esc` closes; `n`/`N` cancels a confirm; `y`/`Y`/`Enter`
/// confirms and dispatches the owning feature's action (e.g. the commit preview
/// issues a `Commit`). All changes flow through `update` messages.
fn modal_key(key: KeyEvent, modal: &ModalKind, state: &super::state::AppState) -> Option<AppMsg> {
    use super::state::ModalKind;
    use crate::features::sql_workspace::sql_tab::results::msg::ResultsMessage as R;
    let close = || AppMsg::CloseModal;
    let active_tab_id = || {
        state
            .sql
            .sql_tab
            .tabs
            .get(state.sql.sql_tab.active_tab)
            .map(|t| t.session.id)
    };
    match key.code {
        KeyCode::Esc => Some(close()),
        // Row-limit picker: up/down cycle the presets, enter applies.
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Down | KeyCode::Char('j') | KeyCode::Enter => {
            if let ModalKind::ResultsRowLimitPicker { current, limits } = modal {
                if key.code == KeyCode::Enter {
                    return active_tab_id().map(|id| sql_results(R::SetRowLimit { limit: *current }, id));
                }
                let delta = if matches!(key.code, KeyCode::Up | KeyCode::Char('k')) {
                    usize::MAX // -1 wraps below
                } else {
                    1usize
                };
                let idx = limits
                    .iter()
                    .position(|l| *l == *current)
                    .unwrap_or(0);
                let len = limits.len().max(1);
                let next = if delta == 1 {
                    (idx + 1) % len
                } else {
                    (idx + len - 1) % len
                };
                return Some(AppMsg::OpenModal(ModalKind::ResultsRowLimitPicker {
                    current: limits[next],
                    limits: limits.clone(),
                }));
            }
            // Page input: enter applies the shown page.
            if let ModalKind::ResultsPageInput { current_page, .. } = modal {
                return active_tab_id().map(|id| sql_results(R::SetPage { page: *current_page }, id));
            }
            None
        }
        // Confirm modals: `y`/`Y` confirms, `n`/`N` cancels (shared handling).
        _ => confirm_yes_no_key(
            key,
            || confirm_yes_msg(modal, state),
            || {
                if crate::common::view::modal::is_confirm_modal(modal) {
                    Some(close())
                } else {
                    None
                }
            },
        ),
    }
}

/// Key bindings for the Header focus pane: move the button cursor and activate.
fn header_key(key: KeyEvent) -> Option<AppMsg> {
    let msg = match key.code {
        KeyCode::Left => HeaderMessage::MoveLeft,
        KeyCode::Right => HeaderMessage::MoveRight,
        KeyCode::Enter => HeaderMessage::Activate,
        _ => return None,
    };
    Some(AppMsg::Header(HeaderMsg::Message(msg)))
}

/// Discover key bindings, dispatched by the close-confirm flag and the active
/// discover child pane.
fn discover_key(key: KeyEvent, sub: DiscoverPane, state: &DiscoverState) -> Option<AppMsg> {
    let code = key.code;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // Close-confirmation uses the shared Yes/No confirm dialog: `y`/`Y`
    // confirms, `n`/`N` cancels — the same confirm shortcut as every other
    // confirm dialog (see `confirm_yes_no_key`).
    if state.close_confirm {
        return confirm_yes_no_key(
            key,
            || Some(discover(DiscoverMessage::Close)),
            || Some(discover(DiscoverMessage::CancelClose)),
        );
    }

    // While editing a target cell, route every key to the targets editor so
    // `Esc` cancels the edit instead of triggering the discover close-confirm
    // dialog, and text/navigation keys edit the cell rather than firing
    // discover-level actions.
    if sub == DiscoverPane::Targets && state.targets.editing {
        return targets_pane_key(key, state);
    }

    // Pane-move chords (Ctrl+nav) take precedence. The discover panes are
    // stacked vertically (engine / targets / results), so only Up/Down (j/k)
    // move between them — matching the footer hint. Left/Right (h/l) do not.
    if ctrl {
        let dir = pane_dir_from_key(&key);
        return match dir {
            Some(PaneDir::Down) => Some(discover(DiscoverMessage::Focus(sub.next()))),
            Some(PaneDir::Up) => Some(discover(DiscoverMessage::Focus(sub.prev()))),
            _ => None,
        };
    }

    match code {
        KeyCode::Esc => Some(discover(DiscoverMessage::RequestClose)),
        // While a scan is in flight, `c` cancels it (stops at the next host
        // boundary); this mirrors the original dbm's `c: cancel` hint.
        KeyCode::Char('c') if state.scanning => {
            Some(discover(DiscoverMessage::CancelScan))
        }
        // Scan / register are discover-level actions available from any pane.
        // `r` registers normally (blocks on precheck warnings); `R` force-
        // registers (bypasses warnings, errors still block).
        KeyCode::Char('s') => Some(discover(DiscoverMessage::StartScan)),
        KeyCode::Char('r') => {
            tracing::debug!(pane = ?sub, "discover key: register (force=false)");
            Some(discover(DiscoverMessage::RegisterSelected { force: false }))
        }
        KeyCode::Char('R') => {
            tracing::debug!(pane = ?sub, "discover key: force-register (force=true)");
            Some(discover(DiscoverMessage::RegisterSelected { force: true }))
        }
        _ => match sub {
            DiscoverPane::Engine => match code {
                KeyCode::Enter | KeyCode::Char('e') => {
                    Some(discover(DiscoverMessage::Focus(DiscoverPane::Engine)))
                }
                _ => None,
            },
            DiscoverPane::Targets => targets_pane_key(key, state),
            DiscoverPane::Results => results_pane_key(key),
        },
    }
}

fn targets_pane_key(key: KeyEvent, state: &DiscoverState) -> Option<AppMsg> {
    let code = key.code;
    if state.targets.editing {
        return match code {
            KeyCode::Esc => Some(targets(TargetsMessage::CancelEdit)),
            KeyCode::Enter => Some(targets(TargetsMessage::CommitEdit)),
            KeyCode::Backspace => Some(targets(TargetsMessage::EditBackspace)),
            KeyCode::Left => Some(targets(TargetsMessage::EditCursorLeft)),
            KeyCode::Right => Some(targets(TargetsMessage::EditCursorRight)),
            KeyCode::Char(c) if !c.is_control() => Some(targets(TargetsMessage::EditChar(c))),
            _ => None,
        };
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match code {
        KeyCode::Char('i') | KeyCode::Enter => Some(targets(TargetsMessage::BeginEdit)),
        KeyCode::Char('o') => Some(targets(TargetsMessage::AddRow)),
        KeyCode::Char('d') | KeyCode::Delete => Some(targets(TargetsMessage::DeleteRow)),
        KeyCode::Char('u') => Some(targets(TargetsMessage::Undo)),
        KeyCode::Char('r') if ctrl => Some(targets(TargetsMessage::Redo)),
        KeyCode::Up | KeyCode::Char('k') => Some(targets(TargetsMessage::MoveUp)),
        KeyCode::Down | KeyCode::Char('j') => Some(targets(TargetsMessage::MoveDown)),
        KeyCode::Left | KeyCode::Char('h') => Some(targets(TargetsMessage::MoveColHost)),
        KeyCode::Right | KeyCode::Char('l') => Some(targets(TargetsMessage::MoveColPorts)),
        _ => None,
    }
}

fn results_pane_key(key: KeyEvent) -> Option<AppMsg> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(results(ResultsMessage::MoveUp)),
        KeyCode::Down | KeyCode::Char('j') => Some(results(ResultsMessage::MoveDown)),
        KeyCode::Char(' ') => Some(results(ResultsMessage::ToggleSelect)),
        KeyCode::Char('u') => Some(results(ResultsMessage::ToggleUnregisteredFilter)),
        _ => None,
    }
}

fn discover(msg: DiscoverMessage) -> AppMsg {
    AppMsg::Discover(DiscoverMsg::Message(msg))
}
fn targets(msg: TargetsMessage) -> AppMsg {
    AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Targets(TargetsMsg::Message(msg))))
}
fn results(msg: ResultsMessage) -> AppMsg {
    AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Results(
        ResultsMsg::Message(msg),
    )))
}

/// Explorer key bindings, dispatched by the active explorer child pane `sub`.
/// `Tab` (and Ctrl+Up/Down via the shell) moves between the instances and
/// objects panes; that flows through a shell `FocusChanged` so the shell focus
/// and the explorer feature's sub-pane stay in sync.
fn explorer_key(key: KeyEvent, sub: ExplorerPane, term_width: u16) -> Option<AppMsg> {
    // `Tab` toggles between the instances and objects panes.
    if key.code == KeyCode::Tab {
        let next = match sub {
            ExplorerPane::Instances => ExplorerPane::Objects,
            ExplorerPane::Objects => ExplorerPane::Instances,
        };
        return Some(AppMsg::Shell(ShellMsg::FocusChanged {
            pane: Pane::Explorer(next),
        }));
    }
    match sub {
        ExplorerPane::Instances => instances_key(key, term_width),
        ExplorerPane::Objects => objects_key(key, term_width),
    }
}

/// Objects pane keys: navigate the object tree. Expansion is on `Enter`
/// (toggle, or open an object), `h` collapses, and Left/Right scroll
/// horizontally — matching the original dbm.
fn objects_key(key: KeyEvent, term_width: u16) -> Option<AppMsg> {
    let msg = match key.code {
        KeyCode::Up | KeyCode::Char('k') => ObjectsMessage::MoveUp,
        KeyCode::Down | KeyCode::Char('j') => ObjectsMessage::MoveDown,
        KeyCode::Enter => ObjectsMessage::Select,
        KeyCode::Char('h') => ObjectsMessage::Collapse,
        KeyCode::Right => ObjectsMessage::ScrollHorizontal { delta: 1, term_width },
        KeyCode::Left => ObjectsMessage::ScrollHorizontal { delta: -1, term_width },
        _ => return None,
    };
    Some(explorer(ExplorerMessage::Objects(ObjectsMsg::Message(msg))))
}

/// Instances pane keys: navigate the connection tree. Expansion is bound to
/// `l`/`h` and Left/Right to horizontal scroll, matching the original dbm.
fn instances_key(key: KeyEvent, term_width: u16) -> Option<AppMsg> {
    let msg = match key.code {
        KeyCode::Up | KeyCode::Char('k') => InstancesMessage::MoveUp,
        KeyCode::Down | KeyCode::Char('j') => InstancesMessage::MoveDown,
        KeyCode::Enter => InstancesMessage::Select,
        KeyCode::Char('l') => InstancesMessage::Expand,
        KeyCode::Char('h') => InstancesMessage::Collapse,
        KeyCode::Right => InstancesMessage::ScrollHorizontal { delta: 1, term_width },
        KeyCode::Left => InstancesMessage::ScrollHorizontal { delta: -1, term_width },
        _ => return None,
    };
    Some(explorer(ExplorerMessage::Instances(InstancesMsg::Message(msg))))
}

fn explorer(msg: ExplorerMessage) -> AppMsg {
    AppMsg::Explorer(ExplorerMsg::Message(msg))
}

/// Instance workspace key bindings, routed by the active sub-pane `sub`
/// (overview / connections), or the connection form when one is open.
fn iw_key(key: KeyEvent, sub: IwPane, state: &IwState) -> Option<AppMsg> {
    if state.connections.form.is_some() {
        return iw_form_key(key, state);
    }
    // `Tab` cycles the instance-workspace sub-panes (overview <-> connections),
    // mirroring the explorer's Tab behavior.
    if key.code == KeyCode::Tab {
        return Some(AppMsg::Shell(ShellMsg::FocusChanged {
            pane: Pane::InstanceWorkspace(sub.next()),
        }));
    }
    match sub {
        IwPane::Overview => {
            // Overview panel keys: unregister (`u`) opens a confirm modal.
            match key.code {
                KeyCode::Char('u') => {
                    if state.instance_name.is_empty() {
                        return None;
                    }
                    Some(AppMsg::OpenModal(
                        crate::app::state::ModalKind::UnregisterInstanceConfirm {
                            instance: state.instance_name.clone(),
                        },
                    ))
                }
                _ => None,
            }
        }
        IwPane::Connections => {
            // `d` opens a delete-confirm modal for the selected connection
            // (matching the original dbm); the actual delete is dispatched from
            // the modal's `y` key.
            if matches!(key.code, KeyCode::Char('d') | KeyCode::Delete) {
                let Some(connection) = state.connections.selected_name() else {
                    return None;
                };
                return Some(AppMsg::OpenModal(
                    crate::app::state::ModalKind::DeleteConnectionConfirm {
                        instance: state.instance_name.clone(),
                        connection,
                    },
                ));
            }
            let msg = match key.code {
                KeyCode::Up | KeyCode::Char('k') => ConnectionsMessage::MoveUp,
                KeyCode::Down | KeyCode::Char('j') => ConnectionsMessage::MoveDown,
                KeyCode::Char('a') => ConnectionsMessage::BeginAdd,
                KeyCode::Char('e') | KeyCode::Enter => ConnectionsMessage::BeginEdit,
                _ => return None,
            };
            Some(iw(IwMessage::Connections(ConnectionsMsg::Message(msg))))
        }
    }
}

/// Form keys when a connection form is open. Up/Down cycle through all four
/// fields (Name → Username → Database → Password) so every field is reachable.
fn iw_form_key(key: KeyEvent, state: &IwState) -> Option<AppMsg> {
    let current = state
        .connections
        .form
        .as_ref()
        .map(|f| f.field)
        .unwrap_or_default();
    let msg = match key.code {
        KeyCode::Esc => ConnectionsMessage::CancelForm,
        KeyCode::Enter => ConnectionsMessage::CommitForm,
        // Cycle through every field (Name → Username → Database → Password)
        // so all four are reachable. Letters remain free to type in a field.
        KeyCode::Up => ConnectionsMessage::FormField(current.prev()),
        KeyCode::Down | KeyCode::Tab => {
            ConnectionsMessage::FormField(current.next())
        }
        KeyCode::Char(c) if !c.is_control() => ConnectionsMessage::FormChar(c),
        KeyCode::Backspace => ConnectionsMessage::FormBackspace,
        _ => return None,
    };
    Some(iw(IwMessage::Connections(ConnectionsMsg::Message(msg))))
}

fn iw(msg: IwMessage) -> AppMsg {
    AppMsg::Iw(IwMsg::Message(msg))
}

/// Confirm-unregister helper: the shell closes the modal and dispatches the
/// unregister to the instance workspace. The shell closes the modal when it
/// sees the `UnregisterInstance` message.
fn close_and_unregister(instance: String) -> AppMsg {
    iw(IwMessage::UnregisterInstance { instance })
}

/// Confirm-delete-connection helper: dispatch the delete to the connections
/// panel. The shell closes the modal when it sees the `DeleteConnection`
/// message (matching the unregister confirm flow).
fn confirm_delete_connection(instance: String, connection: String) -> AppMsg {
    iw(IwMessage::Connections(ConnectionsMsg::Message(
        ConnectionsMessage::DeleteConnection {
            instance_name: instance,
            connection_name: connection,
        },
    )))
}

/// SQL workspace key bindings, routed to the active tab's editor and its
/// overlays (context picker / completion popup).
fn sql_key(key: KeyEvent, state: &SqlState) -> Option<AppMsg> {
    let tab = state.sql_tab.tabs.get(state.sql_tab.active_tab)?;
    let tab_id = tab.session.id;
    let editor = &tab.editor;

    // The context picker, when open, owns all keys.
    if editor.context_picker.open {
        return sql_context_picker_key(key, tab_id);
    }

    // The completion popup handles selection/apply/close when open.
    if editor.sql_completion.is_open() {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Some(sql_editor(EditorMessage::SqlCompletion(SqlCompletionMsg::Message(
                    SqlCompletionMessage::MoveSelection { delta: -1 },
                )), tab_id));
            }
            KeyCode::Down | KeyCode::Char('j') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Some(sql_editor(EditorMessage::SqlCompletion(SqlCompletionMsg::Message(
                    SqlCompletionMessage::MoveSelection { delta: 1 },
                )), tab_id));
            }
            KeyCode::Enter => {
                return Some(sql_editor(EditorMessage::SqlCompletion(SqlCompletionMsg::Message(
                    SqlCompletionMessage::Apply,
                )), tab_id));
            }
            KeyCode::Esc => {
                return Some(sql_editor(EditorMessage::SqlCompletion(SqlCompletionMsg::Message(
                    SqlCompletionMessage::Close,
                )), tab_id));
            }
            _ => {}
        }
    }

    // Tab-bar / tab management keys (only when the popups are closed).
    if let Some(msg) = sql_tab_navigation_key(key, state) {
        return Some(msg);
    }

    // Vertical-splitter nudges work from any sub-pane: `[` grows the history
    // pane (it owns the right side of the editor/history split), `]` shrinks it.
    if !key.modifiers.contains(KeyModifiers::CONTROL) {
        let nudge = match key.code {
            KeyCode::Char('[') => Some(crate::common::view::splitter::VerticalSplitterNudge::Left),
            KeyCode::Char(']') => Some(crate::common::view::splitter::VerticalSplitterNudge::Right),
            _ => None,
        };
        if let Some(nudge) = nudge {
            return Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                SqlTabMsg::Message(SqlTabMessage::NudgeHistoryWidth { tab_id, nudge }),
            ))));
        }
    }

    // Route the key to the focused sub-pane.
    match tab.focus {
        SqlFocus::Editor => {
            // Ctrl+Enter runs the current editor SQL.
            if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Some(sql_editor(EditorMessage::Run, tab_id));
            }
            // Otherwise forward the key to the editor buffer.
            editor_key(key, tab_id)
        }
        SqlFocus::Results => results_key(key, tab_id, &tab.results),
        SqlFocus::History => history_key(key, tab_id, &tab.history),
    }
}

/// Build a `SqlTabMessage::Results` targeting the given tab.
fn sql_results(msg: SqlResultsMessage, tab_id: usize) -> AppMsg {
    AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
        SqlTabMessage::Results {
            tab_id,
            msg: SqlResultsMsg::Message(msg),
        },
    ))))
}

/// Results sub-pane keys: navigation, editing, and toolbar actions.
///
/// Runs only when the active tab's sub-pane focus is `Results`, so keys here do
/// not collide with the editor's. Produces `ResultsMessage`s (or modal messages
/// via the returned `AppMsg`) and never mutates state directly.
fn results_key(key: KeyEvent, tab_id: usize, results: &crate::features::sql_workspace::sql_tab::results::state::ResultsState) -> Option<AppMsg> {
    // Refresh re-runs the last query using the stored connection context.
    if key.code == KeyCode::Char('r') && key.modifiers.contains(KeyModifiers::CONTROL) {
        let needs = !results.last_sql.is_empty()
            && !results.last_instance.is_empty()
            && !results.last_connection.is_empty();
        return if needs {
            Some(sql_results(
                SqlResultsMessage::RunQuery {
                    instance: results.last_instance.clone(),
                    connection: results.last_connection.clone(),
                    database: results.last_database.clone(),
                    schema: results.last_schema.clone(),
                    sql: results.last_sql.clone(),
                    paginated: results.paginated,
                    page: results.page,
                    row_limit: results.row_limit,
                },
                tab_id,
            ))
        } else {
            None
        };
    }

    match key.code {
        // Toggle edit mode.
        KeyCode::Char('i') if key.modifiers.is_empty() => Some(sql_results(
            SqlResultsMessage::EnterEdit,
            tab_id,
        )),
        // Exit edit mode / clear selection.
        KeyCode::Esc if key.modifiers.is_empty() && results.edit.editing => {
            Some(sql_results(SqlResultsMessage::ExitEdit, tab_id))
        }
        // Commit edits: open a preview modal with the built statements, then
        // `y`/`Enter` confirms and dispatches `Commit`.
        KeyCode::Char('s')
            if key.modifiers.contains(KeyModifiers::CONTROL) && results.edit.editing =>
        {
            let statements = results.build_commit_statements().ok();
            statements.map(|statements| {
                AppMsg::OpenModal(super::state::ModalKind::ResultsEditCommitPreview { statements })
            })
        }
        // Roll back edits.
        KeyCode::Char('u')
            if key.modifiers.contains(KeyModifiers::CONTROL) && results.edit.editing =>
        {
            Some(sql_results(SqlResultsMessage::Rollback, tab_id))
        }
        // Insert / duplicate / delete rows (edit mode only).
        KeyCode::Char('i')
            if key.modifiers.contains(KeyModifiers::ALT) && results.edit.editing =>
        {
            Some(sql_results(SqlResultsMessage::AddRow, tab_id))
        }
        KeyCode::Char('p')
            if key.modifiers.contains(KeyModifiers::ALT) && results.edit.editing =>
        {
            Some(sql_results(SqlResultsMessage::DupRow, tab_id))
        }
        KeyCode::Char('d')
            if key.modifiers.is_empty() && results.edit.editing =>
        {
            Some(sql_results(SqlResultsMessage::DelRow, tab_id))
        }
        // Cell selection via arrows.
        KeyCode::Up => Some(sql_results(SqlResultsMessage::MoveSelection { dr: -1, dc: 0 }, tab_id)),
        KeyCode::Down => Some(sql_results(SqlResultsMessage::MoveSelection { dr: 1, dc: 0 }, tab_id)),
        KeyCode::Left => Some(sql_results(SqlResultsMessage::MoveSelection { dr: 0, dc: -1 }, tab_id)),
        KeyCode::Right => Some(sql_results(SqlResultsMessage::MoveSelection { dr: 0, dc: 1 }, tab_id)),
        // Begin `/` search.
        KeyCode::Char('/') if key.modifiers.is_empty() => {
            Some(sql_results(SqlResultsMessage::BeginSearch, tab_id))
        }
        // Row-limit picker modal.
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::ALT) => {
            Some(AppMsg::OpenModal(super::state::ModalKind::ResultsRowLimitPicker {
                current: results.row_limit,
                limits: crate::features::sql_workspace::sql_tab::results::pagination::RESULTS_ROW_LIMIT_PRESETS
                    .to_vec(),
            }))
        }
        // Page input modal.
        KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::ALT) => {
            let total_pages = crate::features::sql_workspace::sql_tab::results::pagination::max_page(
                results.result.as_ref().and_then(|r| r.total_rows),
                results.row_limit,
            );
            Some(AppMsg::OpenModal(super::state::ModalKind::ResultsPageInput {
                current_page: results.page,
                total_pages,
            }))
        }
        _ => None,
    }
}

/// History sub-pane keys: navigation and apply. Runs only when sub-pane focus
/// is `History`. Produces `HistoryMessage`s without mutating state directly.
fn history_key(
    key: KeyEvent,
    tab_id: usize,
    _history: &crate::features::sql_workspace::sql_tab::history::state::HistoryState,
) -> Option<AppMsg> {
    let msg = match key.code {
        KeyCode::Up => HistoryMessage::MoveCursor { delta: -1 },
        KeyCode::Down => HistoryMessage::MoveCursor { delta: 1 },
        KeyCode::Enter => HistoryMessage::Apply,
        KeyCode::Char('/') if key.modifiers.is_empty() => HistoryMessage::BeginSearch,
        _ => return None,
    };
    Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
        SqlTabMessage::History {
            tab_id,
            msg: HistoryMsg::Message(msg),
        },
    )))))
}

/// Tab-bar navigation keys: switch / open / close tabs.
fn sql_tab_navigation_key(key: KeyEvent, state: &SqlState) -> Option<AppMsg> {
    use crate::features::sql_workspace::sql_tab::msg::SqlTabMessage;
    let count = state.sql_tab.tabs.len();
    if count == 0 {
        return None;
    }
    let active = state.sql_tab.active_tab;
    let tab_msg = match key.code {
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            SqlTabMessage::Tab((active + 1) % count)
        }
        KeyCode::BackTab if key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            SqlTabMessage::Tab((active + count - 1) % count)
        }
        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            SqlTabMessage::CloseTab(active)
        }
        KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            SqlTabMessage::OpenTab
        }
        _ => return None,
    };
    Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(tab_msg)))))
}

/// Keys for the context picker overlay (owns all keys while open).
fn sql_context_picker_key(key: KeyEvent, tab_id: usize) -> Option<AppMsg> {
    let msg = match key.code {
        KeyCode::Esc => ContextPickerMessage::Close,
        KeyCode::Tab => ContextPickerMessage::MoveColumn(PickerColumn::Schema),
        KeyCode::BackTab => ContextPickerMessage::MoveColumn(PickerColumn::Database),
        KeyCode::Enter => ContextPickerMessage::Apply,
        KeyCode::Up | KeyCode::Char('k') => ContextPickerMessage::MoveCursor { delta: -1 },
        KeyCode::Down | KeyCode::Char('j') => ContextPickerMessage::MoveCursor { delta: 1 },
        KeyCode::Left | KeyCode::Char('h') => ContextPickerMessage::MoveColumn(PickerColumn::Database),
        KeyCode::Right | KeyCode::Char('l') => ContextPickerMessage::MoveColumn(PickerColumn::Schema),
        KeyCode::Char('/') => ContextPickerMessage::BeginSearch,
        _ => return None,
    };
    Some(sql_editor(EditorMessage::ContextPicker(ContextPickerMsg::Message(msg)), tab_id))
}

/// Forward a key to the editor buffer (typing / navigation / modal commands).
fn editor_key(key: KeyEvent, tab_id: usize) -> Option<AppMsg> {
    Some(sql_editor(EditorMessage::KeyEvent { key, tracked_caps_lock: false }, tab_id))
}

/// Build an `AppMsg::Sql` message targeting the given tab's editor.
fn sql_editor(msg: EditorMessage, tab_id: usize) -> AppMsg {
    AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
        SqlTabMessage::Editor {
            tab_id,
            msg: EditorMsg::Message(msg),
        },
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sql_workspace::sql_tab::state::SqlTabState;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    /// A `SqlState` with `count` tabs open. The default state opens no tabs, so
    /// we open `count` explicitly (for `count == 0`, an empty tab state).
    fn state_with_tabs(count: usize) -> SqlState {
        let mut tab_state = SqlTabState::default();
        for i in 0..count {
            tab_state.open_connection_tab(
                "local".into(),
                format!("conn-{i}"),
                format!("c{i}"),
                None,
                None,
            );
        }
        SqlState {
            sql_tab: tab_state,
        }
    }

    /// An `AppState` with one open SQL tab (the default has none, since tabs
    /// are only created when a connection is selected).
    fn app_state_with_tab() -> crate::app::state::AppState {
        let mut state = crate::app::state::AppState::default();
        state.sql.sql_tab.open_tab();
        state
    }

    fn extract_tab_msg(msg: AppMsg) -> SqlTabMessage {
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(m)))) => m,
            _ => panic!("expected Sql tab message"),
        }
    }

    #[test]
    fn ctrl_tab_switches_to_next_tab() {
        let state = state_with_tabs(3); // active_tab = 2 (last opened)
        let msg = sql_tab_navigation_key(key(KeyCode::Tab, KeyModifiers::CONTROL), &state)
            .expect("ctrl+tab should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Tab(0));
    }

    #[test]
    fn ctrl_shift_tab_wraps_to_previous_tab() {
        let state = state_with_tabs(2); // active_tab = 1
        let msg = sql_tab_navigation_key(
            key(KeyCode::BackTab, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
            &state,
        )
        .expect("ctrl+shift+tab should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Tab(0));
    }

    #[test]
    fn ctrl_w_closes_active_tab() {
        let state = state_with_tabs(2);
        let msg = sql_tab_navigation_key(key(KeyCode::Char('w'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+w should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::CloseTab(1));
    }

    #[test]
    fn single_tab_switching_wraps_to_itself() {
        let state = state_with_tabs(1);
        let msg = sql_tab_navigation_key(key(KeyCode::Tab, KeyModifiers::CONTROL), &state)
            .expect("ctrl+tab should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Tab(0));
    }

    #[test]
    fn ctrl_j_moves_from_header_to_explorer() {
        let state = crate::app::state::AppState::default();
        assert_eq!(state.focus, Pane::Header);
        let msg = key_to_msg(key(KeyCode::Char('j'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+j should switch pane");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::Explorer(ExplorerPane::default()));
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn ctrl_h_moves_from_workspace_back_to_explorer() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::SQLWorkspace;
        let msg = key_to_msg(key(KeyCode::Char('h'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+h should switch pane");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::Explorer(ExplorerPane::default()));
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn ctrl_l_moves_from_explorer_to_workspace() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::default());
        let msg = key_to_msg(key(KeyCode::Char('l'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+l should switch pane");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::SQLWorkspace);
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn ctrl_j_within_explorer_switches_instances_to_objects() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::Instances);
        let msg = key_to_msg(key(KeyCode::Char('j'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+j inside explorer should switch sub-pane");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::Explorer(ExplorerPane::Objects));
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn ctrl_k_within_explorer_switches_objects_to_instances() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::Objects);
        let msg = key_to_msg(key(KeyCode::Char('k'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+k inside explorer should switch sub-pane");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::Explorer(ExplorerPane::Instances));
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn explorer_instances_l_h_expand_collapse_and_arrows_scroll() {
        use crate::features::explorer::instances::msg::InstancesMessage;
        use crate::features::explorer::msg::ExplorerMessage;

        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::Instances);

        // `l` expands, `h` collapses, arrows scroll (original dbm bindings).
        for (code, expect) in [
            (KeyCode::Char('l'), InstancesMessage::Expand),
            (KeyCode::Char('h'), InstancesMessage::Collapse),
            (KeyCode::Right, InstancesMessage::ScrollHorizontal { delta: 1, term_width: 0 }),
            (KeyCode::Left, InstancesMessage::ScrollHorizontal { delta: -1, term_width: 0 }),
        ] {
            let msg = key_to_msg(key(code, KeyModifiers::NONE), &state).expect("explorer key");
            let got = match msg {
                AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
                    InstancesMsg::Message(m),
                ))) => m,
                other => panic!("unexpected msg for {code:?}: {other:?}"),
            };
            let match_kind = match (&got, &expect) {
                (InstancesMessage::Expand, InstancesMessage::Expand)
                | (InstancesMessage::Collapse, InstancesMessage::Collapse)
                | (InstancesMessage::ScrollHorizontal { .. }, InstancesMessage::ScrollHorizontal { .. }) => {
                    true
                }
                _ => false,
            };
            assert!(match_kind, "for {code:?}: got {got:?}, expected kind {expect:?}");
        }
    }

    #[test]
    fn explorer_objects_h_collapses_and_arrows_scroll() {
        use crate::features::explorer::msg::ExplorerMessage;
        use crate::features::explorer::objects::msg::ObjectsMessage;

        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::Objects);

        for (code, expect) in [
            (KeyCode::Char('h'), ObjectsMessage::Collapse),
            (KeyCode::Right, ObjectsMessage::ScrollHorizontal { delta: 1, term_width: 0 }),
            (KeyCode::Left, ObjectsMessage::ScrollHorizontal { delta: -1, term_width: 0 }),
            (KeyCode::Enter, ObjectsMessage::Select),
        ] {
            let msg = key_to_msg(key(code, KeyModifiers::NONE), &state).expect("explorer key");
            let got = match msg {
                AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Objects(
                    ObjectsMsg::Message(m),
                ))) => m,
                other => panic!("unexpected msg for {code:?}: {other:?}"),
            };
            // Compare discriminant (messages carry payload).
            let match_kind = match (&got, &expect) {
                (ObjectsMessage::Collapse, ObjectsMessage::Collapse)
                | (ObjectsMessage::ScrollHorizontal { .. }, ObjectsMessage::ScrollHorizontal { .. })
                | (ObjectsMessage::Select, ObjectsMessage::Select) => true,
                _ => false,
            };
            assert!(match_kind, "for {code:?}: got {got:?}, expected kind {expect:?}");
        }
    }

    #[test]
    fn ctrl_l_in_instance_workspace_moves_overview_to_connections() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Overview);
        let msg = key_to_msg(key(KeyCode::Char('l'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+l inside instance workspace should move to connections");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::InstanceWorkspace(IwPane::Connections));
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn ctrl_l_from_explorer_enters_instance_workspace_when_instance_open() {
        use crate::features::instance_workspace::state::IwState;

        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::default());
        // An instance is open, so the workspace region shows the instance pane.
        state.iw = IwState {
            instance_name: "inst".into(),
            ..Default::default()
        };
        let msg = key_to_msg(key(KeyCode::Char('l'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+l should move into the instance workspace");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::InstanceWorkspace(IwPane::Overview));
            }
            _ => panic!("expected focus change to instance workspace"),
        }

        // From the instance overview, ctrl+l moves to Connections.
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Overview);
        let msg = key_to_msg(key(KeyCode::Char('l'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+l inside overview should move to connections");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::InstanceWorkspace(IwPane::Connections));
            }
            _ => panic!("expected focus change to connections"),
        }
    }

    #[test]
    fn ctrl_h_in_instance_workspace_moves_connections_to_overview() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Connections);
        let msg = key_to_msg(key(KeyCode::Char('h'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+h inside instance workspace should move to overview");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::InstanceWorkspace(IwPane::Overview));
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn d_in_connections_opens_delete_confirm_modal() {
        use crate::app::state::ModalKind;
        use crate::features::instance_workspace::connections::state::ConnectionsState;
        use dbm_store::InstanceConnection;

        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Connections);
        state.iw.instance_name = "inst".into();
        state.iw.connections = ConnectionsState {
            instance_name: "inst".into(),
            connections: vec![InstanceConnection {
                id: "1".into(),
                instance_id: "1".into(),
                name: "conn".into(),
                username: "u".into(),
                database: "db".into(),
                has_password: false,
                ssl_mode: String::new(),
                env_label: None,
                created_at: String::new(),
                updated_at: String::new(),
            }],
            cursor: 0,
            form: None,
        };
        let msg = key_to_msg(key(KeyCode::Char('d'), KeyModifiers::NONE), &state)
            .expect("d should open the delete-confirm modal");
        match msg {
            AppMsg::OpenModal(ModalKind::DeleteConnectionConfirm { instance, connection }) => {
                assert_eq!(instance, "inst");
                assert_eq!(connection, "conn");
            }
            other => panic!("expected DeleteConnectionConfirm modal, got {other:?}"),
        }
    }

    #[test]
    fn modal_y_confirms_delete_connection() {
        use crate::app::state::ModalKind;
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
        use crate::features::instance_workspace::connections::msg::{
            ConnectionsMessage, ConnectionsMsg,
        };

        let state = crate::app::state::AppState::default();
        let modal = ModalKind::DeleteConnectionConfirm {
            instance: "inst".into(),
            connection: "conn".into(),
        };
        let msg = modal_key(key(KeyCode::Char('y'), KeyModifiers::NONE), &modal, &state)
            .expect("y should confirm delete");
        match msg {
            AppMsg::Iw(IwMsg::Message(IwMessage::Connections(
                ConnectionsMsg::Message(ConnectionsMessage::DeleteConnection {
                    instance_name,
                    connection_name,
                }),
            ))) => {
                assert_eq!(instance_name, "inst");
                assert_eq!(connection_name, "conn");
            }
            other => panic!("expected DeleteConnection, got {other:?}"),
        }
    }

    #[test]
    fn ctrl_h_in_instance_workspace_overview_leaves_to_explorer() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Overview);
        let msg = key_to_msg(key(KeyCode::Char('h'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+h from overview should leave to explorer");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::Explorer(ExplorerPane::default()));
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn ctrl_k_from_explorer_instances_leaves_to_header() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::Instances);
        let msg = key_to_msg(key(KeyCode::Char('k'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+k from explorer instances should leave to header");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::Header);
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn ctrl_h_from_workspace_leaves_to_explorer_even_when_results_focused() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::SQLWorkspace;
        // Put the active tab on Results so the old `switch_subpane` path would
        // have intercepted ctrl+h; top-level nav must still win.
        if let Some(tab) = state.sql.sql_tab.tabs.get_mut(state.sql.sql_tab.active_tab) {
            tab.focus = crate::features::sql_workspace::sql_tab::state::SqlFocus::Results;
        }
        let msg = key_to_msg(key(KeyCode::Char('h'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+h from workspace should leave to explorer");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::Explorer(ExplorerPane::default()));
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn ctrl_j_without_control_is_not_a_pane_move() {
        let state = crate::app::state::AppState::default();
        // Plain 'j' is not a pane-move chord (no Ctrl), so it should not switch
        // the pane; the header key handler does not consume it either.
        assert!(key_to_msg(key(KeyCode::Char('j'), KeyModifiers::NONE), &state).is_none());
    }

    #[test]
    fn ctrl_l_in_sql_workspace_moves_subpane_editor_to_results() {
        // Default tab focus is Editor; Ctrl+l (right) moves editor → results.
        let sql = state_with_tabs(1);
        let msg = switch_subpane(crate::app_shell::nav::PaneDir::Right, &sql)
            .expect("editor right should move to results");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Focus(SqlFocus::Results));
    }

    #[test]
    fn ctrl_j_in_sql_workspace_from_editor_moves_to_results() {
        // Editor down → results (results sits below-right of the editor).
        let sql = state_with_tabs(1);
        let msg = switch_subpane(crate::app_shell::nav::PaneDir::Down, &sql)
            .expect("editor down should move to results");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Focus(SqlFocus::Results));
    }

    #[test]
    fn ctrl_j_from_results_moves_to_history() {
        // Set focus to Results, then Down → history.
        let mut sql = state_with_tabs(1);
        sql.sql_tab.tabs[0].focus = SqlFocus::Results;
        let msg = switch_subpane(crate::app_shell::nav::PaneDir::Down, &sql)
            .expect("results down should move to history");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Focus(SqlFocus::History));
    }

    #[test]
    fn results_key_i_enters_edit_mode() {
        let state = app_state_with_tab();
        let results = &state.sql.sql_tab.tabs[0].results;
        let msg = results_key(key(KeyCode::Char('i'), KeyModifiers::NONE), 0, results)
            .expect("i should enter edit mode");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                tab_id: 0,
                msg: SqlResultsMsg::Message(SqlResultsMessage::EnterEdit),
            }
        );
    }

    #[test]
    fn results_key_ctrl_s_opens_commit_preview_only_when_editing() {
        // With no edit session, Ctrl+s yields no commit preview (no-op).
        let state = app_state_with_tab();
        let results = &state.sql.sql_tab.tabs[0].results;
        assert!(
            results_key(key(KeyCode::Char('s'), KeyModifiers::CONTROL), 0, results).is_none(),
            "ctrl+s with no edit session should be a no-op"
        );
    }

    #[test]
    fn discover_ctrl_j_k_switches_pane_vertically() {
        use crate::features::discover::state::DiscoverState;
        let state = DiscoverState::opened();
        // ctrl+j (Down) moves Engine -> Targets.
        let down = discover_key(
            key(KeyCode::Char('j'), KeyModifiers::CONTROL),
            DiscoverPane::Engine,
            &state,
        )
        .expect("ctrl+j should switch discover pane");
        assert!(matches!(
            down,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Focus(DiscoverPane::Targets)))
        ));
        // ctrl+k (Up) from Engine wraps to Results.
        let up = discover_key(
            key(KeyCode::Char('k'), KeyModifiers::CONTROL),
            DiscoverPane::Engine,
            &state,
        )
        .expect("ctrl+k should switch discover pane");
        assert!(matches!(
            up,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Focus(DiscoverPane::Results)))
        ));
    }

    #[test]
    fn key_to_msg_discover_ctrl_j_switches_subpane_not_pane_nav() {
        // Regression: while discover owns focus, Ctrl+j must reach discover_key
        // (sub-pane switch) rather than be swallowed by top-level pane navigation.
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Discover(DiscoverPane::Engine);
        let msg = key_to_msg(key(KeyCode::Char('j'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+j while discover focused should switch sub-pane");
        assert!(matches!(
            msg,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Focus(DiscoverPane::Targets)))
        ));
    }

    #[test]
    fn discover_r_register_and_r_force_register() {
        use crate::features::discover::state::DiscoverState;
        let state = DiscoverState::opened();
        // `r` registers normally (no force).
        let reg = discover_key(
            key(KeyCode::Char('r'), KeyModifiers::NONE),
            DiscoverPane::Results,
            &state,
        )
        .expect("r should register");
        assert!(matches!(
            reg,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::RegisterSelected { force: false }))
        ));
        // `R` force-registers (bypasses warnings).
        let force = discover_key(
            key(KeyCode::Char('R'), KeyModifiers::NONE),
            DiscoverPane::Results,
            &state,
        )
        .expect("R should force-register");
        assert!(matches!(
            force,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::RegisterSelected { force: true }))
        ));
    }

    #[test]
    fn discover_c_cancels_scan_only_while_scanning() {
        use crate::features::discover::state::DiscoverState;
        let mut state = DiscoverState::opened();
        // Not scanning: `c` is not consumed by discover.
        assert!(
            discover_key(key(KeyCode::Char('c'), KeyModifiers::NONE), DiscoverPane::Results, &state)
                .is_none()
        );
        // Mark a scan in flight; `c` now cancels it.
        state.scanning = true;
        let cancel = discover_key(
            key(KeyCode::Char('c'), KeyModifiers::NONE),
            DiscoverPane::Results,
            &state,
        )
        .expect("c while scanning should cancel");
        assert!(matches!(
            cancel,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::CancelScan))
        ));
    }

    #[test]
    fn paste_routes_to_discover_targets_and_sql_editor() {
        // Discover targets focused inside the discover parent pane -> targets paste.
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Discover(DiscoverPane::Targets);
        let msg = paste_to_msg("1.2.3.4\t5432\n", &state).expect("targets paste should route");
        assert!(matches!(
            msg,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Targets(
                TargetsMsg::Message(TargetsMessage::Paste(_))
            )))
        ));

        // SQL editor focused (no modal) -> editor paste.
        let mut state = app_state_with_tab();
        state.focus = Pane::SQLWorkspace;
        state.sql.sql_tab.tabs[0].focus = SqlFocus::Editor;
        let msg = paste_to_msg("SELECT 1", &state).expect("editor paste should route");
        assert!(matches!(
            msg,
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg: EditorMsg::Message(EditorMessage::Paste { .. }),
                    ..
                }
            ))))
        ));

        // No focused paste target -> no-op.
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Header;
        assert!(paste_to_msg("x", &state).is_none());
    }
}
