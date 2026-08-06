//! Pure spatial zone/pane navigation for the main TUI.
//!
//! This module is self-contained and side-effect-free: it maps spatial keys to
//! directions, computes within-zone neighbors, and resolves cross-zone moves,
//! all without touching state, IO or any feature. It is unit-tested in
//! isolation so the adjacency rules can be verified before any key wiring.
//!
//! The concrete pane/zone enums are defined here (rather than on any feature)
//! so the resolver stays decoupled; a feature later maps its own state enums
//! onto these values when wiring the keys.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A spatial navigation direction (Ctrl+h/j/k/l or Ctrl+arrows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneDir {
    Left,
    Down,
    Up,
    Right,
}

/// A top-level focus zone in the shell layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Header,
    Explorer,
    Workspace,
    Instance,
}

/// Sub-pane within the Explorer zone (`[I] Instances` / `[O] Objects`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerPane {
    Instances,
    Objects,
}

/// Sub-pane within the Workspace (SQL tab): editor / history / results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkspacePane {
    #[default]
    Sql,
    Results,
    History,
}

/// Stacked pane within the Instance workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InstancePane {
    #[default]
    Overview,
    Connections,
    Lifecycle,
    Monitor,
    Backup,
}

impl InstancePane {
    const ALL: [Self; 5] = [
        Self::Overview,
        Self::Connections,
        Self::Lifecycle,
        Self::Monitor,
        Self::Backup,
    ];

    /// Whether this pane is currently enabled (rendered & navigable).
    pub fn enabled(self) -> bool {
        matches!(self, Self::Overview | Self::Connections)
    }

    /// The next enabled pane (rightwards), skipping disabled ones, wrapping to
    /// `None` when none exists.
    pub fn next_enabled_no_wrap(self) -> Option<Self> {
        Self::ALL
            .iter()
            .skip_while(|p| **p != self)
            .skip(1)
            .copied()
            .find(|p| p.enabled())
    }

    /// The previous enabled pane (leftwards), skipping disabled ones, wrapping
    /// to `None` when none exists.
    pub fn prev_enabled_no_wrap(self) -> Option<Self> {
        Self::ALL
            .iter()
            .rev()
            .skip_while(|p| **p != self)
            .skip(1)
            .copied()
            .find(|p| p.enabled())
    }
}

/// Sub-focus within the Discover modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverFocus {
    Engine,
    Targets,
    Results,
}

/// Resolved destination for a spatial move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveTarget {
    Header,
    /// Enter Explorer zone; keep current `explorer_pane`.
    ExplorerZone,
    ExplorerPane(ExplorerPane),
    /// Enter Workspace zone; restore last sub-pane via `focus_workspace`.
    WorkspaceZone,
    WorkspacePane(WorkspacePane),
    /// Switch stacked Instance Manager pane (Overview / Connections / …).
    ManagerPane(InstancePane),
    /// Results Detail section (inside Results pane).
    ResultsDetailSection,
    /// Results Table section (clear Detail focus; stay on Results).
    ResultsTableSection,
}

/// Ctrl+h/j/k/l or Ctrl+arrows. Reject if Shift/Alt/Super also held.
pub fn pane_dir_from_key(key: &KeyEvent) -> Option<PaneDir> {
    if !key.modifiers.contains(KeyModifiers::CONTROL)
        || key
            .modifiers
            .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::SUPER)
    {
        return None;
    }
    match key.code {
        KeyCode::Char('h') | KeyCode::Left => Some(PaneDir::Left),
        KeyCode::Char('j') | KeyCode::Down => Some(PaneDir::Down),
        KeyCode::Char('k') | KeyCode::Up => Some(PaneDir::Up),
        KeyCode::Char('l') | KeyCode::Right => Some(PaneDir::Right),
        _ => None,
    }
}

/// Within-Discover adjacency only (Engine → Targets → Results).
pub fn discover_neighbor(focus: DiscoverFocus, dir: PaneDir) -> Option<DiscoverFocus> {
    match (focus, dir) {
        (DiscoverFocus::Engine, PaneDir::Down) => Some(DiscoverFocus::Targets),
        (DiscoverFocus::Targets, PaneDir::Down) => Some(DiscoverFocus::Results),
        (DiscoverFocus::Targets, PaneDir::Up) => Some(DiscoverFocus::Engine),
        (DiscoverFocus::Results, PaneDir::Up) => Some(DiscoverFocus::Targets),
        _ => None,
    }
}

/// Within-Explorer adjacency only (no cross-zone).
pub fn explorer_neighbor(pane: ExplorerPane, dir: PaneDir) -> Option<ExplorerPane> {
    match (pane, dir) {
        (ExplorerPane::Instances, PaneDir::Down) => Some(ExplorerPane::Objects),
        (ExplorerPane::Objects, PaneDir::Up) => Some(ExplorerPane::Instances),
        _ => None,
    }
}

/// Within-Workspace (SQL) adjacency only (no cross-zone).
pub fn workspace_neighbor(
    pane: WorkspacePane,
    dir: PaneDir,
    upper: WorkspacePane,
) -> Option<WorkspacePane> {
    let upper = match upper {
        WorkspacePane::Sql | WorkspacePane::History => upper,
        WorkspacePane::Results => WorkspacePane::Sql,
    };
    match (pane, dir) {
        (WorkspacePane::Sql, PaneDir::Right) => Some(WorkspacePane::History),
        (WorkspacePane::Sql, PaneDir::Down) => Some(WorkspacePane::Results),
        (WorkspacePane::History, PaneDir::Left) => Some(WorkspacePane::Sql),
        (WorkspacePane::History, PaneDir::Down) => Some(WorkspacePane::Results),
        (WorkspacePane::Results, PaneDir::Up) => Some(upper),
        _ => None,
    }
}

/// Within-Manager stacked panes (horizontal only; skips `!enabled`).
pub fn manager_neighbor(pane: InstancePane, dir: PaneDir) -> Option<InstancePane> {
    match dir {
        PaneDir::Right => pane.next_enabled_no_wrap(),
        PaneDir::Left => pane.prev_enabled_no_wrap(),
        _ => None,
    }
}

/// Full spatial move: within-zone neighbor first, then cross-zone edge.
///
/// When Results Detail section is open, Table ↔ Detail are horizontal neighbors.
///
/// The 8-argument signature is a faithful port of the reference resolver; the
/// whole set of inputs is needed to decide a single destination.
#[allow(clippy::too_many_arguments)]
pub fn resolve_move(
    zone: Zone,
    explorer_pane: ExplorerPane,
    workspace_pane: Option<WorkspacePane>,
    workspace_upper: WorkspacePane,
    manager_pane: InstancePane,
    dir: PaneDir,
    results_detail_open: bool,
    results_detail_focused: bool,
) -> Option<MoveTarget> {
    match zone {
        Zone::Header => match dir {
            PaneDir::Down => Some(MoveTarget::ExplorerZone),
            _ => None,
        },
        Zone::Explorer => {
            if let Some(next) = explorer_neighbor(explorer_pane, dir) {
                return Some(MoveTarget::ExplorerPane(next));
            }
            match dir {
                PaneDir::Up => Some(MoveTarget::Header),
                PaneDir::Right => Some(MoveTarget::WorkspaceZone),
                _ => None,
            }
        }
        Zone::Workspace => {
            let Some(pane) = workspace_pane else {
                return match dir {
                    PaneDir::Left => Some(MoveTarget::ExplorerZone),
                    PaneDir::Up => Some(MoveTarget::Header),
                    _ => None,
                };
            };

            if pane == WorkspacePane::Results && results_detail_open {
                if results_detail_focused {
                    return match dir {
                        PaneDir::Left => Some(MoveTarget::ResultsTableSection),
                        PaneDir::Up => {
                            let upper = match workspace_upper {
                                WorkspacePane::Sql | WorkspacePane::History => workspace_upper,
                                WorkspacePane::Results => WorkspacePane::Sql,
                            };
                            Some(MoveTarget::WorkspacePane(upper))
                        }
                        PaneDir::Right | PaneDir::Down => None,
                    };
                }
                if dir == PaneDir::Right {
                    return Some(MoveTarget::ResultsDetailSection);
                }
            }

            if let Some(next) = workspace_neighbor(pane, dir, workspace_upper) {
                return Some(MoveTarget::WorkspacePane(next));
            }
            match (pane, dir) {
                (WorkspacePane::Sql | WorkspacePane::Results, PaneDir::Left) => {
                    Some(MoveTarget::ExplorerZone)
                }
                (WorkspacePane::Sql | WorkspacePane::History, PaneDir::Up) => {
                    Some(MoveTarget::Header)
                }
                _ => None,
            }
        }
        Zone::Instance => {
            if let Some(next) = manager_neighbor(manager_pane, dir) {
                return Some(MoveTarget::ManagerPane(next));
            }
            match dir {
                PaneDir::Left => Some(MoveTarget::ExplorerZone),
                PaneDir::Up => Some(MoveTarget::Header),
                _ => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn pane_dir_from_ctrl_hjkl_and_arrows() {
        assert_eq!(pane_dir_from_key(&ctrl(KeyCode::Char('h'))), Some(PaneDir::Left));
        assert_eq!(pane_dir_from_key(&ctrl(KeyCode::Char('j'))), Some(PaneDir::Down));
        assert_eq!(pane_dir_from_key(&ctrl(KeyCode::Char('k'))), Some(PaneDir::Up));
        assert_eq!(pane_dir_from_key(&ctrl(KeyCode::Char('l'))), Some(PaneDir::Right));
        assert_eq!(pane_dir_from_key(&ctrl(KeyCode::Left)), Some(PaneDir::Left));
        assert_eq!(pane_dir_from_key(&ctrl(KeyCode::Down)), Some(PaneDir::Down));
        assert_eq!(pane_dir_from_key(&ctrl(KeyCode::Up)), Some(PaneDir::Up));
        assert_eq!(pane_dir_from_key(&ctrl(KeyCode::Right)), Some(PaneDir::Right));
        assert_eq!(
            pane_dir_from_key(&KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
            None
        );
    }

    #[test]
    fn discover_adjacency() {
        assert_eq!(
            discover_neighbor(DiscoverFocus::Engine, PaneDir::Down),
            Some(DiscoverFocus::Targets)
        );
        assert_eq!(
            discover_neighbor(DiscoverFocus::Targets, PaneDir::Down),
            Some(DiscoverFocus::Results)
        );
        assert_eq!(
            discover_neighbor(DiscoverFocus::Targets, PaneDir::Up),
            Some(DiscoverFocus::Engine)
        );
        assert_eq!(
            discover_neighbor(DiscoverFocus::Results, PaneDir::Up),
            Some(DiscoverFocus::Targets)
        );
        assert_eq!(discover_neighbor(DiscoverFocus::Engine, PaneDir::Left), None);
        assert_eq!(discover_neighbor(DiscoverFocus::Results, PaneDir::Down), None);
    }

    #[test]
    fn explorer_jk_within_zone() {
        assert_eq!(
            explorer_neighbor(ExplorerPane::Instances, PaneDir::Down),
            Some(ExplorerPane::Objects)
        );
        assert_eq!(
            explorer_neighbor(ExplorerPane::Objects, PaneDir::Up),
            Some(ExplorerPane::Instances)
        );
        assert_eq!(explorer_neighbor(ExplorerPane::Instances, PaneDir::Left), None);
        assert_eq!(explorer_neighbor(ExplorerPane::Instances, PaneDir::Right), None);
    }

    #[test]
    fn workspace_spatial_neighbors() {
        assert_eq!(
            workspace_neighbor(WorkspacePane::Sql, PaneDir::Right, WorkspacePane::Sql),
            Some(WorkspacePane::History)
        );
        assert_eq!(
            workspace_neighbor(WorkspacePane::Sql, PaneDir::Down, WorkspacePane::Sql),
            Some(WorkspacePane::Results)
        );
        assert_eq!(
            workspace_neighbor(WorkspacePane::History, PaneDir::Left, WorkspacePane::Sql),
            Some(WorkspacePane::Sql)
        );
        assert_eq!(
            workspace_neighbor(WorkspacePane::Results, PaneDir::Up, WorkspacePane::History),
            Some(WorkspacePane::History)
        );
        assert_eq!(
            workspace_neighbor(WorkspacePane::Results, PaneDir::Up, WorkspacePane::Results),
            Some(WorkspacePane::Sql)
        );
    }

    #[test]
    fn cross_zone_from_header_and_explorer() {
        assert_eq!(
            resolve_move(
                Zone::Header,
                ExplorerPane::Instances,
                Some(WorkspacePane::Sql),
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Down,
                false,
                false
            ),
            Some(MoveTarget::ExplorerZone)
        );
        assert_eq!(
            resolve_move(
                Zone::Explorer,
                ExplorerPane::Instances,
                Some(WorkspacePane::Sql),
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Up,
                false,
                false
            ),
            Some(MoveTarget::Header)
        );
        assert_eq!(
            resolve_move(
                Zone::Explorer,
                ExplorerPane::Objects,
                Some(WorkspacePane::Sql),
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Right,
                false,
                false
            ),
            Some(MoveTarget::WorkspaceZone)
        );
        // Objects Up stays within Explorer
        assert_eq!(
            resolve_move(
                Zone::Explorer,
                ExplorerPane::Objects,
                Some(WorkspacePane::Sql),
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Up,
                false,
                false
            ),
            Some(MoveTarget::ExplorerPane(ExplorerPane::Instances))
        );
    }

    #[test]
    fn empty_workspace_still_leaves_via_ctrl_hjkl_edges() {
        assert_eq!(
            resolve_move(
                Zone::Workspace,
                ExplorerPane::Instances,
                None,
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Left,
                false,
                false
            ),
            Some(MoveTarget::ExplorerZone)
        );
        assert_eq!(
            resolve_move(
                Zone::Workspace,
                ExplorerPane::Objects,
                None,
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Up,
                false,
                false
            ),
            Some(MoveTarget::Header)
        );
        assert_eq!(
            resolve_move(
                Zone::Workspace,
                ExplorerPane::Instances,
                None,
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Down,
                false,
                false
            ),
            None
        );
    }

    #[test]
    fn cross_zone_from_workspace() {
        assert_eq!(
            resolve_move(
                Zone::Workspace,
                ExplorerPane::Instances,
                Some(WorkspacePane::Sql),
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Left,
                false,
                false
            ),
            Some(MoveTarget::ExplorerZone)
        );
        assert_eq!(
            resolve_move(
                Zone::Workspace,
                ExplorerPane::Instances,
                Some(WorkspacePane::Sql),
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Up,
                false,
                false
            ),
            Some(MoveTarget::Header)
        );
        // History Left stays within Workspace
        assert_eq!(
            resolve_move(
                Zone::Workspace,
                ExplorerPane::Instances,
                Some(WorkspacePane::History),
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Left,
                false,
                false
            ),
            Some(MoveTarget::WorkspacePane(WorkspacePane::Sql))
        );
        // Results Up stays within Workspace
        assert_eq!(
            resolve_move(
                Zone::Workspace,
                ExplorerPane::Instances,
                Some(WorkspacePane::Results),
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Up,
                false,
                false
            ),
            Some(MoveTarget::WorkspacePane(WorkspacePane::Sql))
        );
    }

    #[test]
    fn results_detail_horizontal_neighbors() {
        assert_eq!(
            resolve_move(
                Zone::Workspace,
                ExplorerPane::Instances,
                Some(WorkspacePane::Results),
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Right,
                true,
                false,
            ),
            Some(MoveTarget::ResultsDetailSection)
        );
        assert_eq!(
            resolve_move(
                Zone::Workspace,
                ExplorerPane::Instances,
                Some(WorkspacePane::Results),
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Left,
                true,
                true,
            ),
            Some(MoveTarget::ResultsTableSection)
        );
    }

    #[test]
    fn results_table_left_still_leaves_to_explorer_when_detail_open() {
        assert_eq!(
            resolve_move(
                Zone::Workspace,
                ExplorerPane::Instances,
                Some(WorkspacePane::Results),
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Left,
                true,
                false,
            ),
            Some(MoveTarget::ExplorerZone)
        );
    }

    #[test]
    fn manager_adjacency() {
        assert_eq!(
            manager_neighbor(InstancePane::Overview, PaneDir::Right),
            Some(InstancePane::Connections)
        );
        assert_eq!(
            manager_neighbor(InstancePane::Connections, PaneDir::Left),
            Some(InstancePane::Overview)
        );
        assert_eq!(manager_neighbor(InstancePane::Overview, PaneDir::Left), None);
        assert_eq!(manager_neighbor(InstancePane::Connections, PaneDir::Right), None);
        assert_eq!(manager_neighbor(InstancePane::Overview, PaneDir::Down), None);
    }

    #[test]
    fn resolve_move_manager_edges() {
        assert_eq!(
            resolve_move(
                Zone::Instance,
                ExplorerPane::Instances,
                None,
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Left,
                false,
                false
            ),
            Some(MoveTarget::ExplorerZone)
        );
        assert_eq!(
            resolve_move(
                Zone::Instance,
                ExplorerPane::Instances,
                None,
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Right,
                false,
                false
            ),
            Some(MoveTarget::ManagerPane(InstancePane::Connections))
        );
        assert_eq!(
            resolve_move(
                Zone::Instance,
                ExplorerPane::Instances,
                None,
                WorkspacePane::Sql,
                InstancePane::Connections,
                PaneDir::Left,
                false,
                false
            ),
            Some(MoveTarget::ManagerPane(InstancePane::Overview))
        );
        assert_eq!(
            resolve_move(
                Zone::Instance,
                ExplorerPane::Instances,
                None,
                WorkspacePane::Sql,
                InstancePane::Connections,
                PaneDir::Right,
                false,
                false
            ),
            None
        );
        assert_eq!(
            resolve_move(
                Zone::Instance,
                ExplorerPane::Instances,
                None,
                WorkspacePane::Sql,
                InstancePane::Connections,
                PaneDir::Up,
                false,
                false
            ),
            Some(MoveTarget::Header)
        );
        assert_eq!(
            resolve_move(
                Zone::Instance,
                ExplorerPane::Instances,
                None,
                WorkspacePane::Sql,
                InstancePane::Overview,
                PaneDir::Down,
                false,
                false
            ),
            None
        );
    }
}
