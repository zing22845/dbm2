//! Discover feature messages.

use super::engine::msg::EngineMsg;
use super::results::msg::ResultsMsg;
use super::targets::msg::TargetsMsg;

/// The actual discover messages: pane navigation plus forwarding to the three
/// child sub-modules.
#[derive(Debug, Clone)]
pub enum DiscoverMessage {
    /// Set the active discover child pane.
    Focus(crate::app_shell::nav::DiscoverPane),
    /// Show the close-confirmation dialog.
    RequestClose,
    /// Hide the close-confirmation dialog without closing.
    CancelClose,
    /// Close the discover modal (after confirmation).
    Close,
    /// Set the targets editor height in rows (drag the targets/results
    /// splitter).
    SetTargetsHeight { height: u16 },
    /// Nudge the targets height by one step with `+` / `-` (`plus` is true for
    /// `+`), growing the currently-focused pane (`top_focused` = the targets
    /// editor is focused rather than the results list).
    NudgeTargetsHeight { plus: bool, top_focused: bool },
    /// Start a discovery scan over the current targets.
    StartScan,
    /// Ask the in-flight scan to stop at the next host boundary (`c` key).
    CancelScan,
    /// Register the currently selected discovered instances. `force` bypasses
    /// precheck warnings (used by the force-register key `R`); errors still block.
    RegisterSelected { force: bool },
    /// Scan progress update (streamed from the scan effect).
    ScanProgress { done: u32, total: u32 },
    /// The scan completed with the discovered instances.
    ScanComplete {
        items: Vec<dbm_discovery::DiscoveredInstance>,
    },
    /// The scan was cancelled before completing (no new results persisted).
    ScanCancelled,
    /// The scan failed.
    ScanError { error: String },
    /// Instances were registered. Carries the refreshed discovered list so the
    /// results pane can drop (or re-mark) the now-registered rows.
    RegisterComplete {
        items: Vec<dbm_discovery::DiscoveredInstance>,
    },
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
