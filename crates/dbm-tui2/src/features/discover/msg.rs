//! Discover feature messages.

use super::engine::msg::EngineMsg;
use super::results::msg::ResultsMsg;
use super::targets::msg::TargetsMsg;
use super::state::DiscoverFocus;

/// The actual discover messages: pane navigation plus forwarding to the three
/// child sub-modules.
#[derive(Debug, Clone)]
pub enum DiscoverMessage {
    /// Set the active discover pane.
    Focus(DiscoverFocus),
    /// Show the close-confirmation dialog.
    RequestClose,
    /// Hide the close-confirmation dialog without closing.
    CancelClose,
    /// Close the discover modal (after confirmation).
    Close,
    /// Start a discovery scan over the current targets.
    StartScan,
    /// Register the currently selected discovered instances.
    RegisterSelected,
    /// Scan progress update (streamed from the scan effect).
    ScanProgress { done: u32, total: u32 },
    /// The scan completed with the discovered instances.
    ScanComplete { items: Vec<dbm_discovery::DiscoveredInstance> },
    /// The scan failed.
    ScanError { error: String },
    /// Instances were registered.
    RegisterComplete { count: usize },
    /// Registering failed.
    RegisterError { error: String },
    /// Forwarded engine selector message.
    Engine(EngineMsg),
    /// Forwarded targets editor message.
    Targets(TargetsMsg),
    /// Forwarded results list message.
    Results(ResultsMsg),
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone)]
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
