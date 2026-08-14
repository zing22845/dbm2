//! Explorer instances (connection tree) feature state.

use dbm_store::{InstanceConnection, ManagedInstance};

/// Which workspace is currently active in the tree, matching the original dbm's
/// mutually-exclusive `active_workspace`. At most one node is active: an
/// instance (its instance workspace is shown) or a connection (its SQL
/// workspace is shown). This is the single source of truth for the active-row
/// highlight in the tree and for what the workspace region renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveWorkspaceKind {
    /// The instance at `instance_idx` owns the instance workspace.
    Instance(usize),
    /// The connection at `conn_idx` under `instance_idx` owns the SQL workspace.
    Connection {
        instance_idx: usize,
        conn_idx: usize,
    },
}

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
    /// The active workspace (instance or connection), matching the original
    /// dbm's `ConnectionTreeState::active_workspace`. `None` when no workspace
    /// has been opened yet. Drives the active-row highlight and the workspace render.
    pub active_workspace: Option<ActiveWorkspaceKind>,
    /// A saved connection-active restored from a session whose connection rows
    /// were not loaded yet. `(instance_name, connection_name)`; once the
    /// instance's connections load, the active workspace is refined to that
    /// connection (so a restart keeps the active highlight on the connection,
    /// not its parent instance).
    pub restore_active_connection: Option<(String, String)>,
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

    /// Jump the cursor to a specific visible row (mouse click), clamped to the
    /// visible list. Returns whether the cursor moved.
    pub fn jump_to(&mut self, row: usize) -> bool {
        let max = self.visible_count().saturating_sub(1);
        let target = row.min(max);
        let before = self.cursor;
        self.cursor = target;
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

    /// Expand/collapse the instance at the given visible `row` (a mouse marker
    /// click) without moving the cursor. Connection rows are ignored (they have
    /// no marker). Collapsing an active instance — or an active connection's
    /// parent instance — is blocked (it is forced expanded), matching the
    /// original dbm. Returns whether expansion changed.
    pub fn toggle_expand_at(&mut self, row: usize) -> bool {
        let (is_connection, idx) = self.visible_row_is_connection(row);
        if is_connection || idx == usize::MAX {
            return false;
        }
        // Resolve the collapse-block before the mutable borrow so the immutable
        // reads of `active_workspace` do not overlap it.
        let expanded = self.nodes.get(idx).is_some_and(|n| n.expanded);
        let collapse_blocked = expanded
            && (self.is_active_instance(idx)
                || matches!(
                    self.active_workspace,
                    Some(ActiveWorkspaceKind::Connection { instance_idx: ai, .. })
                        if ai == idx
                ));
        if let Some(node) = self.nodes.get_mut(idx) {
            if node.expanded {
                if collapse_blocked {
                    return false;
                }
                node.expanded = false;
            } else {
                node.expanded = true;
            }
            true
        } else {
            false
        }
    }

    /// Collapse the instance the cursor is on (if the cursor is on an instance
    /// row and it is currently expanded). Collapsing an active instance — or an
    /// active connection's parent instance — is blocked (it is forced expanded),
    /// matching the original dbm. Returns whether collapse changed.
    pub fn collapse(&mut self) -> bool {
        if self.cursor_collapse_blocked() {
            return false;
        }
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
                    // Instance row: " ▸/▾ name" — a leading space, the expand
                    // marker, and a space.
                    let marker = if n.expanded { "▾" } else { "▸" };
                    w = w.max(
                        (3
                            + unicode_width::UnicodeWidthStr::width(marker)
                            + unicode_width::UnicodeWidthStr::width(inst.name.as_str()))
                        .try_into()
                        .unwrap_or(u16::MAX),
                    );
                }
                if n.expanded {
                    for c in &n.connections {
                        // Connection row: "    └ name/db" — 4 spaces, "└ ", name
                        // and "/" + db.
                        let conn_w: usize = 6
                            + unicode_width::UnicodeWidthStr::width(c.name.as_str())
                            + unicode_width::UnicodeWidthStr::width(c.database.as_str());
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

    /// Whether the instance at `instance_idx` is the active workspace.
    pub fn is_active_instance(&self, instance_idx: usize) -> bool {
        self.active_workspace == Some(ActiveWorkspaceKind::Instance(instance_idx))
    }

    /// Whether the connection at `(instance_idx, conn_idx)` is the active
    /// workspace.
    pub fn is_active_connection(&self, instance_idx: usize, conn_idx: usize) -> bool {
        self.active_workspace
            == Some(ActiveWorkspaceKind::Connection {
                instance_idx,
                conn_idx,
            })
    }

    /// Whether the active workspace is an instance (its instance workspace is
    /// shown) rather than a connection.
    pub fn active_is_instance(&self) -> bool {
        matches!(self.active_workspace, Some(ActiveWorkspaceKind::Instance(_)))
    }

    /// Make the instance at `instance_idx` the active workspace. Its node is
    /// forced expanded so the active workspace stays visible.
    pub fn set_active_instance(&mut self, instance_idx: usize) {
        self.active_workspace = Some(ActiveWorkspaceKind::Instance(instance_idx));
        if let Some(node) = self.nodes.get_mut(instance_idx) {
            node.expanded = true;
        }
    }

    /// Make the connection at `(instance_idx, conn_idx)` the active workspace.
    /// Its parent instance node is forced expanded so the active connection
    /// stays visible.
    pub fn set_active_connection(&mut self, instance_idx: usize, conn_idx: usize) {
        self.active_workspace = Some(ActiveWorkspaceKind::Connection {
            instance_idx,
            conn_idx,
        });
        if let Some(node) = self.nodes.get_mut(instance_idx) {
            node.expanded = true;
        }
    }

    /// Whether collapsing is blocked because the cursor's node is on the active
    /// path (an active instance, or an active connection's parent instance).
    pub fn cursor_collapse_blocked(&self) -> bool {
        let Some((instance_idx, _)) = self.cursor_selection() else {
            return false;
        };
        self.is_active_instance(instance_idx)
            || matches!(
                self.active_workspace,
                Some(ActiveWorkspaceKind::Connection {
                    instance_idx: ai,
                    ..
                }) if ai == instance_idx
            )
    }

    /// Clear the active workspace (e.g. the instance was unregistered).
    pub fn clear_active_workspace(&mut self) {
        self.active_workspace = None;
    }

    /// Whether the given visible row is a connection row (`true`) or an
    /// instance row (`false`). Used by mouse hit-testing to find the expand/
    /// collapse marker column.
    pub fn visible_row_is_connection(&self, target_row: usize) -> (bool, usize) {
        let mut row = 0usize;
        for (i, node) in self.nodes.iter().enumerate() {
            if row == target_row {
                return (false, i);
            }
            row += 1;
            if node.expanded {
                row += node.connections.len();
                if target_row < row {
                    return (true, i);
                }
            }
            if target_row < row {
                break;
            }
        }
        (false, usize::MAX)
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
    ///
    /// The active workspace is re-resolved by name after the reload because
    /// node indices may have changed (matching the original dbm, which re-maps
    /// its active instance/connection after a reload).
    pub fn set_instances(&mut self, instances: Vec<ManagedInstance>) {
        let expanded_names: std::collections::HashSet<String> = self
            .nodes
            .iter()
            .filter(|n| n.expanded)
            .filter_map(|n| n.instance.as_ref())
            .map(|i| i.name.clone())
            .collect();
        let prev_active = self.active_workspace;
        // Resolve the previously active instance's name before the reload.
        let prev_instance_name = match prev_active {
            Some(ActiveWorkspaceKind::Instance(i))
            | Some(ActiveWorkspaceKind::Connection {
                instance_idx: i,
                conn_idx: _,
            }) => self.nodes.get(i).map(|n| n.display_name()),
            None => None,
        };
        // If a connection was active, capture its name so the connection row can
        // be re-resolved once the instance's connections reload. Without this a
        // tree reload (e.g. after closing discover, which re-fetches instances)
        // would permanently degrade a connection-active to its parent instance.
        let prev_connection_name = match prev_active {
            Some(ActiveWorkspaceKind::Connection { instance_idx, conn_idx }) => {
                self.nodes
                    .get(instance_idx)
                    .and_then(|n| n.connections.get(conn_idx))
                    .map(|c| c.name.clone())
            }
            _ => None,
        };
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
        // Re-map the active workspace onto the new node indices by name. When a
        // connection was active, its connections are not loaded after a tree
        // reload, so we fall back to the instance workspace for that instance;
        // the connection row is re-resolved once its connections load.
        self.active_workspace = match &prev_instance_name {
            Some(iname) => self
                .nodes
                .iter()
                .position(|n| n.display_name() == *iname)
                .map(ActiveWorkspaceKind::Instance),
            None => None,
        };
        // Re-resolve a connection-active through the SAME mechanism session
        // restore (`apply_snapshot`) uses: queue its name in
        // `restore_active_connection`, leave the workspace on the parent
        // instance meanwhile, and let the single `ConnectionsLoaded` refinement
        // in the update layer do the wait-then-activate (or degrade) once the
        // instance's connections reload. The `Loaded` handler re-fetches
        // connections for every expanded instance, and the active instance is
        // always expanded, so the reload always fires `ConnectionsLoaded`.
        if let (Some(iname), Some(conn_name)) = (prev_instance_name, prev_connection_name) {
            self.restore_active_connection = Some((iname, conn_name));
        }
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
        // If the removed instance owned the active workspace, clear it so we
        // don't keep a stale marker / workspace for a node that no longer
        // exists.
        let removed_active = match self.active_workspace {
            Some(ActiveWorkspaceKind::Instance(i)) => i == idx,
            Some(ActiveWorkspaceKind::Connection {
                instance_idx: i,
                conn_idx: _,
            }) => i == idx,
            None => false,
        };
        if removed_active {
            self.active_workspace = None;
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
            test_succeeded_at: None,
            test_failed_at: None,
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

    #[test]
    fn visible_row_is_connection_distinguishes_rows() {
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a"), inst("b")]);
        s.nodes[0].expanded = true;
        s.nodes[0].loaded = true;
        s.nodes[0].connections = vec![conn("c1")];
        // Row 0 = instance a, row 1 = connection c1, row 2 = instance b.
        assert_eq!(s.visible_row_is_connection(0), (false, 0));
        assert_eq!(s.visible_row_is_connection(1), (true, 0));
        assert_eq!(s.visible_row_is_connection(2), (false, 1));
    }

    #[test]
    fn jump_to_moves_and_clamps_cursor() {
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a"), inst("b"), inst("c")]);
        assert_eq!(s.visible_count(), 3);
        s.jump_to(1);
        assert_eq!(s.cursor, 1);
        // Clamp to the visible count.
        assert!(s.jump_to(100));
        assert_eq!(s.cursor, 2);
        // Same row -> no movement.
        assert!(!s.jump_to(2));
    }

    #[test]
    fn active_workspace_tracks_instance_and_connection() {
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a"), inst("b")]);
        s.nodes[0].expanded = true;
        s.nodes[0].loaded = true;
        s.nodes[0].connections = vec![conn("c1"), conn("c2")];

        // Default: no active workspace, not instance-open.
        assert!(!s.active_is_instance());

        // Set an instance active.
        s.set_active_instance(0);
        assert!(s.is_active_instance(0));
        assert!(!s.is_active_instance(1));
        assert!(s.active_is_instance());

        // Switch to a connection — the instance highlight clears (mutually
        // exclusive, matching the original dbm).
        s.set_active_connection(0, 1);
        assert!(s.is_active_connection(0, 1));
        assert!(!s.is_active_connection(0, 0));
        assert!(!s.is_active_instance(0));
        assert!(!s.active_is_instance());

        s.clear_active_workspace();
        assert!(!s.is_active_instance(0));
        assert!(!s.is_active_connection(0, 1));
    }

    #[test]
    fn set_active_forces_expand_parent_and_blocks_collapse() {
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a")]);
        s.nodes[0].expanded = false;

        // Activating an instance (or a connection under it) forces its node open.
        s.set_active_instance(0);
        assert!(s.nodes[0].expanded, "active instance is forced expanded");

        // Collapsing the active instance is blocked.
        s.cursor = 0;
        assert!(!s.collapse(), "active instance must not collapse");

        // A connection active also keeps the parent instance forced open.
        s.nodes[0].expanded = true;
        s.nodes[0].loaded = true;
        s.nodes[0].connections = vec![conn("c1")];
        s.set_active_connection(0, 0);
        s.cursor = 0; // on the instance row
        assert!(!s.collapse(), "active connection's parent must not collapse");
    }

    #[test]
    fn set_instances_preserves_active_instance_by_name() {
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a"), inst("b"), inst("c")]);
        s.set_active_instance(1); // "b" is active

        // Reload with a reordered/shorter list; the active marker follows the
        // instance by name (now at index 0), and a connection active downgrades
        // to its instance workspace.
        s.set_instances(vec![inst("b"), inst("c")]);
        assert!(s.active_is_instance());
        assert!(s.is_active_instance(0), "active instance follows by name");

        // Removing the active instance clears the marker.
        s.remove_instance(0);
        assert!(!s.active_is_instance());
    }

    #[test]
    fn set_instances_preserves_connection_active_via_restore() {
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a")]);
        s.nodes[0].expanded = true;
        s.nodes[0].loaded = true;
        s.nodes[0].connections = vec![conn("c1"), conn("c2")];
        s.set_active_connection(0, 1); // connection "c2" is active

        // A tree reload (e.g. after closing discover) re-fetches instances. The
        // connection is temporarily downgraded to its parent instance, but the
        // connection name is queued so `ConnectionsLoaded` can re-activate it.
        s.set_instances(vec![inst("a")]);
        assert_eq!(
            s.active_workspace,
            Some(ActiveWorkspaceKind::Instance(0)),
            "reload falls back to the parent instance"
        );
        assert_eq!(
            s.restore_active_connection,
            Some(("a".to_string(), "c2".to_string())),
            "connection-active is queued for re-resolution"
        );
    }

    #[test]
    fn toggle_expand_at_toggles_that_row_without_moving_cursor() {
        // Two instances; the cursor is on row 0. Row 1 (the second instance) is
        // the marker click target.
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a"), inst("b")]);
        s.cursor = 0;
        assert_eq!(s.nodes[1].expanded, false);
        assert!(s.toggle_expand_at(1), "marker click expands row 1");
        assert_eq!(s.nodes[1].expanded, true);
        assert_eq!(s.cursor, 0, "cursor must not move");
        // Toggle again collapses it.
        assert!(s.toggle_expand_at(1), "marker click collapses row 1");
        assert_eq!(s.nodes[1].expanded, false);
        assert_eq!(s.cursor, 0, "cursor still must not move");
    }

    #[test]
    fn toggle_expand_at_blocks_collapsing_an_active_instance() {
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a")]);
        s.set_active_instance(0); // forced expanded
        assert!(!s.toggle_expand_at(0), "active instance cannot be collapsed");
        assert!(s.nodes[0].expanded);
    }

    #[test]
    fn remove_instance_clears_active_workspace_when_owned() {
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a"), inst("b")]);
        s.set_active_instance(1);
        assert!(s.remove_instance(1));
        assert!(s.active_workspace.is_none(), "removing active instance clears it");

        // Removing a non-active instance keeps the marker.
        s.set_active_instance(0);
        assert!(s.remove_instance(0));
        assert!(s.active_workspace.is_none());
    }
}
