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
use crate::features::discover::engine::msg::{EngineMessage, EngineMsg};
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
use crate::features::instance_workspace::overview::msg::{OverviewMessage, OverviewMsg};
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
    // Uppercase letter jumps (`[S]`/`[I]`/`[O]`/`[H]`/`[R]` in the pane titles)
    // move focus to the matching pane, mirroring the original dbm. They are
    // blocked while typing in the SQL editor (insert mode), so `S`/`H`/`R` do
    // not fire mid-edit; and never while a modal is open.
    if state.modal.is_none()
        && let Some(msg) = pane_jump_from_key(key, state)
    {
        return Some(msg);
    }
    // Pane navigation (Ctrl+h/j/k/l / Ctrl+arrows). Inside the SQL workspace a
    // move that stays within its sub-panes (editor / results / history) is
    // handled first — mirroring the original dbm's `resolve_move`, where the
    // workspace's own neighbor map wins and only a move off the workspace
    // boundary falls through to shell-level pane switching. So History → Left
    // lands on the editor, and only editor → Left leaves to the explorer.
    if state.modal.is_none()
        && let Some(dir) = pane_dir_from_key(&key)
    {
        if state.focus == Pane::SQLWorkspace
            && let Some(msg) = switch_subpane(dir, &state.sql)
        {
            return Some(msg);
        }
        if let Some(msg) = switch_pane_by_dir(
            dir,
            state.focus,
            state.instance_workspace_open(),
            state.explorer.pane,
        ) {
            return Some(msg);
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
/// the workspace region (driven by the explorer tree's active-workspace marker).
/// Moving right from the explorer must land on the *displayed* workspace: the
/// instance workspace when an instance is open, otherwise the SQL workspace —
/// otherwise the focus (SQLWorkspace) no longer matches what is on screen, and
/// Ctrl+nav inside the instance workspace stops working.
fn switch_pane_by_dir(
    dir: crate::app_shell::nav::PaneDir,
    focus: Pane,
    instance_open: bool,
    explorer_pane: ExplorerPane,
) -> Option<AppMsg> {
    use crate::app_shell::nav::{ExplorerPane, IwPane, PaneDir as D};
    let pane = match (focus, dir) {
        // Header moves down into the explorer, restoring its last sub-pane.
        (Pane::Header, D::Down) => Pane::Explorer(explorer_pane),
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
            Pane::Explorer(explorer_pane)
        }
        (Pane::InstanceWorkspace(IwPane::Overview), D::Right) => {
            Pane::InstanceWorkspace(IwPane::Connections)
        }
        (Pane::InstanceWorkspace(IwPane::Connections), D::Right) => {
            Pane::InstanceWorkspace(IwPane::Connections)
        }
        // Workspace leaves left to the explorer (restoring its sub-pane) and up
        // to the header.
        (Pane::SQLWorkspace, D::Left) => Pane::Explorer(explorer_pane),
        (Pane::SQLWorkspace, D::Up) => Pane::Header,
        _ => return None,
    };
    tracing::debug!(from = ?focus, to = ?pane, "pane switch via Ctrl+nav");
    Some(AppMsg::Shell(ShellMsg::FocusChanged { pane }))
}

/// Uppercase-letter pane jump, mirroring the original dbm's `[S]`/`[I]`/`[O]`/
/// `[H]`/`[R]` title shortcuts:
/// - `S` → SQL editor, `H` → History, `R` → Results (workspace sub-panes);
/// - `I` → Instances, `O` → Objects (explorer sub-panes).
///
/// Matching the original dbm, these fire only for an *uppercase* letter (Shift
/// or Caps Lock) with no Ctrl/Alt/Meta, and are suppressed while typing in the
/// SQL editor (insert mode) so `S`/`H`/`R` do not interrupt a query mid-edit.
fn pane_jump_from_key(key: KeyEvent, state: &super::state::AppState) -> Option<AppMsg> {
    use crate::common::utils::shortcuts::{
        caps_lock_active, effective_ascii_letter, pane_jump_modifiers_ok,
    };
    if !pane_jump_modifiers_ok(key.modifiers) {
        return None;
    }
    let KeyCode::Char(c) = key.code else {
        return None;
    };
    let upper = effective_ascii_letter(
        c,
        key.modifiers.contains(KeyModifiers::SHIFT),
        caps_lock_active(&key, false),
    );
    if !upper.is_ascii_uppercase() {
        return None;
    }
    // Suppress all letter jumps while typing in the SQL editor (insert mode),
    // mirroring the original dbm's `workspace_text_input_active`.
    if let Pane::SQLWorkspace = state.focus {
        if let Some(tab) = state
            .sql
            .sql_tab
            .active_tab
            .and_then(|i| state.sql.sql_tab.tabs.get(i))
        {
            if tab.focus == SqlFocus::Editor
                && matches!(tab.editor.editor.mode, edtui::EditorMode::Insert)
            {
                return None;
            }
        }
    }
    match upper {
        // Workspace sub-panes.
        'S' => Some(focus_subpane(SqlFocus::Editor)),
        'H' => Some(focus_subpane(SqlFocus::History)),
        'R' => Some(focus_subpane(SqlFocus::Results)),
        // Explorer sub-panes.
        'I' => Some(focus_explorer(crate::app_shell::nav::ExplorerPane::Instances)),
        'O' => Some(focus_explorer(crate::app_shell::nav::ExplorerPane::Objects)),
        _ => None,
    }
}

/// Build a `SqlTabMessage::Focus` app message for the given workspace sub-pane.
fn focus_subpane(focus: SqlFocus) -> AppMsg {
    AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
        SqlTabMessage::Focus(focus),
    ))))
}

/// Build a `ShellMsg::FocusChanged` app message moving focus to an explorer
/// sub-pane (instances / objects).
fn focus_explorer(sub: crate::app_shell::nav::ExplorerPane) -> AppMsg {
    AppMsg::Shell(ShellMsg::FocusChanged {
        pane: Pane::Explorer(sub),
    })
}

/// Move the active tab's sub-pane focus one step in `dir`, mirroring the
/// original dbm's `workspace_neighbor`:
/// - editor → right: history; editor → down: results;
/// - history → left: editor; history → down: results;
/// - results → up: the previous editor/history pane (`upper_pane`).
/// Returns `None` when the move would leave the workspace (e.g. editor → left,
/// which goes to the explorer; results/history → up/left boundaries).
fn switch_subpane(dir: crate::app_shell::nav::PaneDir, sql: &SqlState) -> Option<AppMsg> {
    use crate::app_shell::nav::PaneDir;
    use crate::features::sql_workspace::sql_tab::state::SqlFocus;

    let tab = sql.sql_tab.active_tab.and_then(|i| sql.sql_tab.tabs.get(i))?;
    let focus = match (tab.focus, dir) {
        (SqlFocus::Editor, PaneDir::Right) => SqlFocus::History,
        (SqlFocus::Editor, PaneDir::Down) => SqlFocus::Results,
        (SqlFocus::History, PaneDir::Left) => SqlFocus::Editor,
        (SqlFocus::History, PaneDir::Down) => SqlFocus::Results,
        // Up from results returns to the pane that was active before entering
        // results (editor or history), matching the original dbm.
        (SqlFocus::Results, PaneDir::Up) => {
            if tab.upper_pane == SqlFocus::History {
                SqlFocus::History
            } else {
                SqlFocus::Editor
            }
        }
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
    let Some(tab_id) = state.sql.sql_tab.active_tab else {
        return None;
    };
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
                .active_tab()
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
    let active_tab_id = || state.sql.sql_tab.active_tab().map(|t| t.session.id);
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
    // Non-navigation Ctrl chords (e.g. Ctrl+r = redo in the targets pane) must
    // fall through to the per-pane handler instead of being swallowed here.
    if ctrl {
        if let Some(dir) = pane_dir_from_key(&key) {
            return match dir {
                PaneDir::Down => Some(discover(DiscoverMessage::Focus(sub.next()))),
                PaneDir::Up => Some(discover(DiscoverMessage::Focus(sub.prev()))),
                _ => None,
            };
        }
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
        // registers (bypasses warnings, errors still block). Ctrl+r / Ctrl+Shift+r
        // are redo in the targets pane, so register only fires without Ctrl.
        KeyCode::Char('s') => Some(discover(DiscoverMessage::StartScan)),
        KeyCode::Char('r') if !ctrl => {
            tracing::debug!(pane = ?sub, "discover key: register (force=false)");
            Some(discover(DiscoverMessage::RegisterSelected { force: false }))
        }
        KeyCode::Char('R') if !ctrl => {
            tracing::debug!(pane = ?sub, "discover key: force-register (force=true)");
            Some(discover(DiscoverMessage::RegisterSelected { force: true }))
        }
        _ => match sub {
            DiscoverPane::Engine => match code {
                // `e`/`Enter` would switch the engine if there were more than
                // one; today it is a no-op, so surface that note on the engine
                // footer instead of silently doing nothing.
                KeyCode::Enter | KeyCode::Char('e') => Some(discover(
                    DiscoverMessage::Engine(EngineMsg::Message(
                        EngineMessage::ShowOnlyEngineNote,
                    )),
                )),
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

/// Objects pane keys: navigate the object tree. `l`/`Enter` toggle/expand the
/// cursor's row (or open/activate it), `h` collapses, and Left/Right scroll
/// horizontally — matching the original dbm.
fn objects_key(key: KeyEvent, term_width: u16) -> Option<AppMsg> {
    let msg = match key.code {
        KeyCode::Up | KeyCode::Char('k') => ObjectsMessage::MoveUp,
        KeyCode::Down | KeyCode::Char('j') => ObjectsMessage::MoveDown,
        // `l` expands the cursor's row (database, schema, or group), matching the
        // instances pane: it only expands, never collapses (`h` collapses).
        // Enter selects: it activates a schema or opens an object, and toggles
        // expandable rows.
        KeyCode::Char('l') => ObjectsMessage::Expand,
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
        // Enter on a connection focuses its already-open tab (or opens one).
        KeyCode::Enter => InstancesMessage::Select,
        // `n` on a connection always opens a fresh SQL editor.
        KeyCode::Char('n') => InstancesMessage::NewConnectionTab,
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
            // Overview panel keys: move the cursor (j/k, ↑/↓), H-Scroll (←/→),
            // and unregister (`u`) which opens a confirm modal.
            let msg = match key.code {
                KeyCode::Up | KeyCode::Char('k') => Some(overview(OverviewMessage::MoveCursor(-1))),
                KeyCode::Down | KeyCode::Char('j') => Some(overview(OverviewMessage::MoveCursor(1))),
                KeyCode::Char('u') => {
                    if state.instance_name.is_empty() {
                        None
                    } else {
                        Some(AppMsg::OpenModal(
                            crate::app::state::ModalKind::UnregisterInstanceConfirm {
                                instance: state.instance_name.clone(),
                            },
                        ))
                    }
                }
                KeyCode::Char('r') => {
                    // Refresh the open instance (matching the original dbm): the
                    // update re-probes lifecycle and reloads overview +
                    // connections. A 1s cooldown set on refresh means a held `r`
                    // fires once and ignores the auto-repeat.
                    if state.instance_name.is_empty()
                        || state
                            .overview
                            .refresh_cooldown_until
                            .is_some_and(|until| std::time::Instant::now() < until)
                    {
                        None
                    } else {
                        Some(iw(IwMessage::Refresh {
                            instance_name: state.instance_name.clone(),
                        }))
                    }
                }
                _ => None,
            };
            msg
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
                // Align with the original dbm: `a` adds, `i` edits the selected
                // connection (no separate `e`/`Enter` binding).
                KeyCode::Char('a') => ConnectionsMessage::BeginAdd,
                KeyCode::Char('i') => ConnectionsMessage::BeginEdit,
                // Test the selected connection (the list's `t`), matching dbm,
                // at most once per second (cooldown set in the update).
                KeyCode::Char('t') => {
                    if state
                        .connections
                        .test_cooldown_until
                        .is_some_and(|until| std::time::Instant::now() < until)
                    {
                        return None;
                    }
                    ConnectionsMessage::TestSelected
                }
                _ => return None,
            };
            Some(iw(IwMessage::Connections(ConnectionsMsg::Message(msg))))
        }
    }
}

/// Form keys when a connection form is open, matching the original dbm: typing
/// happens in an explicit per-field insert mode. In insert mode keys edit the
/// current field (`Enter` commits it, `Esc` reverts it); in normal mode `i`
/// starts editing a field, `j`/`k` move between fields, `Enter` saves the whole
/// connection and `Esc` cancels the form.
fn iw_form_key(key: KeyEvent, state: &IwState) -> Option<AppMsg> {
    use crate::features::instance_workspace::connections::state::FormMode;
    let form = state.connections.form.as_ref()?;
    let insert = form.mode == FormMode::Insert;
    let msg = if insert {
        match key.code {
            // In insert mode, printable characters go straight into the field.
            KeyCode::Char(c) if !c.is_control() => ConnectionsMessage::FormChar(c),
            KeyCode::Backspace => ConnectionsMessage::FormBackspace,
            KeyCode::Enter => ConnectionsMessage::CommitFieldInsert,
            KeyCode::Esc => ConnectionsMessage::CancelFieldInsert,
            _ => return None,
        }
    } else {
        match key.code {
            KeyCode::Char('i') | KeyCode::Char('I') => ConnectionsMessage::BeginFieldInsert,
            KeyCode::Up | KeyCode::Char('k') => ConnectionsMessage::FormField(form.field.prev()),
            KeyCode::Down | KeyCode::Char('j') => ConnectionsMessage::FormField(form.field.next()),
            KeyCode::Enter => ConnectionsMessage::CommitForm,
            KeyCode::Esc => ConnectionsMessage::CancelForm,
            // Test the form's current values against the database, at most once
            // per second (cooldown set in the update).
            KeyCode::Char('t') | KeyCode::Char('T') => {
                if state
                    .connections
                    .test_cooldown_until
                    .is_some_and(|until| std::time::Instant::now() < until)
                {
                    return None;
                }
                ConnectionsMessage::TestForm
            }
            // `dd` clears the current field and enters insert mode: the first
            // `d` arms a short window, a second `d` within it clears.
            KeyCode::Char('d') => {
                let within = form.pending_d_at.is_some_and(|at| {
                    at.elapsed()
                        <= std::time::Duration::from_millis(300)
                });
                if within {
                    ConnectionsMessage::ClearFieldAndInsert
                } else {
                    ConnectionsMessage::SetPendingD
                }
            }
            _ => return None,
        }
    };
    Some(iw(IwMessage::Connections(ConnectionsMsg::Message(msg))))
}

fn iw(msg: IwMessage) -> AppMsg {
    AppMsg::Iw(IwMsg::Message(msg))
}

fn overview(msg: OverviewMessage) -> AppMsg {
    iw(IwMessage::Overview(OverviewMsg::Message(msg)))
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
    let tab = state.sql_tab.active_tab()?;
    let tab_id = tab.session.id;
    let editor = &tab.editor;
    tracing::debug!(
        code = ?key.code,
        modifiers = ?key.modifiers,
        focus = ?tab.focus,
        editor_mode = ?editor.editor.mode,
        "sql_key received"
    );

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
            // A bare Enter applies the highlighted completion; Alt+Enter (or
            // any modified Enter) is NOT consumed here so it falls through to
            // the editor's run-SQL accelerator below.
            KeyCode::Enter if key.modifiers.is_empty() => {
                return Some(sql_editor(EditorMessage::SqlCompletion(SqlCompletionMsg::Message(
                    SqlCompletionMessage::Apply,
                )), tab_id));
            }
            // A bare Tab applies the highlighted completion too (matching the
            // original dbm's `handle_popup_key`) instead of inserting a tab
            // character into the buffer.
            KeyCode::Tab if key.modifiers.is_empty() => {
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
            // `,` opens the database/schema context picker in Normal/Visual mode,
            // matching the original dbm. The picker focuses the schema column.
            if key.code == KeyCode::Char(',')
                && key.modifiers.is_empty()
                && matches!(tab.editor.editor.mode, edtui::EditorMode::Normal | edtui::EditorMode::Visual)
            {
                return Some(sql_editor(
                    EditorMessage::ContextPicker(ContextPickerMsg::Message(
                        ContextPickerMessage::Open {
                            column: PickerColumn::Schema,
                            instance: tab.session.instance.clone().unwrap_or_default(),
                            connection: tab
                                .session
                                .connection
                                .clone()
                                .unwrap_or_default(),
                            database: tab.session.database.clone().unwrap_or_default(),
                            schema: tab.session.schema.clone().unwrap_or_default(),
                        },
                    )),
                    tab_id,
                ));
            }
            // Alt+Enter runs the current editor SQL, matching the original
            // dbm (`run_sql_query`): it works in both Insert and Normal modes.
            // Ctrl+Enter was a redundant duplicate that many terminals fail to
            // report with the CONTROL modifier, so it has been removed.
            if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::ALT) {
                return Some(sql_editor(EditorMessage::Run, tab_id));
            }
            // Alt+Tab toggles table-name completion (TblCmp) in insert mode.
            // Ctrl+T is reserved for theme toggling in dbm2, so the original
            // dbm's Ctrl+T is rebound to Alt+Tab (the editor header shows the
            // TblCmp status).
            if key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::ALT) {
                return Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                    SqlTabMsg::Message(SqlTabMessage::ToggleTableCompletion { tab_id }),
                ))));
            }
            // `Shift+Tab` forces the completion popup open in insert mode,
            // matching the original dbm's `completion_trigger_key`: it accepts
            // either a `BackTab` code or a `Tab` + SHIFT combination (some
            // terminals report Shift+Tab as Tab+SHIFT), and always excludes
            // CONTROL/ALT so it doesn't collide with other shortcuts.
            // A tab-ish key code with the SHIFT modifier: some terminals report
            // Shift+Tab as `BackTab`, others as `Tab`+SHIFT or even `Char('\t')`.
            let is_tab_key = matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('\t'));
            let is_shift_tab = is_tab_key
                && key.modifiers.contains(KeyModifiers::SHIFT)
                && key
                    .modifiers
                    .intersection(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    .is_empty();
            tracing::debug!(
                is_shift_tab,
                insert_mode = matches!(tab.editor.editor.mode, edtui::EditorMode::Insert),
                "completion trigger check"
            );
            // The original dbm only triggers completion from insert mode.
            if is_shift_tab
                && matches!(tab.editor.editor.mode, edtui::EditorMode::Insert)
            {
                return Some(sql_editor(EditorMessage::ForceCompletion, tab_id));
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
    // `Alt+t` opens a new tab and must work even with no tabs yet, so it is
    // handled before the empty guard below.
    if key.code == KeyCode::Char('t') && key.modifiers.contains(KeyModifiers::ALT) {
        return Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
            SqlTabMessage::OpenTab,
        )))));
    }
    let count = state.sql_tab.visible_tab_count();
    if count == 0 {
        return None;
    }
    let visible_active = state.sql_tab.global_to_visible().unwrap_or(0);
    let tab_msg = match key.code {
        // Plain `Shift+Tab` (BackTab) is not tab navigation — in the editor it
        // forces the completion popup open (see `sql_workspace_key`).
        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            SqlTabMessage::CloseTab(visible_active)
        }
        // `Alt+n` / `Alt+p` move to the next / previous visible tab.
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::ALT) => {
            SqlTabMessage::Tab((visible_active + 1) % count)
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::ALT) => {
            SqlTabMessage::Tab((visible_active + count - 1) % count)
        }
        // `Alt+1..9` jumps directly to the nth visible tab (1-based).
        KeyCode::Char(c)
            if key.modifiers.contains(KeyModifiers::ALT)
                && c.is_ascii_digit()
                && c != '0' =>
        {
            let idx = (c as u8 - b'1') as usize;
            if idx >= count {
                return None;
            }
            SqlTabMessage::Tab(idx)
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
    /// All tabs share the same connection so they are all visible.
    fn state_with_tabs(count: usize) -> SqlState {
        let mut tab_state = SqlTabState::default();
        for _ in 0..count {
            tab_state.open_connection_tab(
                "local".into(),
                "main-db".into(),
                "c1".into(),
                None,
                None,
                None,
            );
        }
        SqlState {
            sql_tab: tab_state,
        }
    }

    /// An `AppState` with one open SQL tab (the default has none, since tabs
    /// are only created when a connection is selected). The tab is bound to a
    /// connection so it appears in the tab bar.
    fn app_state_with_tab() -> crate::app::state::AppState {
        let mut state = crate::app::state::AppState::default();
        state.sql.sql_tab.open_connection_tab(
            "local".into(),
            "main-db".into(),
            "c1".into(),
            None,
            None,
            None,
        );
        state
    }

    fn extract_tab_msg(msg: AppMsg) -> SqlTabMessage {
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(m)))) => m,
            _ => panic!("expected Sql tab message"),
        }
    }

    #[test]
    fn ctrl_tab_is_not_tab_navigation() {
        // `Ctrl+Tab` is no longer bound to tab switching; use `Alt+n` for the
        // next tab instead (there was never a footer hint advertising it).
        let state = state_with_tabs(3);
        assert!(
            sql_tab_navigation_key(key(KeyCode::Tab, KeyModifiers::CONTROL), &state).is_none(),
            "Ctrl+Tab must not switch tabs anymore"
        );
    }

    #[test]
    fn ctrl_shift_tab_is_not_tab_navigation() {
        // `Ctrl+Shift+Tab` is no longer bound to tab switching; use `Alt+p`
        // for the previous tab instead.
        let state = state_with_tabs(2);
        assert!(
            sql_tab_navigation_key(
                key(KeyCode::BackTab, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
                &state,
            )
            .is_none(),
            "Ctrl+Shift+Tab must not switch tabs anymore"
        );
    }

    #[test]
    fn alt_p_switches_to_previous_tab() {
        let state = state_with_tabs(3);
        let count = state.sql_tab.visible_tab_count();
        let visible_active = state.sql_tab.global_to_visible().unwrap_or(0);
        assert_eq!(visible_active, 2);
        let msg = sql_tab_navigation_key(key(KeyCode::Char('p'), KeyModifiers::ALT), &state)
            .expect("alt+p should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Tab((visible_active + count - 1) % count));
    }

    #[test]
    fn plain_shift_tab_is_not_tab_navigation() {
        // Plain Shift+Tab (BackTab without Ctrl) is no longer tab navigation:
        // in the editor it forces completion. So it must be None here.
        let state = state_with_tabs(2);
        assert!(
            sql_tab_navigation_key(key(KeyCode::BackTab, KeyModifiers::SHIFT), &state).is_none(),
            "plain Shift+Tab must not switch tabs"
        );
    }

    fn force_completion_msg(state: &SqlState, key: KeyEvent) -> AppMsg {
        sql_key(key, state).expect("Shift+Tab in insert-mode editor should force completion")
    }

    #[test]
    fn shift_tab_in_editor_forces_completion() {
        let mut state = state_with_tabs(1);
        // The completion trigger only fires in insert mode (original dbm).
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        let msg = force_completion_msg(&state, key(KeyCode::BackTab, KeyModifiers::SHIFT));
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg: EditorMsg::Message(EditorMessage::ForceCompletion),
                    ..
                },
            )))) => {}
            other => panic!("expected ForceCompletion, got {other:?}"),
        }
    }

    #[test]
    fn tab_plus_shift_also_forces_completion() {
        // Some terminals report Shift+Tab as `Tab` + SHIFT rather than BackTab;
        // both forms must trigger completion (original dbm).
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        let msg = force_completion_msg(&state, key(KeyCode::Tab, KeyModifiers::SHIFT));
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg: EditorMsg::Message(EditorMessage::ForceCompletion),
                    ..
                },
            )))) => {}
            other => panic!("expected ForceCompletion, got {other:?}"),
        }
    }

    #[test]
    fn char_tab_plus_shift_also_forces_completion() {
        // Some terminals report Shift+Tab as `Char('\t')` + SHIFT; that form
        // must also force completion in insert mode.
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        let msg = sql_key(key(KeyCode::Char('\t'), KeyModifiers::SHIFT), &state)
            .expect("char-tab+shift should force completion");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg: EditorMsg::Message(EditorMessage::ForceCompletion),
                    ..
                },
            )))) => {}
            other => panic!("expected ForceCompletion, got {other:?}"),
        }
    }

    #[test]
    fn shift_tab_in_normal_mode_does_not_force_completion() {
        // The original dbm only triggers completion from insert mode.
        let state = state_with_tabs(1); // default editor mode is normal
        let msg = sql_key(key(KeyCode::BackTab, KeyModifiers::SHIFT), &state);
        assert!(
            !matches!(
                msg,
                Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Editor {
                        msg: EditorMsg::Message(EditorMessage::ForceCompletion),
                        ..
                    },
                )))))
            ),
            "Shift+Tab in normal mode must not force completion"
        );
    }

    #[test]
    fn alt_tab_in_editor_emits_toggle_table_completion() {
        // Alt+Tab toggles TblCmp (Ctrl+T is reserved for theme toggling in
        // dbm2). In insert mode it must toggle table-name completion.
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        let msg = sql_key(key(KeyCode::Tab, KeyModifiers::ALT), &state)
            .expect("alt+tab should be handled in the editor");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::ToggleTableCompletion { .. },
            )))) => {}
            other => panic!("expected ToggleTableCompletion, got {other:?}"),
        }
    }

    #[test]
    fn comma_in_normal_mode_opens_the_context_picker() {
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Normal;
        let msg = sql_key(key(KeyCode::Char(','), KeyModifiers::NONE), &state)
            .expect("comma should open the context picker in normal mode");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor { .. },
            )))) => {}
            other => panic!("expected an editor message (context picker open), got {other:?}"),
        }
    }

    #[test]
    fn alt_enter_in_normal_mode_runs_sql() {
        // Alt+Enter is the original dbm's run-SQL accelerator; it must work in
        // Normal mode without an open completion popup.
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Normal;
        let msg = sql_key(key(KeyCode::Enter, KeyModifiers::ALT), &state)
            .expect("alt+enter should run the editor SQL in normal mode");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg: EditorMsg::Message(EditorMessage::Run),
                    ..
                },
            )))) => {}
            other => panic!("expected EditorMessage::Run, got {other:?}"),
        }
    }

    #[test]
    fn alt_enter_in_insert_mode_runs_sql() {
        // Alt+Enter must also run SQL while the editor is in Insert mode.
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        let msg = sql_key(key(KeyCode::Enter, KeyModifiers::ALT), &state)
            .expect("alt+enter should run the editor SQL in insert mode");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg: EditorMsg::Message(EditorMessage::Run),
                    ..
                },
            )))) => {}
            other => panic!("expected EditorMessage::Run, got {other:?}"),
        }
    }

    #[test]
    fn alt_enter_with_completion_open_still_runs_sql() {
        // When the completion popup is open, a bare Enter applies the
        // completion, but Alt+Enter must fall through to run the SQL.
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        // Open the completion popup with one item so `is_open()` is true.
        state.sql_tab.tabs[0].editor.sql_completion = {
            use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::{
                CompletionItem, CompletionKind,
            };
            let mut sc = crate::features::sql_workspace::sql_tab::editor::sql_completion::state::SqlCompletionState::default();
            sc.open = true;
            sc.items = vec![CompletionItem {
                label: "customers".into(),
                kind: CompletionKind::Table,
                detail: None,
                insert_text: "customers".into(),
            }];
            sc
        };
        let msg = sql_key(key(KeyCode::Enter, KeyModifiers::ALT), &state)
            .expect("alt+enter should run SQL even with the completion popup open");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg: EditorMsg::Message(EditorMessage::Run),
                    ..
                },
            )))) => {}
            other => panic!("expected EditorMessage::Run, got {other:?}"),
        }
    }

    #[test]
    fn tab_with_completion_open_applies_completion() {
        // When the completion popup is open, a bare Tab applies the highlighted
        // completion (matching the original dbm's `handle_popup_key`) instead
        // of inserting a tab character into the buffer.
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        state.sql_tab.tabs[0].editor.sql_completion = {
            use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::{
                CompletionItem, CompletionKind,
            };
            let mut sc = crate::features::sql_workspace::sql_tab::editor::sql_completion::state::SqlCompletionState::default();
            sc.open = true;
            sc.items = vec![CompletionItem {
                label: "customers".into(),
                kind: CompletionKind::Table,
                detail: None,
                insert_text: "customers".into(),
            }];
            sc
        };
        let msg = sql_key(key(KeyCode::Tab, KeyModifiers::NONE), &state)
            .expect("bare tab with the completion popup open should apply completion");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg: EditorMsg::Message(EditorMessage::SqlCompletion(SqlCompletionMsg::Message(
                        SqlCompletionMessage::Apply,
                    ))),
                    ..
                },
            )))) => {}
            other => panic!("expected SqlCompletionMessage::Apply, got {other:?}"),
        }
    }

    #[test]
    fn ctrl_w_closes_active_tab() {
        let state = state_with_tabs(2);
        let msg = sql_tab_navigation_key(key(KeyCode::Char('w'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+w should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::CloseTab(1));
    }

    #[test]
    fn single_tab_alt_n_switching_wraps_to_itself() {
        // With a single tab, `Alt+n` (next tab) wraps back to the same tab.
        let state = state_with_tabs(1);
        let msg = sql_tab_navigation_key(key(KeyCode::Char('n'), KeyModifiers::ALT), &state)
            .expect("alt+n should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Tab(0));
    }

    #[test]
    fn alt_digit_switches_to_nth_tab() {
        let state = state_with_tabs(3); // 3 visible tabs
        // Alt+1 -> tab index 0, Alt+2 -> tab index 1, Alt+3 -> tab index 2.
        let msg = sql_tab_navigation_key(key(KeyCode::Char('1'), KeyModifiers::ALT), &state)
            .expect("alt+1 should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Tab(0));
        let msg = sql_tab_navigation_key(key(KeyCode::Char('3'), KeyModifiers::ALT), &state)
            .expect("alt+3 should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Tab(2));
    }

    #[test]
    fn alt_digit_out_of_range_is_ignored() {
        let state = state_with_tabs(2); // only 2 tabs
        // Alt+9 exceeds the tab count, so it must not be handled.
        assert!(
            sql_tab_navigation_key(key(KeyCode::Char('9'), KeyModifiers::ALT), &state).is_none(),
            "alt+9 with only 2 tabs must be ignored"
        );
    }

    #[test]
    fn alt_t_opens_new_tab_even_without_tabs() {
        // `Alt+t` opens a tab and must work even when there are no tabs yet.
        let state = state_with_tabs(0);
        let msg = sql_tab_navigation_key(key(KeyCode::Char('t'), KeyModifiers::ALT), &state)
            .expect("alt+t should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::OpenTab);
    }

    #[test]
    fn alt_n_and_alt_p_cycle_tabs() {
        let state = state_with_tabs(3);
        let count = state.sql_tab.visible_tab_count();
        assert_eq!(count, 3);
        // The last opened tab is active, so `visible_active` is 2.
        let visible_active = state.sql_tab.global_to_visible().unwrap_or(0);
        assert_eq!(visible_active, 2);
        // Alt+n -> next tab (wrap from last back to first).
        let msg = sql_tab_navigation_key(key(KeyCode::Char('n'), KeyModifiers::ALT), &state)
            .expect("alt+n should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Tab((visible_active + 1) % count));
        // Alt+p -> previous tab.
        let msg = sql_tab_navigation_key(key(KeyCode::Char('p'), KeyModifiers::ALT), &state)
            .expect("alt+p should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Tab((visible_active + count - 1) % count));
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

        // `l` expands, `h` collapses, arrows scroll, Enter selects, `n` opens a
        // fresh editor (original dbm bindings).
        for (code, expect) in [
            (KeyCode::Char('l'), InstancesMessage::Expand),
            (KeyCode::Char('h'), InstancesMessage::Collapse),
            (KeyCode::Right, InstancesMessage::ScrollHorizontal { delta: 1, term_width: 0 }),
            (KeyCode::Left, InstancesMessage::ScrollHorizontal { delta: -1, term_width: 0 }),
            (KeyCode::Enter, InstancesMessage::Select),
            (KeyCode::Char('n'), InstancesMessage::NewConnectionTab),
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
                | (InstancesMessage::ScrollHorizontal { .. }, InstancesMessage::ScrollHorizontal { .. })
                | (InstancesMessage::Select, InstancesMessage::Select)
                | (InstancesMessage::NewConnectionTab, InstancesMessage::NewConnectionTab) => {
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
            (KeyCode::Char('l'), ObjectsMessage::Expand),
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
                | (ObjectsMessage::Select, ObjectsMessage::Select)
                | (ObjectsMessage::Expand, ObjectsMessage::Expand) => true,
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
        // An instance is active, so the workspace region shows the instance pane.
        state.explorer.instances.set_active_instance(0);
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
                test_succeeded_at: None,
                test_failed_at: None,
            }],
            cursor: 0,
            restore_cursor: None,
            form: None,
            status: None,
            status_kind: crate::features::instance_workspace::connections::state::ConnectionStatusKind::Idle,
            test_cooldown_until: None,
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
    fn overview_jk_move_cursor_and_arrows_hscroll() {
        use crate::features::instance_workspace::overview::msg::{OverviewMessage, OverviewMsg};
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Overview);
        // j -> MoveCursor(1)
        let msg = key_to_msg(key(KeyCode::Char('j'), KeyModifiers::NONE), &state)
            .expect("j in overview should move cursor down");
        assert!(matches!(
            msg,
            AppMsg::Iw(IwMsg::Message(IwMessage::Overview(OverviewMsg::Message(
                OverviewMessage::MoveCursor(1)
            ))))
        ));
        // k -> MoveCursor(-1)
        let msg = key_to_msg(key(KeyCode::Char('k'), KeyModifiers::NONE), &state)
            .expect("k in overview should move cursor up");
        assert!(matches!(
            msg,
            AppMsg::Iw(IwMsg::Message(IwMessage::Overview(OverviewMsg::Message(
                OverviewMessage::MoveCursor(-1)
            ))))
        ));
    }

    #[test]
    fn overview_r_refreshes_instance() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Overview);
        state.iw.instance_name = "inst-a".to_string();
        let msg = key_to_msg(key(KeyCode::Char('r'), KeyModifiers::NONE), &state)
            .expect("r in overview should refresh the instance");
        assert!(matches!(
            msg,
            AppMsg::Iw(IwMsg::Message(IwMessage::Refresh { instance_name }))
                if instance_name == "inst-a"
        ));
    }

    #[test]
    fn overview_r_honors_refresh_cooldown() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Overview);
        state.iw.instance_name = "inst-a".to_string();
        // Within the cooldown -> `r` is a no-op (no refresh message).
        state.iw.overview.refresh_cooldown_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
        assert!(key_to_msg(key(KeyCode::Char('r'), KeyModifiers::NONE), &state).is_none());
        // Once the cooldown has passed -> `r` refreshes again.
        state.iw.overview.refresh_cooldown_until = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
        assert!(key_to_msg(key(KeyCode::Char('r'), KeyModifiers::NONE), &state).is_some());
    }

    #[test]
    fn connections_t_honors_test_cooldown() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Connections);
        state.iw.instance_name = "inst".to_string();
        // Within the test cooldown -> `t` is ignored (no test).
        state.iw.connections.test_cooldown_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
        assert!(key_to_msg(key(KeyCode::Char('t'), KeyModifiers::NONE), &state).is_none());
        // Once the cooldown has passed -> `t` dispatches the test.
        state.iw.connections.test_cooldown_until =
            Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
        assert!(key_to_msg(key(KeyCode::Char('t'), KeyModifiers::NONE), &state).is_some());
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
        if let Some(tab) = state
            .sql
            .sql_tab
            .active_tab
            .and_then(|i| state.sql.sql_tab.tabs.get_mut(i))
        {
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
    fn uppercase_s_jumps_to_sql_editor() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::default()); // jump from another pane
        let msg = key_to_msg(key(KeyCode::Char('S'), KeyModifiers::SHIFT), &state)
            .expect("uppercase S should jump to the SQL editor");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Focus(f),
            )))) => assert_eq!(f, SqlFocus::Editor),
            other => panic!("expected Focus(Editor), got {other:?}"),
        }
    }

    #[test]
    fn uppercase_h_and_r_jump_to_history_and_results() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::SQLWorkspace;
        let msg = key_to_msg(key(KeyCode::Char('H'), KeyModifiers::SHIFT), &state)
            .expect("uppercase H should jump to history");
        assert!(matches!(
            msg,
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Focus(f)
            )))) if f == SqlFocus::History
        ));
        let msg = key_to_msg(key(KeyCode::Char('R'), KeyModifiers::SHIFT), &state)
            .expect("uppercase R should jump to results");
        assert!(matches!(
            msg,
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Focus(f)
            )))) if f == SqlFocus::Results
        ));
    }

    #[test]
    fn uppercase_i_and_o_jump_to_explorer_panes() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::SQLWorkspace;
        let msg = key_to_msg(key(KeyCode::Char('I'), KeyModifiers::SHIFT), &state)
            .expect("uppercase I should jump to instances");
        assert!(matches!(
            msg,
            AppMsg::Shell(ShellMsg::FocusChanged { pane: Pane::Explorer(ExplorerPane::Instances) })
        ));
        let msg = key_to_msg(key(KeyCode::Char('O'), KeyModifiers::SHIFT), &state)
            .expect("uppercase O should jump to objects");
        assert!(matches!(
            msg,
            AppMsg::Shell(ShellMsg::FocusChanged { pane: Pane::Explorer(ExplorerPane::Objects) })
        ));
    }

    #[test]
    fn jump_suppressed_while_typing_in_sql_editor_insert_mode() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::SQLWorkspace;
        state.sql.sql_tab.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            None,
            None,
            None,
        );
        state.sql.sql_tab.tabs[0].focus = SqlFocus::Editor;
        state.sql.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        // While typing (insert mode), uppercase S must NOT jump away (it becomes
        // a normal keystroke), so the result must not be a Focus/pane-jump msg.
        let msg = key_to_msg(key(KeyCode::Char('S'), KeyModifiers::SHIFT), &state);
        assert!(
            !matches!(
                msg,
                Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                    SqlTabMsg::Message(SqlTabMessage::Focus(_))
                ))))
            ),
            "S while typing in the editor must not jump to another pane"
        );
    }

    #[test]
    fn lowercase_letters_do_not_jump() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::SQLWorkspace;
        // Plain lowercase s/i/o/h/r (no Shift/Caps) must not trigger a jump.
        for c in ['s', 'i', 'o', 'h', 'r'] {
            assert!(
                key_to_msg(key(KeyCode::Char(c), KeyModifiers::NONE), &state).is_none(),
                "plain lowercase {c} must not jump"
            );
        }
    }

    #[test]
    fn ctrl_l_in_sql_workspace_moves_subpane_editor_to_history() {
        // Default tab focus is Editor; Ctrl+l (right) moves editor → history,
        // matching the original dbm's `workspace_neighbor`.
        let sql = state_with_tabs(1);
        let msg = switch_subpane(crate::app_shell::nav::PaneDir::Right, &sql)
            .expect("editor right should move to history");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Focus(SqlFocus::History));
    }

    #[test]
    fn workspace_left_returns_to_explorer_remembered_subpane() {
        use crate::app_shell::nav::{ExplorerPane, PaneDir};
        // From the SQL workspace going Left, the explorer is restored at its
        // remembered sub-pane (Objects) rather than resetting to Instances.
        let msg = switch_pane_by_dir(PaneDir::Left, Pane::SQLWorkspace, false, ExplorerPane::Objects)
            .expect("workspace left should return to the explorer");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane: Pane::Explorer(sub) }) => {
                assert_eq!(sub, ExplorerPane::Objects)
            }
            other => panic!("expected explorer, got {other:?}"),
        }
    }

    #[test]
    fn ctrl_l_from_editor_does_not_leave_workspace() {
        // Editor → left leaves to the explorer (shell-level), so switch_subpane
        // returns None for it.
        let sql = state_with_tabs(1);
        assert!(
            switch_subpane(crate::app_shell::nav::PaneDir::Left, &sql).is_none(),
            "editor left must fall through to the explorer"
        );
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
    fn ctrl_j_from_results_does_not_move() {
        // The original dbm has no Down neighbor for Results, so it returns None
        // (falls through to shell-level switching).
        let mut sql = state_with_tabs(1);
        sql.sql_tab.tabs[0].focus = SqlFocus::Results;
        assert!(
            switch_subpane(crate::app_shell::nav::PaneDir::Down, &sql).is_none(),
            "results down must not move within the workspace"
        );
    }

    #[test]
    fn ctrl_k_from_results_returns_to_upper_pane() {
        // Results → Up returns to the previous editor/history pane (upper_pane).
        let mut sql = state_with_tabs(1);
        sql.sql_tab.tabs[0].focus = SqlFocus::Results;
        sql.sql_tab.tabs[0].upper_pane = SqlFocus::History;
        let msg = switch_subpane(crate::app_shell::nav::PaneDir::Up, &sql)
            .expect("results up should return to history");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Focus(SqlFocus::History));
        // Default upper_pane is editor.
        sql.sql_tab.tabs[0].upper_pane = SqlFocus::Editor;
        let msg = switch_subpane(crate::app_shell::nav::PaneDir::Up, &sql)
            .expect("results up should return to editor");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Focus(SqlFocus::Editor));
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
    fn discover_engine_e_enter_shows_only_engine_note() {
        use crate::features::discover::engine::msg::{EngineMessage, EngineMsg};
        use crate::features::discover::state::DiscoverState;
        let state = DiscoverState::opened();
        // `e` on the engine pane is not a focus move (it is already focused);
        // it surfaces the "only Postgres" note instead of silently doing nothing.
        let e = discover_key(
            key(KeyCode::Char('e'), KeyModifiers::NONE),
            DiscoverPane::Engine,
            &state,
        )
        .expect("e on engine pane should dispatch the engine note");
        assert!(matches!(
            e,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Engine(
                EngineMsg::Message(EngineMessage::ShowOnlyEngineNote)
            )))
        ));
        // Enter behaves the same way.
        let enter = discover_key(key(KeyCode::Enter, KeyModifiers::NONE), DiscoverPane::Engine, &state)
            .expect("Enter on engine pane should dispatch the engine note");
        assert!(matches!(
            enter,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Engine(
                EngineMsg::Message(EngineMessage::ShowOnlyEngineNote)
            )))
        ));
    }

    #[test]
    fn discover_ctrl_r_in_targets_redoes() {
        use crate::features::discover::state::DiscoverState;
        use crate::features::discover::targets::msg::{TargetsMessage, TargetsMsg};
        let state = DiscoverState::opened();
        // Ctrl+r must reach the targets pane's Redo, not be swallowed by the
        // discover Ctrl-navigation branch (non-nav Ctrl chords fall through).
        let msg = discover_key(
            key(KeyCode::Char('r'), KeyModifiers::CONTROL),
            DiscoverPane::Targets,
            &state,
        )
        .expect("ctrl+r in targets should dispatch Redo");
        assert!(matches!(
            msg,
            AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Targets(
                TargetsMsg::Message(TargetsMessage::Redo)
            )))
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
