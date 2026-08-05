//! Header feature intents.

use crate::app_shell::intent::Intent;
use super::msg::{HeaderMessage, HeaderMsg};

/// Intents emitted by the header feature.
#[derive(Debug, Clone)]
pub enum HeaderIntent {
    /// The user activated the header button at `index`. The shell is expected
    /// to open the corresponding feature (currently only `Discover`).
    Activate { index: usize },
}

impl Intent for HeaderIntent {
    type Message = HeaderMsg;

    fn into_message(self) -> Self::Message {
        match self {
            // Activating a header button has no header-local message; it is a
            // one-way notification to the shell, so routing it back to the
            // header would be a no-op. Keep the conversion total by mapping to
            // the (unused) envelope; real messages replace this arm once button
            // activation needs to feed back.
            HeaderIntent::Activate { .. } => HeaderMsg::Message(HeaderMessage::Activate),
        }
    }
}
