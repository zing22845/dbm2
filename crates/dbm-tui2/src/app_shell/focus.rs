//! Focus zones. The shell tracks which top-level region currently owns
//! keyboard input; features read this to decide whether to handle keys.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusZone {
    /// Top status/header bar.
    #[default]
    Header,
    /// Left-side explorer / object tree.
    Explorer,
    /// Main workspace area (tabs, editors, results).
    SQLWorkspace,
    /// Instance / connection management pane.
    InstanceWorkspace,
}
