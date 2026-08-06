//! Shell-level intents. The shell currently has no feature-to-feature
//! requests of its own, so this is a placeholder for completeness.

use super::intent_trait::Intent;
use crate::app_shell::msg::ShellMsg;

/// Intents owned by the shell.
#[derive(Debug, Clone)]
pub enum ShellIntent {
    /// Request the shell to quit.
    Quit,
}

impl Intent for ShellIntent {
    type Message = ShellMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {
            ShellIntent::Quit => Some(ShellMsg::Quit),
        }
    }
}
