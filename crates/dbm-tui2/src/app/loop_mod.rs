//! The run loop and top-level message scheduling.
//!
//! `run_event_loop` wires together:
//!   - crossterm terminal events  -> `AppMsg`
//!   - the central `update`        -> intents + effects
//!   - the `EffectRunner`          -> `Action`s (fed back via a channel)
//!   - the intent router           -> `AppMsg`s (fed back into a queue)
//!   - a periodic tick             -> redraw
//!
//! Cascade handling is **iterative, not recursive**. An update pass may
//! produce `Intent`s that resolve to further `AppMsg`s; those are pushed onto
//! a `VecDeque` and drained in a loop, so arbitrarily deep (or cyclic)
//! cascades never grow the call stack. A per-round depth cap bounds the work
//! done in a single event round so a logic bug cannot turn into an unbounded
//! hot loop (a livelock), though it does not mask an infinite cascade.

use std::collections::VecDeque;
use std::time::Duration;

use crossterm::event::{Event as CEvent, EventStream, KeyCode};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::app::action::Action;
use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::app::update::{handle_action, update, UpdateResult};
use crate::app::view::render;
use crate::app_shell::effect::EffectRunner;
use crate::app_shell::focus::FocusZone;
use crate::app_shell::intent::IntentRouter;
use crate::features::global_footer::view as footer_view;
use crate::features::header::msg::{HeaderMessage, HeaderMsg};
use crate::features::perf_monitor::backend::CountingBackend;

const TICK_RATE: Duration = Duration::from_millis(250);
/// Upper bound on how many messages are processed in a single event round
/// (the triggering event + the cascade it starts). Guards against livelock
/// caused by a cyclic intent cascade; it does not otherwise change behavior.
const MAX_MESSAGES_PER_ROUND: usize = 100;

/// Run the application. Sets up the terminal, the channels and the event
/// loop, then tears the terminal down on exit.
pub async fn run_event_loop() -> anyhow::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    // The backend is wrapped in a `CountingBackend` so the redundancy metric
    // can read how many cells each frame actually changed.
    let backend = CountingBackend::new(CrosstermBackend::new(stdout));
    let mut terminal = Terminal::new(backend)?;

    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<Action>();
    // The composition root creates the shared service bundle once and injects
    // it into the effect runner, which hands it to each effect as it runs.
    let services = crate::common::service::services::Services::new()?;
    let effect_runner = {
        let services = std::sync::Arc::new(services);
        let (runner, handle) = EffectRunner::new(action_tx.clone(), services.clone());
        tokio::spawn(handle.run());
        runner
    };

    let mut state = AppState::default();
    // Restore a previously persisted session (open tabs + focus) before the
    // first frame so the shell comes up where the user left it.
    if let Err(e) = crate::app::session::restore_session(&mut state) {
        tracing::warn!("failed to restore TUI session: {e}");
    }
    let mut reader = EventStream::new();
    let mut tick = tokio::time::interval(TICK_RATE);

    // Populate the explorer tree on startup.
    process_message_round(
        &effect_runner,
        &mut action_rx,
        AppMsg::Explorer(crate::features::explorer::msg::ExplorerMsg::Message(
            crate::features::explorer::msg::ExplorerMessage::Instances(
                crate::features::explorer::instances::msg::InstancesMsg::Message(
                    crate::features::explorer::instances::msg::InstancesMessage::Load,
                ),
            ),
        )),
        &mut state,
    );

    // Which SQL-tab splitter is being drag-resized, if any. This is transient
    // interaction state that lives only for the lifetime of a drag gesture; it
    // never reaches `AppState` (TEA: state mutations still flow through
    // `update` via split-resize messages).
    let mut split_drag: Option<crate::features::sql_workspace::sql_tab::layout::SqlSplitter> = None;

    loop {
        // Draw the current frame. The perf_monitor feature is passive: the run
        // loop samples each frame here and feeds the smoothed FPS and
        // redundant-redraw ratio from the wrapped backend.
        terminal.draw(|frame| render(frame, &state))?;
        let changed_cells = terminal.backend_mut().last_changed_cells();
        state.perf.record_frame();
        state.perf.record_redundancy(changed_cells);

        // Wait for the next input: a terminal event, a tick, or an async
        // action (from an effect).
        tokio::select! {
            maybe_event = reader.next() => {
                if let Some(Ok(CEvent::Key(key))) = maybe_event {
                    use crate::app_shell::msg::ShellMsg;
                    use crossterm::event::KeyModifiers as KM;
                    let ctrl = key.modifiers.contains(KM::CONTROL);
                    let global = match (ctrl, key.code) {
                        // `q` or `CTRL+D` quits through the standard message
                        // flow so the handler sets the quit flag.
                        (_, KeyCode::Char('q')) | (true, KeyCode::Char('d')) => {
                            Some(AppMsg::Shell(ShellMsg::Quit))
                        }
                        // `CTRL+T` toggles the active theme (dark/light).
                        (true, KeyCode::Char('t')) => {
                            Some(AppMsg::Shell(ShellMsg::ToggleTheme))
                        }
                        // Not a global shortcut: let the focused feature handle
                        // the key below.
                        _ => None,
                    };
                    let msg = global.or_else(|| crate::app::input::key_to_msg(key, &state));
                    if let Some(msg) = msg {
                        process_message_round(&effect_runner, &mut action_rx, msg, &mut state);
                    }
                    // Neither a global shortcut nor a focused-feature key is a
                    // no-op: nothing to dispatch.
                } else if let Some(Ok(CEvent::Mouse(mouse))) = maybe_event {
                    use crossterm::event::{MouseButton, MouseEventKind};
                    use ratatui::prelude::Position;
                    tracing::debug!(
                        kind = ?mouse.kind,
                        col = mouse.column,
                        row = mouse.row,
                        "mouse event received"
                    );
                    let point = Position::new(mouse.column, mouse.row);
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) if state.modal.is_none() => {
                            // Map the click to a focus zone by region. The layout
                            // mirrors `app/view.rs`: header (top 3 rows), explorer
                            // (left 20% of the body), workspace (right 80%).
                            let size = terminal.size()?;
                            let footer_h = footer_view::footer_height(&state.footer, size.width);
                            let body_top = 3u16;
                            let body_h =
                                size.height.saturating_sub(body_top).saturating_sub(footer_h);
                            let explorer_w = (size.width.saturating_mul(2) / 10).max(1);

                            // Clicking a region moves focus there (shell-level).
                            let target_zone = if mouse.row < body_top {
                                Some(FocusZone::Header)
                            } else if mouse.row >= body_top + body_h {
                                None
                            } else if mouse.column < explorer_w {
                                Some(FocusZone::Explorer)
                            } else if state.iw.instance_name.is_empty() {
                                Some(FocusZone::SQLWorkspace)
                            } else {
                                Some(FocusZone::InstanceWorkspace)
                            };
                            tracing::debug!(
                                point = ?point,
                                target_zone = ?target_zone,
                                current_focus = ?state.focus,
                                "mouse click zone mapping"
                            );
                            if let Some(zone) = target_zone.filter(|z| *z != state.focus) {
                                let msg = AppMsg::Shell(
                                    crate::app_shell::msg::ShellMsg::FocusChanged { zone },
                                );
                                process_message_round(
                                    &effect_runner,
                                    &mut action_rx,
                                    msg,
                                    &mut state,
                                );
                            }

                            // Left-click on the header `Discover` button activates
                            // it, in addition to moving focus to the header.
                            let header_area = Rect::new(0, 0, size.width, 3);
                            let button_rect =
                                crate::features::header::view::discover_button_rect(header_area);
                            let clicked = button_rect.is_some_and(|r| r.contains(point));
                            tracing::debug!(clicked, "header button click resolved");
                            if clicked {
                                // Clicking the header button is an explicit user
                                // intent: move focus to the Header zone (via the
                                // shell message), then dispatch Activate. Both go
                                // through `update` so every state change flows
                                // through the single state-transition channel.
                                process_message_round(
                                    &effect_runner,
                                    &mut action_rx,
                                    AppMsg::Shell(crate::app_shell::msg::ShellMsg::FocusChanged {
                                        zone: crate::app_shell::focus::FocusZone::Header,
                                    }),
                                    &mut state,
                                );
                                process_message_round(
                                    &effect_runner,
                                    &mut action_rx,
                                    AppMsg::Header(HeaderMsg::Message(HeaderMessage::Activate)),
                                    &mut state,
                                );
                                tracing::debug!("dispatching HeaderMessage::Activate");
                            }

                            // Starting a drag on a SQL-tab splitter begins a
                            // resize gesture (only when the SQL workspace owns
                            // focus and it is actually rendered).
                            if state.focus == FocusZone::SQLWorkspace
                                && let Some((layout, _tab_id)) =
                                    sql_tab_layout_for_hit(terminal.size()?, &state)
                                && let Some(splitter) = layout.splitter_at(point.x, point.y)
                            {
                                split_drag = Some(splitter);
                                tracing::debug!(?splitter, "splitter drag started");
                            }
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            if let Some(splitter) = split_drag {
                                let size = terminal.size()?;
                                if let Some((layout, tab_id)) = sql_tab_layout_for_hit(size, &state) {
                                    // Compute the new split value from the mouse
                                    // position and dispatch a resize message (all
                                    // state changes flow through `update`).
                                    let msg = split_resize_msg(splitter, point, layout, tab_id);
                                    process_message_round(
                                        &effect_runner,
                                        &mut action_rx,
                                        msg,
                                        &mut state,
                                    );
                                }
                            }
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            if split_drag.take().is_some() {
                                tracing::debug!("splitter drag finished");
                            }
                        }
                        _ => {
                            tracing::debug!(
                                is_left_down = matches!(
                                    mouse.kind,
                                    MouseEventKind::Down(MouseButton::Left)
                                ),
                                modal_open = state.modal.is_some(),
                                "mouse event ignored"
                            );
                        }
                    }
                }
            }
            _ = tick.tick() => {
                let msg = AppMsg::Shell(crate::app_shell::msg::ShellMsg::Tick);
                process_message_round(&effect_runner, &mut action_rx, msg, &mut state);
            }
            maybe_action = action_rx.recv() => {
                if let Some(action) = maybe_action {
                    process_action_round(&effect_runner, &mut action_rx, action, &mut state);
                }
            }
        }

        if state.should_quit {
            break;
        }
    }

    // Persist the session before tearing the terminal down so a relaunch
    // restores the open tabs. Best-effort: a write failure is logged, not fatal.
    if let Err(e) = crate::app::session::persist_session(&state) {
        tracing::warn!("failed to save TUI session: {e}");
    }

    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    Ok(())
}

/// Compute the active SQL tab's body layout for mouse hit-testing, using the
/// tab's stored split values. Returns the layout and the tab's session id, or
/// `None` when the SQL workspace is not currently rendered (a modal is open,
/// the instance workspace is showing, or no tab is active).
fn sql_tab_layout_for_hit(
    size: ratatui::layout::Size,
    state: &AppState,
) -> Option<(crate::features::sql_workspace::sql_tab::layout::SqlTabLayout, usize)> {
    use crate::features::sql_workspace::sql_tab::layout::sql_tab_layout;

    if state.modal.is_some() || !state.iw.instance_name.is_empty() {
        return None;
    }
    let tab = state.sql.sql_tab.tabs.get(state.sql.sql_tab.active_tab)?;
    let footer_h = footer_view::footer_height(&state.footer, size.width);
    let body_top = 3u16;
    let body_h = size.height.saturating_sub(body_top).saturating_sub(footer_h);
    if body_h < 3 {
        return None;
    }
    let explorer_w = (size.width.saturating_mul(2) / 10).max(1);
    let workspace_w = size.width.saturating_sub(explorer_w);
    let workspace = Rect::new(explorer_w, body_top, workspace_w, body_h);
    // The SQL tab's body sits below its 1-row tab bar within the workspace.
    let sql_body = Rect::new(
        workspace.x,
        workspace.y + 1,
        workspace.width,
        workspace.height.saturating_sub(1),
    );
    let layout = sql_tab_layout(sql_body, tab.split_ratio, tab.history_pane_width);
    if layout.editor.width == 0 {
        return None;
    }
    Some((layout, tab.session.id))
}

/// Build the split-resize message for a drag gesture at `point`, using the
/// current layout as the reference frame.
fn split_resize_msg(
    splitter: crate::features::sql_workspace::sql_tab::layout::SqlSplitter,
    point: ratatui::prelude::Position,
    layout: crate::features::sql_workspace::sql_tab::layout::SqlTabLayout,
    tab_id: usize,
) -> AppMsg {
    use crate::features::sql_workspace::sql_tab::layout::SqlSplitter;
    use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
    use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};

    let msg = match splitter {
        SqlSplitter::EditorResults => {
            // The splitter's row becomes the top-pane height; express it as a
            // percent of the body height so the ratio survives terminal resizes.
            let body_top = layout.editor.y;
            let body_h = layout.results.bottom().saturating_sub(body_top).max(1);
            let top_h = point.y.saturating_sub(body_top);
            let ratio = ((u32::from(top_h) * 100) / u32::from(body_h)).min(99) as u8;
            SqlTabMessage::SetSplitRatio { tab_id, ratio }
        }
        SqlSplitter::EditorHistory => {
            // The history pane owns the right side; its width is the distance
            // from the splitter to the right edge of the top row.
            let right_edge = layout.history.right();
            let width = right_edge.saturating_sub(point.x);
            SqlTabMessage::SetHistoryWidth { tab_id, width }
        }
    };
    AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(msg))))
}

/// A round triggered by an external message (keyboard/tick). The seed message
/// and any already-available async actions are queued and drained iteratively.
fn process_message_round(
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    seed: AppMsg,
    state: &mut AppState,
) {
    let mut pending: VecDeque<AppMsg> = VecDeque::new();
    pending.push_back(seed);
    drain_async_actions(action_rx, &mut pending);
    drain_pending(effect_runner, &mut pending, state);
}

/// A round triggered by an async action (an effect result). The action is
/// applied via `handle_action`; its intents/effects are consumed by the same
/// iterative drain as a message round.
fn process_action_round(
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    seed: Action,
    state: &mut AppState,
) {
    let mut pending: VecDeque<AppMsg> = VecDeque::new();
    let result = handle_action(seed, state);
    queue_result(effect_runner, result, &mut pending);
    drain_async_actions(action_rx, &mut pending);
    drain_pending(effect_runner, &mut pending, state);
}

/// Iteratively apply every queued message until the queue is empty or the
/// per-round budget is exhausted. Intents produced by an update resolve to new
/// `AppMsg`s that are pushed back onto the queue, replacing recursion with a
/// heap-backed work list.
fn drain_pending(
    effect_runner: &EffectRunner<Action>,
    pending: &mut VecDeque<AppMsg>,
    state: &mut AppState,
) {
    let mut processed = 0usize;
    while let Some(msg) = pending.pop_front() {
        if processed >= MAX_MESSAGES_PER_ROUND {
            tracing::warn!(
                "message round budget ({MAX_MESSAGES_PER_ROUND}) exhausted; \
                 dropping remaining cascade ({} messages)",
                pending.len()
            );
            break;
        }
        processed += 1;

        let result = update(msg, state);
        queue_result(effect_runner, result, pending);
    }
}

/// Submit an update result's effects to the runner and push each routed intent
/// back onto the pending queue. Intents are programmatic cross-feature
/// requests; the messages they resolve to are applied without the focus guard
/// because delivery is not keyboard input.
fn queue_result(
    effect_runner: &EffectRunner<Action>,
    result: UpdateResult,
    pending: &mut VecDeque<AppMsg>,
) {
    for effect in result.effects {
        effect_runner.submit(effect);
    }
    for intent in result.intents {
        let nested = IntentRouter::route::<AppMsg>(intent);
        pending.push_back(nested);
    }
    pending.extend(result.pending);
}

/// Convert an effect-produced action into the message(s) it should dispatch
/// back into the router.
///
/// This is the single conversion used by both the message-round drain and the
/// action-round seed (`handle_action`), so an async action is handled
/// identically no matter how it arrives — eliminating the previous race where
/// a feature action received as a `recv()` seed was dropped by `handle_action`
/// and never drained.
pub(crate) fn action_to_app_msgs(action: Action) -> Vec<AppMsg> {
    match action {
        Action::Dispatch(msg) => vec![msg],
        Action::Shell(crate::app_shell::action::ShellAction::Quit) => {
            vec![AppMsg::Shell(crate::app_shell::msg::ShellMsg::Quit)]
        }
        // The discover feature's scan/register actions feed back into the
        // discover modal as messages.
        Action::Discover(action) => vec![AppMsg::Discover(
            crate::features::discover::msg::DiscoverMsg::Message(discover_action_to_msg(action)),
        )],
        // The explorer's load actions feed back into the explorer.
        Action::Explorer(action) => vec![AppMsg::Explorer(
            crate::features::explorer::msg::ExplorerMsg::Message(explorer_action_to_msg(action)),
        )],
        // The instance workspace's load/save/delete actions feed back.
        Action::Iw(action) => vec![AppMsg::Iw(
            crate::features::instance_workspace::msg::IwMsg::Message(iw_action_to_msg(action)),
        )],
        // The SQL workspace's catalog-load actions feed back into the targeted
        // tab's editor (the context picker). A commit completion also closes
        // the commit-preview modal.
        Action::Sql(action) => {
            use crate::features::sql_workspace::effect::SqlAction;
            use crate::features::sql_workspace::sql_tab::effect::SqlTabAction;
            use crate::features::sql_workspace::sql_tab::results::effect::ResultsAction;
            let mut msgs = Vec::new();
            if matches!(
                &action,
                SqlAction::SqlTab(SqlTabAction::Results {
                    action: ResultsAction::CommitResult { .. },
                    ..
                })
            ) {
                msgs.push(AppMsg::CloseModal);
            }
            msgs.push(AppMsg::Sql(
                crate::features::sql_workspace::msg::SqlMsg::Message(sql_action_to_msg(action)),
            ));
            msgs
        }
        // Header/footer/perf effects are currently stateless; nothing to route.
        Action::Header(_) | Action::Footer(_) | Action::Perf(_) => Vec::new(),
    }
}

/// Move any already-available async actions out of the channel and into the
/// queue as dispatched messages. Does not await; only drains what is ready so
/// the event loop never blocks on effects.
fn drain_async_actions(
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    pending: &mut VecDeque<AppMsg>,
) {
    loop {
        match action_rx.try_recv() {
            Ok(action) => pending.extend(action_to_app_msgs(action)),
            Err(mpsc::error::TryRecvError::Empty) => break,
            Err(mpsc::error::TryRecvError::Disconnected) => break,
        }
    }
}

/// Convert a discover action into the corresponding discover message.
fn discover_action_to_msg(action: crate::features::discover::effect::DiscoverAction) -> crate::features::discover::msg::DiscoverMessage {
    use crate::features::discover::effect::DiscoverAction as A;
    use crate::features::discover::msg::DiscoverMessage as M;
    match action {
        A::ScanProgress { done, total } => M::ScanProgress { done, total },
        A::ScanComplete { items } => M::ScanComplete { items },
        A::ScanError { error } => M::ScanError { error },
        A::RegisterComplete { count } => M::RegisterComplete { count },
        A::RegisterError { error } => M::RegisterError { error },
    }
}

/// Convert an instance workspace action into the corresponding iw message.
fn iw_action_to_msg(action: crate::features::instance_workspace::effect::IwAction) -> crate::features::instance_workspace::msg::IwMessage {
    use crate::features::instance_workspace::connections::effect::ConnectionsAction as CA;
    use crate::features::instance_workspace::connections::msg::{ConnectionsMessage, ConnectionsMsg};
    use crate::features::instance_workspace::effect::IwAction as IA;
    use crate::features::instance_workspace::msg::IwMessage as IM;
    use crate::features::instance_workspace::overview::effect::OverviewAction as OA;
    use crate::features::instance_workspace::overview::msg::{OverviewMessage, OverviewMsg};
    match action {
        IA::Overview(action) => match action {
            OA::Loaded { instance } => IM::Overview(OverviewMsg::Message(OverviewMessage::Loaded {
                instance,
            })),
            OA::Error { error } => {
                tracing::warn!("iw overview load failed: {error}");
                IM::Overview(OverviewMsg::Message(OverviewMessage::Reload))
            }
        },
        IA::Connections(action) => match action {
            CA::Loaded { connections } => {
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::Loaded { connections }))
            }
            CA::Saved | CA::Deleted => {
                // Reload the connections after a mutation (the connections
                // update fills in the current instance name).
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::Reload))
            }
            CA::Error { error } => {
                tracing::warn!("iw connections op failed: {error}");
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::MoveUp))
            }
        },
    }
}

/// Convert a SQL workspace action into the corresponding workspace message,
/// routing editor actions back to the originating tab's editor (the context
/// picker's catalog results).
fn sql_action_to_msg(action: crate::features::sql_workspace::effect::SqlAction) -> crate::features::sql_workspace::msg::SqlMessage {
    use crate::features::sql_workspace::effect::SqlAction as SA;
    use crate::features::sql_workspace::sql_tab::effect::SqlTabAction as STA;
    use crate::features::sql_workspace::sql_tab::editor::effect::EditorAction as EA;
    use crate::features::sql_workspace::sql_tab::editor::context_picker::msg::ContextPickerMsg;
    use crate::features::sql_workspace::sql_tab::editor::msg::{EditorMessage, EditorMsg};
    use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
    use crate::features::sql_workspace::msg::SqlMessage;
    match action {
        SA::SqlTab(STA::Editor { tab_id, action }) => {
            let msg = match action {
                EA::ContextPicker(cp) => EditorMsg::Message(EditorMessage::ContextPicker(
                    ContextPickerMsg::Message(cp_action_to_msg(cp)),
                )),
            };
            SqlMessage::SqlTab(SqlTabMsg::Message(SqlTabMessage::Editor { tab_id, msg }))
        }
        SA::SqlTab(STA::Results { tab_id, action }) => {
            let results_msg = results_action_to_msg(action);
            SqlMessage::SqlTab(SqlTabMsg::Message(SqlTabMessage::Results {
                tab_id,
                msg: crate::features::sql_workspace::sql_tab::results::msg::ResultsMsg::Message(
                    results_msg,
                ),
            }))
        }
    }
}

/// Convert a results action into the corresponding results message.
fn results_action_to_msg(action: crate::features::sql_workspace::sql_tab::results::effect::ResultsAction) -> crate::features::sql_workspace::sql_tab::results::msg::ResultsMessage {
    use crate::features::sql_workspace::sql_tab::results::effect::ResultsAction as RA;
    use crate::features::sql_workspace::sql_tab::results::msg::ResultsMessage as M;
    match action {
        RA::ResultReady { result, paginated } => M::SetResult { result, paginated },
        RA::QueryError { message } => {
            tracing::warn!("query failed: {message}");
            M::ClearResult
        }
        RA::CommitResult { ok, message } => {
            tracing::info!("commit ok={ok}: {message}");
            M::ResetSelection
        }
        RA::EditabilityReady { target, blocked } => M::EditabilityReady { target, blocked },
    }
}

/// Convert a context picker action into the corresponding picker message.
fn cp_action_to_msg(action: crate::features::sql_workspace::sql_tab::editor::context_picker::effect::ContextPickerAction) -> crate::features::sql_workspace::sql_tab::editor::context_picker::msg::ContextPickerMessage {
    use crate::features::sql_workspace::sql_tab::editor::context_picker::effect::ContextPickerAction as CPA;
    use crate::features::sql_workspace::sql_tab::editor::context_picker::msg::ContextPickerMessage as M;
    match action {
        CPA::DatabasesLoaded { items } => M::DatabasesLoaded { items },
        CPA::DatabasesError { error } => M::DatabasesError { error },
        CPA::SchemasLoaded { items } => M::SchemasLoaded { items },
        CPA::SchemasError { error } => M::SchemasError { error },
    }
}

/// Convert an explorer action into the corresponding explorer message.
fn explorer_action_to_msg(action: crate::features::explorer::effect::ExplorerAction) -> crate::features::explorer::msg::ExplorerMessage {
    use crate::features::explorer::effect::ExplorerAction as EA;
    use crate::features::explorer::instances::effect::InstancesAction as IA;
    use crate::features::explorer::instances::msg::{InstancesMessage, InstancesMsg};
    use crate::features::explorer::msg::ExplorerMessage as EM;
    match action {
        EA::Instances(action) => match action {
            IA::InstancesLoaded { instances } => EM::Instances(InstancesMsg::Message(
                InstancesMessage::Loaded { instances },
            )),
            IA::ConnectionsLoaded { instance_idx, connections } => {
                EM::Instances(InstancesMsg::Message(InstancesMessage::ConnectionsLoaded {
                    instance_idx,
                    connections,
                }))
            }
            IA::LoadError { error } => {
                tracing::warn!("explorer load failed: {error}");
                EM::Instances(InstancesMsg::Message(InstancesMessage::Load))
            }
        },
        EA::Objects(action) => EM::Objects(objects_action_to_msg(action)),
    }
}

/// Convert an objects action into the corresponding objects message.
fn objects_action_to_msg(
    action: crate::features::explorer::objects::effect::ObjectsAction,
) -> crate::features::explorer::objects::msg::ObjectsMsg {
    use crate::features::explorer::objects::effect::ObjectsAction as OA;
    use crate::features::explorer::objects::msg::ObjectsMessage as M;
    let msg = match action {
        OA::DatabasesLoaded { databases } => M::DatabasesLoaded { databases },
        OA::DatabasesError { error } => {
            tracing::warn!("objects database load failed: {error}");
            M::DatabasesError { error }
        }
        OA::SchemasLoaded { database, schemas } => M::SchemasLoaded { database, schemas },
        OA::SchemasError { database, error } => {
            tracing::warn!("objects schemas load failed for {database}: {error}");
            M::SchemasError { database, error }
        }
        OA::ExtensionsLoaded { database, extensions } => {
            M::ExtensionsLoaded { database, extensions }
        }
        OA::ExtensionsError { database, error } => {
            tracing::warn!("objects extensions load failed for {database}: {error}");
            M::ExtensionsError { database, error }
        }
        OA::ObjectListLoaded { database, schema, kind, items } => {
            M::ObjectListLoaded { database, schema, kind, items }
        }
        OA::ObjectListError { database, schema, kind, error } => {
            tracing::warn!("objects {kind:?} load failed for {database}.{schema}: {error}");
            M::ObjectListError { database, schema, kind, error }
        }
    };
    crate::features::explorer::objects::msg::ObjectsMsg::Message(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sql_workspace::effect::SqlAction;
    use crate::features::sql_workspace::msg::SqlMessage;
    use crate::features::sql_workspace::sql_tab::effect::SqlTabAction;
    use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
    use crate::features::sql_workspace::sql_tab::results::effect::ResultsAction;
    use crate::features::sql_workspace::sql_tab::results::msg::{ResultsMessage, ResultsMsg};
    use crate::features::sql_workspace::sql_tab::results::state::QueryResultData;

    #[test]
    fn sql_results_result_ready_routes_to_set_result() {
        let action = SqlAction::SqlTab(SqlTabAction::Results {
            tab_id: 3,
            action: ResultsAction::ResultReady {
                result: QueryResultData {
                    columns: vec![],
                    rows: vec![vec!["1".into()]],
                    rows_affected: None,
                    total_rows: Some(1),
                },
                paginated: true,
            },
        });
        let SqlMessage::SqlTab(SqlTabMsg::Message(SqlTabMessage::Results { tab_id, msg })) =
            sql_action_to_msg(action)
        else {
            panic!("expected Results route");
        };
        assert_eq!(tab_id, 3);
        let ResultsMsg::Message(ResultsMessage::SetResult { result, paginated }) = msg else {
            panic!("expected SetResult");
        };
        assert!(paginated);
        assert_eq!(result.rows[0][0], "1");
    }

    #[test]
    fn sql_results_query_error_clears_result() {
        let action = SqlAction::SqlTab(SqlTabAction::Results {
            tab_id: 1,
            action: ResultsAction::QueryError {
                message: "boom".into(),
            },
        });
        let SqlMessage::SqlTab(SqlTabMsg::Message(SqlTabMessage::Results { tab_id, msg })) =
            sql_action_to_msg(action)
        else {
            panic!("expected Results route");
        };
        assert_eq!(tab_id, 1);
        assert!(matches!(msg, ResultsMsg::Message(ResultsMessage::ClearResult)));
    }
}
