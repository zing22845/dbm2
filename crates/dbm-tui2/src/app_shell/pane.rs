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
    /// Main SQL workspace area (tabs, editor, history, results).
    SQLWorkspace,
    /// Instance / connection management pane. Like the explorer, it is a
    /// parent pane whose child sub-pane (overview / connections) is focused.
    InstanceWorkspace(crate::app_shell::nav::IwPane),
    /// The discover modal, open as a parent pane; its child sub-pane is focused.
    Discover(crate::app_shell::nav::DiscoverPane),
}

/// A persistence-friendly name for a parent pane (for session snapshots).
pub fn pane_name(pane: Pane) -> &'static str {
    match pane {
        Pane::Header => "header",
        Pane::Explorer(_) => "explorer",
        // SQLWorkspace and InstanceWorkspace get distinct names so their focus
        // can round-trip through the session snapshot; older snapshots stored
        // both as "workspace" (mapped back to SQLWorkspace for compatibility).
        Pane::SQLWorkspace => "sql_workspace",
        Pane::InstanceWorkspace(_) => "instance_workspace",
        // A discover-open focus is not persisted as discover (it is a transient
        // flow, not a workspace to restore); it serializes to its own name and
        // resolves back to the SQL workspace.
        Pane::Discover(_) => "discover",
    }
}

/// Resolve a parent pane from a persistence-friendly name (session snapshots).
pub fn pane_from_name(name: &str) -> Option<Pane> {
    match name {
        "header" => Some(Pane::Header),
        "explorer" => Some(Pane::Explorer(
            crate::app_shell::nav::ExplorerPane::default(),
        )),
        "sql_workspace" => Some(Pane::SQLWorkspace),
        // A discover-open focus restores to the SQL workspace (discover is a
        // transient flow, not a pane to restore).
        "discover" => Some(Pane::SQLWorkspace),
        "instance_workspace" => Some(Pane::InstanceWorkspace(
            crate::app_shell::nav::IwPane::default(),
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_and_instance_workspace_round_trip_distinctly() {
        // SQLWorkspace and InstanceWorkspace must serialize to different names
        // and resolve back to their own variant (so session focus restores
        // correctly instead of both collapsing to SQLWorkspace).
        assert_eq!(pane_name(Pane::SQLWorkspace), "sql_workspace");
        assert_eq!(
            pane_name(Pane::InstanceWorkspace(crate::app_shell::nav::IwPane::default())),
            "instance_workspace"
        );

        assert_eq!(pane_from_name("sql_workspace"), Some(Pane::SQLWorkspace));
        assert_eq!(
            pane_from_name("instance_workspace"),
            Some(Pane::InstanceWorkspace(crate::app_shell::nav::IwPane::default()))
        );
    }

    #[test]
    fn discover_serializes_as_discover_and_restores_to_sql_workspace() {
        // A discover-open focus is not restored as discover (transient flow);
        // it round-trips to the SQL workspace.
        let p = Pane::Discover(crate::app_shell::nav::DiscoverPane::default());
        assert_eq!(pane_name(p), "discover");
        assert_eq!(pane_from_name("discover"), Some(Pane::SQLWorkspace));
    }

    #[test]
    fn explorer_round_trips() {
        let p = Pane::Explorer(crate::app_shell::nav::ExplorerPane::default());
        assert_eq!(pane_name(p), "explorer");
        assert_eq!(pane_from_name("explorer"), Some(p));
        assert_eq!(pane_from_name("bogus"), None);
    }
}
