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
    /// Horizontal scroll offset of the tree (`Left`/`Right`), matching the
    /// original dbm's tree horizontal scroll.
    pub h_scroll: u16,
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
    /// Move the cursor up (clamped). Returns whether the cursor actually moved,
    /// so callers can skip a redundant repaint at the top.
    pub fn move_up(&mut self) -> bool {
        let before = self.cursor;
        self.cursor = self.cursor.saturating_sub(1);
        self.cursor != before
    }

    /// Move the cursor down (clamped to the visible list). Returns whether the
    /// cursor actually moved, so callers can skip a redundant repaint at the
    /// bottom.
    pub fn move_down(&mut self) -> bool {
        let max = self.visible_count().saturating_sub(1);
        let before = self.cursor;
        self.cursor = (self.cursor + 1).min(max);
        self.cursor != before
    }

    /// Expand the instance the cursor is on (if the cursor is on an instance
    /// row and it isn't already expanded). Returns whether expansion changed.
    pub fn expand(&mut self) -> bool {
        if let Some((node, _)) = self.node_at_cursor_mut()
            && !node.expanded
        {
            node.expanded = true;
            true
        } else {
            false
        }
    }

    /// Collapse the instance the cursor is on (if the cursor is on an instance
    /// row and it is currently expanded). Returns whether collapse changed.
    pub fn collapse(&mut self) -> bool {
        if let Some((node, _)) = self.node_at_cursor_mut()
            && node.expanded
        {
            node.expanded = false;
            true
        } else {
            false
        }
    }

    /// The display width (in columns) of the widest rendered row. Used to
    /// clamp horizontal scrolling so it stops at the content boundary.
    pub fn max_row_width(&self) -> u16 {
        self.nodes
            .iter()
            .map(|n| {
                let mut w: u16 = 0;
                if let Some(inst) = &n.instance {
                    // Instance row: " ▸/▾ name"
                    let marker = if n.expanded { "▾" } else { "▸" };
                    w = w.max(
                        (2
                            + unicode_width::UnicodeWidthStr::width(marker)
                            + unicode_width::UnicodeWidthStr::width(inst.name.as_str()))
                        .try_into()
                        .unwrap_or(u16::MAX),
                    );
                }
                if n.expanded {
                    for c in &n.connections {
                        // Connection row: "    └ name"
                        let conn_w: usize = 4 + 1 // └
                            + unicode_width::UnicodeWidthStr::width(c.name.as_str());
                        w = w.max(conn_w.try_into().unwrap_or(u16::MAX));
                    }
                }
                w
            })
            .max()
            .unwrap_or(0)
    }

    /// Scroll the tree horizontally by `delta` columns, clamped to `[0, max]`.
    /// Returns whether the scroll offset actually moved, so callers can skip a
    /// redundant repaint when already at a boundary (matching the original dbm,
    /// where a no-op horizontal scroll does not redraw).
    pub fn scroll_horizontal(&mut self, delta: i16, max: u16) -> bool {
        let before = self.h_scroll;
        self.h_scroll = (self.h_scroll as i32 + i32::from(delta))
            .clamp(0, i32::from(max)) as u16;
        self.h_scroll != before
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

    /// Replace the tree with freshly loaded instances, preserving which
    /// instances were expanded (matching the original dbm's `reload_tree`, which
    /// re-expands and re-loads the previously selected instance). Connections
    /// are intentionally left unloaded here; they reload lazily on expand.
    pub fn set_instances(&mut self, instances: Vec<ManagedInstance>) {
        let expanded_names: std::collections::HashSet<String> = self
            .nodes
            .iter()
            .filter(|n| n.expanded)
            .filter_map(|n| n.instance.as_ref())
            .map(|i| i.name.clone())
            .collect();
        self.nodes = instances
            .into_iter()
            .map(|i| {
                let expanded = expanded_names.contains(&i.name);
                InstanceNode {
                    instance: Some(i),
                    expanded,
                    connections: Vec::new(),
                    loaded: false,
                }
            })
            .collect();
        self.cursor = 0;
        self.scroll = 0;
    }

    /// Remove the instance node at `idx` (e.g. after unregistering it) and
    /// clamp the cursor/scroll into the new tree. Returns whether a node was
    /// actually removed.
    pub fn remove_instance(&mut self, idx: usize) -> bool {
        if idx >= self.nodes.len() {
            return false;
        }
        self.nodes.remove(idx);
        let max = self.visible_count().saturating_sub(1);
        self.cursor = self.cursor.min(max);
        self.scroll = self.scroll.min(max);
        true
    }

    /// Update a single instance's metadata in place (matching the original
    /// dbm's `reload_managed_instance`), preserving its expansion state and any
    /// loaded connections. Returns whether the named instance was found.
    pub fn set_single_instance(&mut self, instance: ManagedInstance) -> bool {
        if let Some(node) = self
            .nodes
            .iter_mut()
            .find(|n| n.display_name() == instance.name)
        {
            node.instance = Some(instance);
            true
        } else {
            false
        }
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

    /// The id of the connection matching `instance`/`connection` by name, if
    /// present. Used to resolve the connection_id for an object-tree target.
    pub fn connection_id_by_name(&self, instance: &str, connection: &str) -> Option<String> {
        self.nodes.iter().find_map(|node| {
            if node.display_name() != instance {
                return None;
            }
            node.connections
                .iter()
                .find(|c| c.name == connection)
                .map(|c| c.id.clone())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(name: &str) -> ManagedInstance {
        ManagedInstance {
            id: format!("id-{name}"),
            fingerprint: format!("fp-{name}"),
            name: name.to_string(),
            engine: dbm_core::Engine::Postgres,
            host: "127.0.0.1".to_string(),
            port: 5432,
            socket_path: None,
            data_dir: None,
            env_label: None,
            registered_at: "now".to_string(),
            version_full: None,
            version_short: None,
            version_checked_at: None,
            lifecycle_status: None,
            lifecycle_checked_at: None,
            lifecycle_detail: None,
        }
    }

    fn conn(name: &str) -> InstanceConnection {
        InstanceConnection {
            id: format!("c-{name}"),
            instance_id: "id".to_string(),
            name: name.to_string(),
            username: "postgres".to_string(),
            database: "postgres".to_string(),
            has_password: false,
            ssl_mode: String::new(),
            env_label: None,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
        }
    }

    #[test]
    fn set_instances_preserves_expanded_instances() {
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a"), inst("b")]);
        s.nodes[0].expanded = true;
        s.nodes[0].loaded = true;
        s.nodes[0].connections = vec![conn("c")];

        // Reloading keeps "a" expanded but drops its (stale) connections.
        s.set_instances(vec![inst("a"), inst("c")]);
        assert!(s.nodes[0].expanded, "previously expanded instance stays expanded");
        assert!(!s.nodes[0].loaded, "connections are re-lazily loaded");
        assert!(s.nodes[0].connections.is_empty());
        assert!(!s.nodes[1].expanded);
    }

    #[test]
    fn remove_instance_removes_and_clamps_cursor() {
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a"), inst("b"), inst("c")]);
        s.cursor = 2;
        assert!(s.remove_instance(1));
        assert_eq!(s.nodes.len(), 2);
        assert_eq!(s.instance_name(1), "c");
        assert!(s.cursor <= 1, "cursor clamped into the new tree");
        assert!(!s.remove_instance(5), "out-of-range removal is a no-op");
    }

    #[test]
    fn set_single_instance_updates_only_matching_node() {
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a"), inst("b")]);
        s.nodes[0].expanded = true;
        s.nodes[0].connections = vec![conn("c")];

        let mut updated = inst("b");
        updated.host = "10.0.0.5".to_string();
        assert!(s.set_single_instance(updated));
        // "b" metadata updated; "a" expansion + connections untouched.
        assert_eq!(s.nodes[1].instance.as_ref().unwrap().host, "10.0.0.5");
        assert!(s.nodes[0].expanded);
        assert_eq!(s.nodes[0].connections.len(), 1);

        // Unknown instance name returns false.
        assert!(!s.set_single_instance(inst("nope")));
    }
}
