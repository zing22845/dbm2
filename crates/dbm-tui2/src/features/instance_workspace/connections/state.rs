//! Instance connections feature state.

use dbm_store::InstanceConnection;

/// The form field currently being edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormField {
    #[default]
    Name,
    Username,
    Database,
    Password,
}

/// A connection add/edit form in progress.
#[derive(Debug, Clone, Default)]
pub struct ConnectionForm {
    pub name: String,
    pub username: String,
    pub database: String,
    pub password: String,
    /// When editing, the original connection name (used to look up on save).
    pub edit_original_name: Option<String>,
    /// The form field under the edit cursor.
    pub field: FormField,
}

/// State for the instance connections panel.
#[derive(Debug, Clone, Default)]
pub struct ConnectionsState {
    /// The instance whose connections are shown.
    pub instance_name: String,
    /// The instance's connections.
    pub connections: Vec<InstanceConnection>,
    /// Cursor row within the connection list.
    pub cursor: usize,
    /// Whether an add/edit form is open.
    pub form: Option<ConnectionForm>,
}

impl ConnectionsState {
    pub fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if self.connections.is_empty() {
            return;
        }
        self.cursor = (self.cursor + 1).min(self.connections.len() - 1);
    }

    /// Open an empty add-connection form.
    pub fn begin_add(&mut self) {
        self.form = Some(ConnectionForm {
            username: "postgres".into(),
            database: "postgres".into(),
            ..ConnectionForm::default()
        });
    }

    /// Open an edit form seeded from the connection at `idx`.
    pub fn begin_edit(&mut self, idx: usize) {
        let Some(conn) = self.connections.get(idx) else {
            return;
        };
        self.form = Some(ConnectionForm {
            name: conn.name.clone(),
            username: conn.username.clone(),
            database: conn.database.clone(),
            password: String::new(), // passwords are not stored; a blank keeps the old
            edit_original_name: Some(conn.name.clone()),
            field: FormField::Name,
        });
    }

    /// The selected connection's name (for deletion), if the cursor is on one.
    pub fn selected_name(&self) -> Option<String> {
        self.connections.get(self.cursor).map(|c| c.name.clone())
    }
}
