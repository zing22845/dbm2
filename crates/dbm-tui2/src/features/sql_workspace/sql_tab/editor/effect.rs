//! Editor feature effects and actions.

use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;
use super::context_picker::effect::{ContextPickerAction, ContextPickerEffect};
use super::sql_completion::effect::SqlCompletionEffect;

/// Actions produced by editor effects (from its child sub-modules).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorAction {
    /// An action from the context picker sub-module.
    ContextPicker(ContextPickerAction),
}

impl From<ContextPickerAction> for EditorAction {
    fn from(a: ContextPickerAction) -> Self {
        EditorAction::ContextPicker(a)
    }
}

impl From<super::sql_completion::effect::SqlCompletionAction> for EditorAction {
    fn from(_a: super::sql_completion::effect::SqlCompletionAction) -> Self {
        match _a {}
    }
}

/// Effects emitted by the editor feature, delegating to its child sub-modules.
#[derive(Debug, Clone)]
pub enum EditorEffect {
    /// An effect from the context picker sub-module.
    ContextPicker(ContextPickerEffect),
    /// An effect from the sql completion sub-module.
    SqlCompletion(SqlCompletionEffect),
}

impl Effect for EditorEffect {
    type Action = EditorAction;

    fn run(self, emit: Emitter<Self::Action>, services: std::sync::Arc<Services>) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                EditorEffect::ContextPicker(e) => {
                    let emit = emit.map::<ContextPickerAction>();
                    e.run(emit, services)
                        .await
                        .into_iter()
                        .map(Into::into)
                        .collect()
                }
                EditorEffect::SqlCompletion(e) => {
                    let emit = emit.map::<super::sql_completion::effect::SqlCompletionAction>();
                    e.run(emit, services)
                        .await
                        .into_iter()
                        .map(Into::into)
                        .collect()
                }
            }
        })
    }
}
