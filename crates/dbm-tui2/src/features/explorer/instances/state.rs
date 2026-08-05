//! Explorer instances (connection tree) feature state.

use dbm_store::{InstanceConnection, ManagedInstance};

/// A single instance node in the tree: the managed instance plus its loaded
/// connections and expansion state.
#[derive(Debug, Clone, Default)]
pub struct InstanceNode {
    /// The managed instance.
    pub instance: Option<ManagedInstance>,
    /// Whether the instance's connections are expanded.
    pub expanded: bool,
    /// The instance's connections (lazily loaded).
    pub connections: Vec<InstanceConnection>,
    /// Whether connections have been loaded from the store.
    pub loaded: bool,
}

impl InstanceNode {
    fn display_name(&self) -> String {
        self.instance
            .as_ref()
            .map(|i| i.name.clone())
            .unwrap_or_default()
    }

    fn connection_count(&self) -> usize {
        self.connections.len()
    }
}

/// State for the explorer instances (connection tree) pane.
#[derive(Debug, Clone, Default)]
pub struct InstancesState {
    /// The tree's instance nodes, in display order.
    pub nodes: Vec<InstanceNode>,
    /// Cursor row within the flat visible list.
    pub cursor: usize,
    /// Scroll offset of the tree.
    pub scroll: usize,
}

impl InstancesState {
    /// The number of visible rows (each instance + its expanded connections).
    pub fn visible_count(&self) -> usize {
        self.nodes
            .iter()
            .map(|n| if n.expanded { 1 + n.connection_count() } else { 1 })
            .sum()
    }

    /// Move the cursor up (clamped).
    pub fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Move the cursor down (clamped to the visible list).
    pub fn move_down(&mut self) {
        let max = self.visible_count().saturating_sub(1);
        self.cursor = (self.cursor + 1).min(max);
    }

    /// Toggle the expansion of the instance the cursor is on (if the cursor is
    /// on an instance row).
    pub fn toggle_expand(&mut self) {
        if let Some((node, _)) = self.node_at_cursor_mut() {
            node.expanded = !node.expanded;
        }
    }

    /// The visible row index of the node the cursor is on, if any.
    fn node_at_cursor_mut(&mut self) -> Option<(&mut InstanceNode, usize)> {
        let mut row = 0usize;
        for node in self.nodes.iter_mut() {
            if row == self.cursor {
                return Some((node, row));
            }
            row += 1;
            if node.expanded {
                row += node.connection_count();
            }
            if self.cursor < row {
                break;
            }
        }
        None
    }

    /// Resolve the cursor's position to either an instance or a connection.
    /// Returns `(instance_idx, Option<conn_idx>)`.
    pub fn cursor_selection(&self) -> Option<(usize, Option<usize>)> {
        let mut row = 0usize;
        for (i, node) in self.nodes.iter().enumerate() {
            if row == self.cursor {
                return Some((i, None));
            }
            row += 1;
            if node.expanded {
                for (ci, _) in node.connections.iter().enumerate() {
                    if row == self.cursor {
                        return Some((i, Some(ci)));
                    }
                    row += 1;
                }
            }
            if self.cursor < row {
                break;
            }
        }
        None
    }

    /// Replace the tree with freshly loaded instances.
    pub fn set_instances(&mut self, instances: Vec<ManagedInstance>) {
        self.nodes = instances
            .into_iter()
            .map(|i| InstanceNode {
                instance: Some(i),
                expanded: false,
                connections: Vec::new(),
                loaded: false,
            })
            .collect();
        self.cursor = 0;
        self.scroll = 0;
    }

    /// Display name of the instance at `idx`.
    pub fn instance_name(&self, idx: usize) -> String {
        self.nodes.get(idx).map(|n| n.display_name()).unwrap_or_default()
    }

    /// The `(instance_name, connection_name)` at the cursor, if it is a
    /// connection row.
    pub fn connection_at_cursor(&self) -> Option<(String, String)> {
        let (i, conn) = self.cursor_selection()?;
        let conn = conn?;
        let instance_name = self.nodes.get(i)?.display_name();
        let conn_name = self.nodes.get(i)?.connections.get(conn)?.name.clone();
        Some((instance_name, conn_name))
    }
}
