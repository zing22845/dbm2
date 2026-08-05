//! Explorer objects feature effects and actions.

use std::sync::Arc;

use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;

/// Actions produced by objects effects.
#[derive(Debug, Clone)]
pub enum ObjectsAction {
    /// The catalog rows for the bound connection were loaded.
    RowsLoaded { rows: Vec<super::state::ObjectsRow> },
    /// Loading failed.
    Error { error: String },
}

/// Effects emitted by the objects tree.
///
/// Catalog fetching requires a live database connection (dbm-driver-pg) and is
/// deferred until a real connection is available; the enum is defined now so
/// the state/update/rendering pipeline is complete.
#[derive(Debug, Clone)]
pub enum ObjectsEffect {}

impl Effect for ObjectsEffect {
    type Action = ObjectsAction;

    fn run(self, _emit: Emitter<Self::Action>, _services: Arc<Services>) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {}
        })
    }
}
