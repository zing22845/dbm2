//! Pure spatial navigation types for the pane hierarchy.
//!
//! The shell tracks keyboard focus as a `Pane` (see `super::pane`), where a
//! parent pane may host child sub-panes. This module holds the *pure* navigation
//! vocabulary that the shell and input layer share: the spatial direction enum,
//! the key-to-direction mapping, and the child-pane enums (`ExplorerPane`,
//! `DiscoverPane`) with their prev/next cycling. Features re-export the child
//! pane type they use for their own state.
//!
//! Everything here is self-contained and side-effect-free: it only maps keys to
//! directions and moves sub-pane enums, without touching state, IO or any
//! feature. It is unit-tested in isolation.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A spatial navigation direction (Ctrl+h/j/k/l or Ctrl+arrows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneDir {
    Left,
    Down,
    Up,
    Right,
}

/// Sub-pane within the Explorer parent pane (`[I] Instances` / `[O] Objects`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExplorerPane {
    #[default]
    Instances,
    Objects,
}

impl ExplorerPane {
    /// Move focus to the previous explorer sub-pane (wrapping).
    pub fn prev(self) -> Self {
        match self {
            ExplorerPane::Instances => ExplorerPane::Objects,
            ExplorerPane::Objects => ExplorerPane::Instances,
        }
    }

    /// Move focus to the next explorer sub-pane (wrapping).
    pub fn next(self) -> Self {
        match self {
            ExplorerPane::Instances => ExplorerPane::Objects,
            ExplorerPane::Objects => ExplorerPane::Instances,
        }
    }
}

/// Sub-pane within the Discover parent pane (engine / targets / results).
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

/// Sub-pane within the InstanceWorkspace parent pane (overview / connections).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IwPane {
    /// The instance overview panel.
    #[default]
    Overview,
    /// The instance connections panel.
    Connections,
}

impl IwPane {
    /// Move focus to the previous instance-workspace sub-pane (wrapping).
    pub fn prev(self) -> Self {
        match self {
            IwPane::Overview => IwPane::Connections,
            IwPane::Connections => IwPane::Overview,
        }
    }

    /// Move focus to the next instance-workspace sub-pane (wrapping).
    pub fn next(self) -> Self {
        match self {
            IwPane::Overview => IwPane::Connections,
            IwPane::Connections => IwPane::Overview,
        }
    }
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
        // `KeyCode::Backspace` maps to Ctrl+h: many terminals report Ctrl+h as
        // ASCII 0x08 (Backspace), which crossterm parses as `Backspace` rather
        // than `Char('h')`. Without this, Ctrl+h navigation silently stops
        // working on those terminals.
        KeyCode::Char('h') | KeyCode::Backspace | KeyCode::Left => Some(PaneDir::Left),
        KeyCode::Char('j') | KeyCode::Down => Some(PaneDir::Down),
        KeyCode::Char('k') | KeyCode::Up => Some(PaneDir::Up),
        KeyCode::Char('l') | KeyCode::Right => Some(PaneDir::Right),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn ctrl_hjkl_map_to_directions() {
        let ctrl = KeyModifiers::CONTROL;
        assert_eq!(
            pane_dir_from_key(&key(KeyCode::Char('h'), ctrl)),
            Some(PaneDir::Left)
        );
        assert_eq!(
            pane_dir_from_key(&key(KeyCode::Char('j'), ctrl)),
            Some(PaneDir::Down)
        );
        assert_eq!(
            pane_dir_from_key(&key(KeyCode::Char('k'), ctrl)),
            Some(PaneDir::Up)
        );
        assert_eq!(
            pane_dir_from_key(&key(KeyCode::Char('l'), ctrl)),
            Some(PaneDir::Right)
        );
        // Arrow keys without Ctrl are not directional.
        assert!(pane_dir_from_key(&key(KeyCode::Left, KeyModifiers::NONE)).is_none());
        // Ctrl+h is often encoded as Backspace by terminals.
        assert_eq!(
            pane_dir_from_key(&key(KeyCode::Backspace, ctrl)),
            Some(PaneDir::Left)
        );
        // Extra modifiers are rejected.
        assert!(pane_dir_from_key(&key(KeyCode::Char('j'), ctrl | KeyModifiers::SHIFT)).is_none());
    }

    #[test]
    fn explorer_pane_cycles_instances_objects() {
        assert_eq!(ExplorerPane::Instances.next(), ExplorerPane::Objects);
        assert_eq!(ExplorerPane::Objects.next(), ExplorerPane::Instances);
        assert_eq!(ExplorerPane::Instances.prev(), ExplorerPane::Objects);
        assert_eq!(ExplorerPane::default(), ExplorerPane::Instances);
    }
}
