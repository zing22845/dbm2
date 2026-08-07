//! Pane hierarchy for keyboard focus.
//!
//! The shell tracks which pane currently owns keyboard input as a single value:
//! a **parent pane** (the focused top-level region) that may host **child
//! panes**. This replaces the previous flat `FocusZone` plus the separate
//! discover-modal focus: `Discover` is itself a parent pane whose child sub
//! panes (engine / targets / results) are the focused region while the discover
//! modal is open.
//!
//! Keeping focus as one value lets the shell route keys, gate feature updates,
//! and render active borders from a single source of truth.

/// A parent pane in the focus hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    /// Top status/header bar.
    #[default]
    Header,
    /// Left-side explorer / object tree. Like `Discover`, it is a parent pane
    /// whose child sub-pane (instances / objects) is the focused region.
    Explorer(crate::app_shell::nav::ExplorerPane),
    /// Main workspace area (SQL tabs, editor, history, results).
    Workspace,
    /// Instance / connection management pane.
    InstanceWorkspace,
    /// The discover modal, open as a parent pane; its child sub-pane is focused.
    Discover(DiscoverPane),
}

/// Child panes under the `Discover` parent pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiscoverPane {
    /// The engine selector.
    #[default]
    Engine,
    /// The targets (host : ports) editor.
    Targets,
    /// The results list.
    Results,
}

impl Pane {
    /// Whether this pane is a modal overlay (owns all keyboard input).
    pub fn is_modal(self) -> bool {
        matches!(self, Pane::Discover(_))
    }

    /// The discover child pane, if this is the discover parent pane.
    pub fn as_discover(self) -> Option<DiscoverPane> {
        match self {
            Pane::Discover(sub) => Some(sub),
            _ => None,
        }
    }
}

impl DiscoverPane {
    /// Move focus to the previous discover sub-pane (wrapping).
    pub fn prev(self) -> Self {
        match self {
            DiscoverPane::Engine => DiscoverPane::Results,
            DiscoverPane::Targets => DiscoverPane::Engine,
            DiscoverPane::Results => DiscoverPane::Targets,
        }
    }

    /// Move focus to the next discover sub-pane (wrapping).
    pub fn next(self) -> Self {
        match self {
            DiscoverPane::Engine => DiscoverPane::Targets,
            DiscoverPane::Targets => DiscoverPane::Results,
            DiscoverPane::Results => DiscoverPane::Engine,
        }
    }
}

/// A persistence-friendly name for a parent pane (for session snapshots).
pub fn pane_name(pane: Pane) -> &'static str {
    match pane {
        Pane::Header => "header",
        Pane::Explorer(_) => "explorer",
        Pane::Workspace | Pane::InstanceWorkspace => "workspace",
        Pane::Discover(_) => "workspace",
    }
}

/// Resolve a parent pane from a persistence-friendly name (session snapshots).
/// Accepts the legacy lowercase names plus a few historical aliases.
pub fn pane_from_name(name: &str) -> Option<Pane> {
    match name {
        "header" | "Header" => Some(Pane::Header),
        "explorer" | "tree" | "Explorer" => Some(Pane::Explorer(
            crate::app_shell::nav::ExplorerPane::default(),
        )),
        "workspace" | "SQLWorkspace" | "InstanceWorkspace" => Some(Pane::Workspace),
        _ => None,
    }
}
