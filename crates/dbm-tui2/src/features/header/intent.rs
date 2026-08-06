//! Header feature intents.

use crate::app_shell::intent::Intent;
use super::msg::HeaderMsg;

/// Intents emitted by the header feature.
#[derive(Debug, Clone)]
pub enum HeaderIntent {
    /// The user activated the header button at `index`. The shell is expected
    /// to open the corresponding feature (currently only `Discover`).
    Activate { index: usize },
}

impl Intent for HeaderIntent {
    type Message = HeaderMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {
            // Activating a header button is a one-way notification to the shell
            // (it opens the feature); it has no header-local message.
            HeaderIntent::Activate { .. } => None,
        }
    }
}
