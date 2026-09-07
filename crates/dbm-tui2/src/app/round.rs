//! Message and action rounds: apply input and drain the resulting cascade.
//!
//! An update pass may produce intents and effects; intents resolve to further
//! messages, so a single input fans out into a cascade. It is drained
//! iteratively (never recursively) and bounded by
//! [`MAX_MESSAGES_PER_ROUND`] so a logic bug cannot turn into an unbounded hot
//! loop.
//!
//! This sits *below* `loop_mod`: it knows how to run a round, but nothing about
//! the terminal, the select loop or session persistence.

use std::collections::VecDeque;

use tokio::sync::mpsc;

use crate::app::action::Action;
use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::app::update::{UpdateResult, update_unchecked};
use crate::app_shell::effect::EffectRunner;
use crate::app_shell::intent::IntentRouter;

/// Upper bound on how many messages are processed in a single event round
/// (the triggering event + the cascade it starts). Guards against livelock
/// caused by a cyclic intent cascade; it does not otherwise change behavior.
const MAX_MESSAGES_PER_ROUND: usize = 100;

/// A round triggered by an external message (keyboard/tick). The seed message
/// and any already-available async actions are queued and drained iteratively.
/// Returns the aggregated [`UpdateResult`] so the caller can decide whether the
/// round actually changed rendering state (`dirty`).
pub(crate) fn process_message_round(
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    seed: AppMsg,
    state: &mut AppState,
) -> UpdateResult {
    let mut pending: VecDeque<AppMsg> = VecDeque::new();
    pending.push_back(seed);
    drain_async_actions(action_rx, &mut pending);
    drain_pending(effect_runner, &mut pending, state)
}

/// A round triggered by an async action (an effect result). The action is
/// applied via `handle_action`; its intents/effects are consumed by the same
/// iterative drain as a message round.
pub(crate) fn process_action_round(
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    seed: Action,
    state: &mut AppState,
) -> UpdateResult {
    let mut pending: VecDeque<AppMsg> = VecDeque::new();
    let result = handle_action(seed, state);
    // The seed's own dirty flag must survive into the returned result, or a
    // completion/error that only mutates state (no queued cascade) would be
    // applied without ever scheduling a repaint — leaving a stale frame (e.g.
    // a "scanning… N/N" footer) on screen after the scan already finished.
    let mut aggregated = UpdateResult::new();
    aggregated.dirty |= result.dirty;
    queue_result(effect_runner, result, &mut pending);
    drain_async_actions(action_rx, &mut pending);
    aggregated.dirty |= drain_pending(effect_runner, &mut pending, state).dirty;
    aggregated
}

/// Iteratively apply every queued message until the queue is empty or the
/// per-round budget is exhausted. Intents produced by an update resolve to new
/// `AppMsg`s that are pushed back onto the queue, replacing recursion with a
/// heap-backed work list.
///
/// Returns an aggregated [`UpdateResult`] whose `dirty` is the OR of every
/// applied update: if *any* message in the round changed rendering state, the
/// round as a whole is considered dirty so the caller repaints once.
fn drain_pending(
    effect_runner: &EffectRunner<Action>,
    pending: &mut VecDeque<AppMsg>,
    state: &mut AppState,
) -> UpdateResult {
    let mut processed = 0usize;
    let mut aggregated = UpdateResult::new();
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

        // Use `update_unchecked`: keyboard focus routing is done in the input
        // layer (`key_to_msg` only produces messages for the active pane), so
        // the focus guard is redundant for keyboard input and actively harmful
        // for programmatic messages — an intent or pending cascade may target a
        // *non-focused* pane (e.g. updating the explorer tree while the focus
        // sits on the workspace). Such cross-pane updates must not be dropped.
        let result = update_unchecked(msg, state);
        aggregated.dirty |= result.dirty;
        queue_result(effect_runner, result, pending);
    }
    aggregated
}

/// Submit an update result's effects to the runner and push each routed intent
/// back onto the pending queue. Intents are programmatic cross-feature
/// requests; the messages they resolve to are applied without the focus guard
/// because delivery is not keyboard input.
pub(crate) fn queue_result(
    effect_runner: &EffectRunner<Action>,
    result: UpdateResult,
    pending: &mut VecDeque<AppMsg>,
) {
    for effect in result.effects {
        effect_runner.submit(effect);
    }
    for intent in result.intents {
        // Cross-feature intents (routed by a parent) produce no message and are
        // skipped here rather than panicking.
        if let Some(nested) = IntentRouter::route::<AppMsg>(intent) {
            pending.push_back(nested);
        }
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
            use crate::features::sql_workspace::sql_tab::editor::effect::EditorAction;
            use crate::features::sql_workspace::sql_tab::effect::SqlTabAction;
            use crate::features::sql_workspace::sql_tab::results::effect::ResultsAction;
            let mut msgs = Vec::new();
            // A clipboard copy produces no feature message: its outcome is
            // surfaced as a global-footer status only.
            let mut copy_column_name = false;
            let mut editor_copy_selection = false;
            let mut commit_handled = false;
            match &action {
                // A commit finished: close the preview modal, surface the
                // outcome in the global footer, and on success drive the
                // post-commit flow inside the results feature (exit edit + rerun
                // via `CommitOutcome`). A failed commit keeps the edit session.
                SqlAction::SqlTab(SqlTabAction::Results {
                    tab_id,
                    action: ResultsAction::CommitResult { ok, message },
                }) => {
                    msgs.push(AppMsg::CloseModal);
                    msgs.push(AppMsg::Footer(
                        crate::features::global_footer::msg::FooterMsg::Message(
                            crate::features::global_footer::msg::FooterMessage::SetStatus(if *ok {
                                message.clone()
                            } else {
                                format!("Commit failed: {message}")
                            }),
                        ),
                    ));
                    if *ok {
                        msgs.push(AppMsg::Sql(
                            crate::features::sql_workspace::msg::SqlMsg::Message(
                                crate::features::sql_workspace::msg::SqlMessage::SqlTab(
                                    crate::features::sql_workspace::sql_tab::msg::SqlTabMsg::Message(
                                        crate::features::sql_workspace::sql_tab::msg::SqlTabMessage::Results {
                                            tab_id: *tab_id,
                                            msg: crate::features::sql_workspace::sql_tab::results::msg::ResultsMsg::Message(
                                                crate::features::sql_workspace::sql_tab::results::msg::ResultsMessage::CommitOutcome { ok: true },
                                            ),
                                        },
                                    ),
                                ),
                            ),
                        ));
                    }
                    commit_handled = true;
                }
                // A failed query surfaces its message in the results pane (via
                // the routed `QueryError` message) and in the global footer
                // status line, mirroring the original dbm's `set_status`.
                SqlAction::SqlTab(SqlTabAction::Results {
                    action: ResultsAction::QueryError { message },
                    ..
                }) => {
                    msgs.push(AppMsg::Footer(
                        crate::features::global_footer::msg::FooterMsg::Message(
                            crate::features::global_footer::msg::FooterMessage::SetStatus(format!(
                                "Query failed: {message}"
                            )),
                        ),
                    ));
                }
                SqlAction::SqlTab(SqlTabAction::Results {
                    action: ResultsAction::CopyColumnName { ok },
                    ..
                }) => {
                    msgs.push(AppMsg::Footer(
                        crate::features::global_footer::msg::FooterMsg::Message(
                            crate::features::global_footer::msg::FooterMessage::SetStatus(if *ok {
                                "Copied to clipboard".to_string()
                            } else {
                                "Copy failed".to_string()
                            }),
                        ),
                    ));
                    copy_column_name = true;
                }
                // An editor selection copy is surfaced exactly like the
                // column-name copy: a global-footer status, no feature message.
                SqlAction::SqlTab(SqlTabAction::Editor {
                    action: EditorAction::CopySelection { ok },
                    ..
                }) => {
                    msgs.push(AppMsg::Footer(
                        crate::features::global_footer::msg::FooterMsg::Message(
                            crate::features::global_footer::msg::FooterMessage::SetStatus(if *ok {
                                "Copied to clipboard".to_string()
                            } else {
                                "Copy failed".to_string()
                            }),
                        ),
                    ));
                    editor_copy_selection = true;
                }
                _ => {}
            }
            if !copy_column_name && !editor_copy_selection && !commit_handled {
                msgs.push(AppMsg::Sql(
                    crate::features::sql_workspace::msg::SqlMsg::Message(sql_action_to_msg(action)),
                ));
            }
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
fn discover_action_to_msg(
    action: crate::features::discover::effect::DiscoverAction,
) -> crate::features::discover::msg::DiscoverMessage {
    use crate::features::discover::effect::DiscoverAction as A;
    use crate::features::discover::msg::DiscoverMessage as M;
    match action {
        A::ScanProgress { done, total } => M::ScanProgress { done, total },
        A::ScanComplete { items } => M::ScanComplete { items },
        A::ScanCancelled => M::ScanCancelled,
        A::ScanError { error } => M::ScanError { error },
        A::RegisterComplete { items } => M::RegisterComplete { items },
        A::RegisterError { error } => M::RegisterError { error },
    }
}

/// Convert an instance workspace action into the corresponding iw message.
fn iw_action_to_msg(
    action: crate::features::instance_workspace::effect::IwAction,
) -> crate::features::instance_workspace::msg::IwMessage {
    use crate::features::instance_workspace::connections::effect::ConnectionsAction as CA;
    use crate::features::instance_workspace::connections::msg::{
        ConnectionsMessage, ConnectionsMsg,
    };
    use crate::features::instance_workspace::effect::IwAction as IA;
    use crate::features::instance_workspace::msg::IwMessage as IM;
    use crate::features::instance_workspace::overview::effect::OverviewAction as OA;
    use crate::features::instance_workspace::overview::msg::{OverviewMessage, OverviewMsg};
    match action {
        IA::Overview(action) => match action {
            OA::Loaded { instance } => {
                IM::Overview(OverviewMsg::Message(OverviewMessage::Loaded { instance }))
            }
            OA::Error { error } => {
                tracing::warn!("iw overview load failed: {error}");
                IM::Overview(OverviewMsg::Message(OverviewMessage::Reload))
            }
        },
        IA::Connections(action) => match action {
            CA::Loaded { connections } => {
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::Loaded {
                    connections,
                }))
            }
            CA::Saved => {
                // The form is still open until the save succeeds; `Saved` closes
                // it and reloads the list (a failed save keeps the form via
                // `SaveError`, so it is not silently dropped).
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::Saved))
            }
            CA::Deleted => {
                // Reload the connections after a delete (no form involved).
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::Reload))
            }
            CA::Error { error } => {
                tracing::warn!("iw connections op failed: {error}");
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::SaveError(
                    error,
                )))
            }
            CA::TestResult { ok, error } => {
                // Match the original dbm's wording and color: "Test OK" in green
                // on success, "Test failed: <reason>" in red on failure, with a
                // timestamp prefix to limit waste on repeat tests.
                let ts = crate::common::utils::time::utc_timestamp();
                let (status, kind) = if ok {
                    (
                        format!("{ts} Test OK"),
                        crate::features::instance_workspace::connections::state::ConnectionStatusKind::Success,
                    )
                } else {
                    (
                        format!(
                            "{ts} Test failed: {}",
                            error.unwrap_or_else(|| "could not connect".to_string())
                        ),
                        crate::features::instance_workspace::connections::state::ConnectionStatusKind::Failure,
                    )
                };
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::SetStatus {
                    status,
                    kind,
                }))
            }
            CA::ListTestResult { ok, error } => {
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::TestComplete {
                    ok,
                    error,
                }))
            }
        },
        IA::Unregistered { instance } => IM::Unregistered { instance },
    }
}

/// Convert a SQL workspace action into the corresponding workspace message,
/// routing editor actions back to the originating tab's editor (the context
/// picker's catalog results).
fn sql_action_to_msg(
    action: crate::features::sql_workspace::effect::SqlAction,
) -> crate::features::sql_workspace::msg::SqlMessage {
    use crate::features::sql_workspace::effect::SqlAction as SA;
    use crate::features::sql_workspace::msg::SqlMessage;
    use crate::features::sql_workspace::sql_tab::editor::context_picker::msg::ContextPickerMsg;
    use crate::features::sql_workspace::sql_tab::editor::effect::EditorAction as EA;
    use crate::features::sql_workspace::sql_tab::editor::msg::{EditorMessage, EditorMsg};
    use crate::features::sql_workspace::sql_tab::effect::SqlTabAction as STA;
    use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
    match action {
        SA::SqlTab(STA::Editor { tab_id, action }) => {
            let msg = match action {
                EA::ContextPicker(cp) => EditorMsg::Message(EditorMessage::ContextPicker(
                    ContextPickerMsg::Message(cp_action_to_msg(cp)),
                )),
                EA::CompletionCatalogLoaded(data) => {
                    EditorMsg::Message(EditorMessage::CatalogLoaded {
                        tables: data.tables,
                        columns_by_table: data.columns_by_table,
                    })
                }
                // The selection-copy outcome is reported by the round footer
                // status; it never reaches this feature-message path.
                EA::CopySelection { .. } => {
                    unreachable!("editor selection copy is reported by the round footer status")
                }
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
        SA::SqlTab(STA::History { tab_id, action }) => {
            use crate::features::sql_workspace::sql_tab::history::effect::HistoryAction as HA;
            use crate::features::sql_workspace::sql_tab::history::store::SqlHistoryStore;
            match action {
                HA::HistoryLoaded {
                    instance,
                    connection,
                    entries,
                } => {
                    let mut store = SqlHistoryStore::default();
                    if !entries.is_empty() {
                        let mut map = std::collections::HashMap::new();
                        map.insert((instance, connection), entries);
                        store = SqlHistoryStore::from_map(map);
                    }
                    SqlMessage::SqlTab(SqlTabMsg::Message(SqlTabMessage::SetHistoryStore {
                        tab_id,
                        store,
                    }))
                }
            }
        }
    }
}

/// Convert a results action into the corresponding results message.
fn results_action_to_msg(
    action: crate::features::sql_workspace::sql_tab::results::effect::ResultsAction,
) -> crate::features::sql_workspace::sql_tab::results::msg::ResultsMessage {
    use crate::features::sql_workspace::sql_tab::results::effect::ResultsAction as RA;
    use crate::features::sql_workspace::sql_tab::results::msg::ResultsMessage as M;
    match action {
        RA::ResultReady { result, paginated } => M::SetResult { result, paginated },
        RA::QueryError { message } => {
            tracing::warn!("query failed: {message}");
            // Mirror the original dbm: clear the result and surface the error
            // text in the results pane (stored as `query_error`).
            M::QueryError { message }
        }
        RA::CountReady { sql, total } => M::CountReady { sql, total },
        // The copy outcome is surfaced as a footer status by the round (see
        // `action_to_app_msgs`); it never reaches this feature-message path.
        RA::CopyColumnName { .. } => {
            unreachable!("column-name copy is reported by the round footer status")
        }
        // The commit outcome is handled entirely by the round (footer status +
        // post-commit `CommitOutcome` message); it never reaches this route.
        RA::CommitResult { ok, message } => {
            tracing::info!("commit ok={ok}: {message}");
            unreachable!("commit outcome is handled by the round footer / CommitOutcome flow")
        }
        RA::EditabilityReady { target, blocked } => M::EditabilityReady { target, blocked },
    }
}

/// Convert a context picker action into the corresponding picker message.
fn cp_action_to_msg(
    action: crate::features::sql_workspace::sql_tab::editor::context_picker::effect::ContextPickerAction,
) -> crate::features::sql_workspace::sql_tab::editor::context_picker::msg::ContextPickerMessage {
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
fn explorer_action_to_msg(
    action: crate::features::explorer::effect::ExplorerAction,
) -> crate::features::explorer::msg::ExplorerMessage {
    use crate::features::explorer::effect::ExplorerAction as EA;
    use crate::features::explorer::instances::effect::InstancesAction as IA;
    use crate::features::explorer::instances::msg::{InstancesMessage, InstancesMsg};
    use crate::features::explorer::msg::ExplorerMessage as EM;
    match action {
        EA::Instances(action) => match action {
            IA::InstancesLoaded { instances } => {
                EM::Instances(InstancesMsg::Message(InstancesMessage::Loaded {
                    instances,
                }))
            }
            IA::ConnectionsLoaded {
                instance_idx,
                connections,
            } => EM::Instances(InstancesMsg::Message(InstancesMessage::ConnectionsLoaded {
                instance_idx,
                connections,
            })),
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
        OA::ExtensionsLoaded {
            database,
            extensions,
        } => M::ExtensionsLoaded {
            database,
            extensions,
        },
        OA::ExtensionsError { database, error } => {
            tracing::warn!("objects extensions load failed for {database}: {error}");
            M::ExtensionsError { database, error }
        }
        OA::ObjectListLoaded {
            database,
            schema,
            kind,
            items,
        } => M::ObjectListLoaded {
            database,
            schema,
            kind,
            items,
        },
        OA::ObjectListError {
            database,
            schema,
            kind,
            error,
        } => {
            tracing::warn!("objects {kind:?} load failed for {database}.{schema}: {error}");
            M::ObjectListError {
                database,
                schema,
                kind,
                error,
            }
        }
    };
    crate::features::explorer::objects::msg::ObjectsMsg::Message(msg)
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
    for msg in action_to_app_msgs(action) {
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
    use crate::features::sql_workspace::effect::SqlAction;
    use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
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
    fn sql_results_query_error_maps_to_query_error_message() {
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
        assert!(matches!(
            msg,
            ResultsMsg::Message(ResultsMessage::QueryError { message }) if message == "boom"
        ));
    }

    #[test]
    fn sql_history_loaded_routes_to_set_history_store() {
        use crate::features::sql_workspace::sql_tab::history::effect::HistoryAction;
        let action = SqlAction::SqlTab(SqlTabAction::History {
            tab_id: 7,
            action: HistoryAction::HistoryLoaded {
                instance: "inst".into(),
                connection: "c1".into(),
                entries: vec!["SELECT 1".into(), "SELECT 2".into()],
            },
        });
        let SqlMessage::SqlTab(SqlTabMsg::Message(SqlTabMessage::SetHistoryStore {
            tab_id,
            store,
        })) = sql_action_to_msg(action)
        else {
            panic!("expected SetHistoryStore route");
        };
        assert_eq!(tab_id, 7);
        assert_eq!(store.entries("inst", "c1"), &["SELECT 1", "SELECT 2"]);
    }

    #[test]
    fn editor_copy_action_surfaces_footer_status_only() {
        use crate::app::action::Action;
        use crate::features::global_footer::msg::{FooterMessage, FooterMsg};
        use crate::features::sql_workspace::sql_tab::editor::effect::EditorAction;
        let action = Action::Sql(SqlAction::SqlTab(SqlTabAction::Editor {
            tab_id: 2,
            action: EditorAction::CopySelection { ok: true },
        }));
        let msgs = action_to_app_msgs(action);
        assert!(
            msgs.iter().any(|m| matches!(
                m,
                AppMsg::Footer(FooterMsg::Message(FooterMessage::SetStatus(s)))
                    if s == "Copied to clipboard"
            )),
            "copy action must surface a footer status, got: {msgs:?}"
        );
        assert!(
            !msgs.iter().any(|m| matches!(m, AppMsg::Sql(_))),
            "a clipboard copy must not feed back a feature message"
        );
    }

    #[test]
    fn commit_result_success_closes_modal_reports_status_and_posts_outcome() {
        use crate::app::action::Action;
        use crate::features::global_footer::msg::{FooterMessage, FooterMsg};
        use crate::features::sql_workspace::sql_tab::results::msg::{ResultsMessage, ResultsMsg};
        let action = Action::Sql(SqlAction::SqlTab(SqlTabAction::Results {
            tab_id: 1,
            action: ResultsAction::CommitResult {
                ok: true,
                message: "Committed 2 statement(s)".into(),
            },
        }));
        let msgs = action_to_app_msgs(action);
        assert!(msgs.iter().any(|m| matches!(m, AppMsg::CloseModal)));
        assert!(msgs.iter().any(|m| matches!(
            m,
            AppMsg::Footer(FooterMsg::Message(FooterMessage::SetStatus(s)))
                if s == "Committed 2 statement(s)"
        )));
        assert!(
            msgs.iter().any(|m| matches!(
                m,
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Results {
                        msg: ResultsMsg::Message(ResultsMessage::CommitOutcome { ok: true }),
                        ..
                    },
                ))))
            )),
            "a successful commit must drive the post-commit flow"
        );
    }

    #[test]
    fn commit_result_failure_reports_error_and_skips_post_commit() {
        use crate::app::action::Action;
        use crate::features::global_footer::msg::{FooterMessage, FooterMsg};
        use crate::features::sql_workspace::sql_tab::results::msg::{ResultsMessage, ResultsMsg};
        let action = Action::Sql(SqlAction::SqlTab(SqlTabAction::Results {
            tab_id: 1,
            action: ResultsAction::CommitResult {
                ok: false,
                message: "conflict on row 3".into(),
            },
        }));
        let msgs = action_to_app_msgs(action);
        assert!(msgs.iter().any(|m| matches!(
            m,
            AppMsg::Footer(FooterMsg::Message(FooterMessage::SetStatus(s)))
                if s == "Commit failed: conflict on row 3"
        )));
        assert!(
            !msgs.iter().any(|m| matches!(
                m,
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Results {
                        msg: ResultsMsg::Message(ResultsMessage::CommitOutcome { .. }),
                        ..
                    },
                ))))
            )),
            "a failed commit must keep the edit session intact"
        );
    }

    #[test]
    fn action_round_propagates_seed_dirty_flag() {
        // Regression: a completion action that only mutates state (no queued
        // cascade) must still mark the round dirty, otherwise the event loop
        // skips the repaint and leaves a stale "scanning…" frame on screen.
        let (action_tx, mut action_rx) = mpsc::unbounded_channel::<Action>();
        let services = std::sync::Arc::new(crate::common::service::services::Services::default());
        let (effect_runner, handle) = EffectRunner::new(action_tx, services);
        drop(handle); // no effects run in this test; the runner is only a sink

        let mut state = crate::app::state::AppState::default();
        state.discover.scanning = true;
        state.discover.scan_progress = Some((10, 11));

        let action = crate::app::action::Action::Discover(
            crate::features::discover::effect::DiscoverAction::ScanComplete { items: vec![] },
        );
        let result = process_action_round(&effect_runner, &mut action_rx, action, &mut state);

        assert!(result.dirty, "completion must mark the round dirty");
        assert!(
            !state.discover.scanning,
            "scanning must clear on completion"
        );
        assert_eq!(state.discover.scan_progress, None);
    }
}
