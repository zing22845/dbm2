//! History feature effects and actions.

use crate::app::action::Action;
use crate::app_shell::effect::Effect;
use crate::app_shell::effect::effect_trait::{BoxFuture, Emitter};

/// Actions produced by history effects. Routed back to the owning tab by the
/// loop's `sql_action_to_msg`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryAction {
    /// The persisted history for `(instance, connection)` was loaded (newest
    /// first); `entries` are the raw SQL texts.
    HistoryLoaded {
        instance: String,
        connection: String,
        entries: Vec<String>,
    },
}

impl From<HistoryAction> for Action {
    fn from(a: HistoryAction) -> Self {
        // The real tab_id is restored by `sql_action_to_msg` (the loop), like
        // `ResultsAction`. tab 0 here is only a compile-time placeholder for
        // streamed emission.
        use crate::features::sql_workspace::effect::SqlAction;
        use crate::features::sql_workspace::sql_tab::effect::SqlTabAction;
        Action::Sql(SqlAction::SqlTab(SqlTabAction::History { tab_id: 0, action: a }))
    }
}

#[derive(Debug, Clone)]
pub enum HistoryEffect {
    /// Persist a successfully executed statement to the SQLite history store
    /// (mirrors the original dbm's `record_sql_history`). Fire-and-forget: the
    /// in-memory `SqlHistoryStore` was already updated by the update.
    PersistSuccess {
        instance: String,
        connection: String,
        sql: String,
    },
    /// Load the persisted history for a connection into the owning tab's
    /// in-memory store (issued when a tab is created, so a freshly opened
    /// connection sees its previously saved history).
    LoadHistory {
        instance: String,
        connection: String,
    },
}

impl Effect for HistoryEffect {
    type Action = HistoryAction;

    fn run(
        self,
        emit: Emitter<Self::Action>,
        services: std::sync::Arc<crate::common::service::services::Services>,
    ) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                HistoryEffect::PersistSuccess {
                    instance,
                    connection,
                    sql,
                } => {
                    // SQLite is blocking I/O; run it off the async executor.
                    let store = services.store.clone();
                    tokio::task::spawn_blocking(move || {
                        if let Ok(store) = store.lock() {
                            let _ = store.record_sql_history(&instance, &connection, &sql);
                        }
                    })
                    .await
                    .ok();
                    Vec::new()
                }
                HistoryEffect::LoadHistory {
                    instance,
                    connection,
                } => {
                    let store = services.store.clone();
                    let loaded = tokio::task::spawn_blocking(move || {
                        let store = store.lock().ok()?;
                        store.load_sql_history().ok()
                    })
                    .await
                    .ok()
                    .flatten();
                    let entries = loaded
                        .as_ref()
                        .and_then(|map| map.get(&(instance.clone(), connection.clone())))
                        .cloned()
                        .unwrap_or_default();
                    emit.emit(HistoryAction::HistoryLoaded {
                        instance,
                        connection,
                        entries,
                    });
                    Vec::new()
                }
            }
        })
    }
}
