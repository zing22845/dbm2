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

impl FormField {
    /// The next field in the form (wrapping), in display order.
    pub fn next(self) -> Self {
        match self {
            FormField::Name => FormField::Username,
            FormField::Username => FormField::Database,
            FormField::Database => FormField::Password,
            FormField::Password => FormField::Name,
        }
    }

    /// The previous field in the form (wrapping), in display order.
    pub fn prev(self) -> Self {
        match self {
            FormField::Name => FormField::Password,
            FormField::Username => FormField::Name,
            FormField::Database => FormField::Username,
            FormField::Password => FormField::Database,
        }
    }
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
    /// One-line status shown on the connections pane footer (e.g. "Added …").
    /// Owned by this pane so it does not leak into the overview footer.
    pub status: Option<String>,
}

impl ConnectionsState {
    pub fn move_up(&mut self) -> bool {
        let before = self.cursor;
        self.cursor = self.cursor.saturating_sub(1);
        self.cursor != before
    }

    pub fn move_down(&mut self) -> bool {
        if self.connections.is_empty() {
            return false;
        }
        let before = self.cursor;
        self.cursor = (self.cursor + 1).min(self.connections.len() - 1);
        self.cursor != before
    }

    /// Open an empty add-connection form. Returns whether a form was opened.
    pub fn begin_add(&mut self) -> bool {
        self.form = Some(ConnectionForm {
            username: "postgres".into(),
            database: "postgres".into(),
            ..ConnectionForm::default()
        });
        true
    }

    /// Open an edit form seeded from the connection at `idx`. Returns whether
    /// the form was opened (i.e. `idx` was valid).
    pub fn begin_edit(&mut self, idx: usize) -> bool {
        let Some(conn) = self.connections.get(idx) else {
            return false;
        };
        self.form = Some(ConnectionForm {
            name: conn.name.clone(),
            username: conn.username.clone(),
            database: conn.database.clone(),
            password: String::new(), // passwords are not stored; a blank keeps the old
            edit_original_name: Some(conn.name.clone()),
            field: FormField::Name,
        });
        true
    }

    /// The selected connection's name (for deletion), if the cursor is on one.
    pub fn selected_name(&self) -> Option<String> {
        self.connections.get(self.cursor).map(|c| c.name.clone())
    }
}
