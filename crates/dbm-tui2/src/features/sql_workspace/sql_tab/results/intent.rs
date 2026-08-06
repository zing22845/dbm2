//! Results feature intents.

use crate::app_shell::intent::Intent;
use super::msg::ResultsMsg;
use super::detail::intent::DetailIntent;

#[derive(Debug, Clone)]
pub enum ResultsIntent {
    Detail(DetailIntent),
}

impl Intent for ResultsIntent {
    type Message = ResultsMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {
            ResultsIntent::Detail(i) => i.into_message().map(Into::into),
        }
    }
}

