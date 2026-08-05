//! Shell-level messages. These are produced by the framework (terminal
//! events, ticks, focus changes) and are routed through the central
//! `AppMsg` router under the `Shell` variant.

use super::focus::FocusZone;

/// Messages owned by the shell (not by any feature).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellMsg {
    /// User requested to quit the application.
    Quit,
    /// Periodic redraw tick produced by the internal timer.
    Tick,
    /// The active focus zone changed.
    FocusChanged { zone: FocusZone },
}
