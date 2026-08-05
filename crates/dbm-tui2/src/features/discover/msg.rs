//! Discover feature messages.

use super::engine::msg::EngineMsg;
use super::results::msg::ResultsMsg;
use super::targets::msg::TargetsMsg;

/// The actual discover messages: forwarded to the three child sub-modules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoverMessage {
    /// Forwarded engine selector message.
    Engine(EngineMsg),
    /// Forwarded targets editor message.
    Targets(TargetsMsg),
    /// Forwarded results list message.
    Results(ResultsMsg),
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoverMsg {
    Message(DiscoverMessage),
}

impl From<DiscoverMessage> for DiscoverMsg {
    fn from(m: DiscoverMessage) -> Self {
        DiscoverMsg::Message(m)
    }
}

impl From<EngineMsg> for DiscoverMsg {
    fn from(m: EngineMsg) -> Self {
        DiscoverMsg::Message(DiscoverMessage::Engine(m))
    }
}
impl From<TargetsMsg> for DiscoverMsg {
    fn from(m: TargetsMsg) -> Self {
        DiscoverMsg::Message(DiscoverMessage::Targets(m))
    }
}
impl From<ResultsMsg> for DiscoverMsg {
    fn from(m: ResultsMsg) -> Self {
        DiscoverMsg::Message(DiscoverMessage::Results(m))
    }
}
