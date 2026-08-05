//! Editor feature effects and actions.

use crate::app::action::Action;
use crate::app_shell::effect::Effect;
use super::context_picker::effect::ContextPickerEffect;
use super::sql_completion::effect::SqlCompletionEffect;

#[derive(Debug, Clone)]
pub enum EditorAction {}

impl From<EditorAction> for Action {
    fn from(_a: EditorAction) -> Self {
        match _a {}
    }
}

#[derive(Debug, Clone)]
pub enum EditorEffect {
    ContextPicker(ContextPickerEffect),
    SqlCompletion(SqlCompletionEffect),
}

impl Effect for EditorEffect {
    type Action = EditorAction;

    fn run(self, _emit: crate::app_shell::effect::effect_trait::Emitter<Self::Action>, _services: std::sync::Arc<crate::common::service::services::Services>) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
            EditorEffect::ContextPicker(_) | EditorEffect::SqlCompletion(_) => Vec::new(),
    }
    })
    }
}
