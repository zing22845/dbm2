//! Shell-level effects. The shell currently has no async side effects of its
//! own, so this is an empty placeholder. Future real effects (e.g. persisting
//! configuration) can be added as new variants without touching the app layer.

use super::effect_trait::{BoxFuture, Effect};
use crate::app_shell::action::ShellAction;

/// Effects owned by the shell. Empty enum: no side effects exist yet.
#[derive(Debug, Clone)]
pub enum ShellEffect {}

impl Effect for ShellEffect {
    type Action = ShellAction;

    fn run(self) -> BoxFuture<Vec<Self::Action>> {
        // The enum is uninhabited, so this can never be called; the trait
        // still requires an implementation, so return an empty list.
        Box::pin(async { vec![] })
    }
}
