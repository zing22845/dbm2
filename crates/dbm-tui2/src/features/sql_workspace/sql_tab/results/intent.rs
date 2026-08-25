//! Results feature intents.

use crate::app_shell::intent::Intent;
use super::msg::ResultsMsg;
use super::detail::intent::DetailIntent;
use super::list::intent::ListIntent;

#[derive(Debug, Clone)]
pub enum ResultsIntent {
    List(ListIntent),
    Detail(DetailIntent),
}

impl Intent for ResultsIntent {
    type Message = ResultsMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {
            ResultsIntent::List(i) => i.into_message().map(Into::into),
            ResultsIntent::Detail(i) => i.into_message().map(Into::into),
        }
    }
}