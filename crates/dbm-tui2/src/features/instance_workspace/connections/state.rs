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

/// The connection form input mode, matching the original dbm: typing happens in
/// an explicit per-field insert mode (`i`), not directly into the fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormMode {
    /// No field is being edited; `j`/`k` move, `Enter` saves the whole form,
    /// `Esc` cancels.
    #[default]
    Normal,
    /// The current field is being edited; keys insert/delete, `Enter` commits
    /// the field, `Esc` reverts it.
    Insert,
}

/// The kind of status shown on the connections footer, used to color it like
/// the original dbm (success green, failure red).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionStatusKind {
    /// No status (footer shows only the operation hints).
    #[default]
    Idle,
    /// A successful action/test (green).
    Success,
    /// A failed action/test (red).
    Failure,
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
    /// Whether a field is currently being edited (`Insert`) or not (`Normal`).
    pub mode: FormMode,
    /// Snapshot of the current field value taken when insert mode began, used to
    /// revert on `Esc` (matching the original dbm's insert-field cancel).
    pub insert_field_snapshot: Option<String>,
    /// Timestamp of the first `d` press, used to detect the `dd` clear-field
    /// chord (a second `d` within the timeout clears the field and enters
    /// insert mode, matching the original dbm).
    pub pending_d_at: Option<std::time::Instant>,
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
    /// Status text shown on the connections footer (e.g. a test result).
    /// Owned by this pane so it does not leak into the overview footer.
    pub status: Option<String>,
    /// How `status` is colored (success green, failure red), matching dbm.
    pub status_kind: ConnectionStatusKind,
    /// Cooldown deadline for `t` (connection test) — shared by the form and the
    /// list, limiting tests to at most once per second to avoid waste.
    pub test_cooldown_until: Option<std::time::Instant>,
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
    ///
    /// The default `username` is the current system user (`$USER`), falling back
    /// to "postgres", matching the original dbm's add form.
    pub fn begin_add(&mut self) -> bool {
        let default_user = std::env::var("USER").unwrap_or_else(|_| "postgres".into());
        self.form = Some(ConnectionForm {
            username: default_user,
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
            ..ConnectionForm::default()
        });
        true
    }

    /// The selected connection's name (for deletion), if the cursor is on one.
    pub fn selected_name(&self) -> Option<String> {
        self.connections.get(self.cursor).map(|c| c.name.clone())
    }

    /// Enter insert mode on the current field, snapshotting its value so `Esc`
    /// can revert it. A no-op when already in insert mode.
    pub fn begin_field_insert(&mut self) -> bool {
        let Some(form) = &mut self.form else {
            return false;
        };
        if form.mode == FormMode::Insert {
            return false;
        }
        form.insert_field_snapshot = Some(form.field_value().to_string());
        form.mode = FormMode::Insert;
        true
    }

    /// Commit the current field: clear the insert snapshot and return to normal
    /// mode. The just-typed value is kept.
    pub fn commit_field_insert(&mut self) -> bool {
        let Some(form) = &mut self.form else {
            return false;
        };
        if form.mode != FormMode::Insert {
            return false;
        }
        form.insert_field_snapshot = None;
        form.mode = FormMode::Normal;
        true
    }

    /// Cancel the current field edit, reverting it to the snapshot taken when
    /// insert mode began.
    pub fn cancel_field_insert(&mut self) -> bool {
        let Some(form) = &mut self.form else {
            return false;
        };
        if form.mode != FormMode::Insert {
            return false;
        }
        if let Some(snapshot) = form.insert_field_snapshot.take() {
            form.set_field_value(&snapshot);
        }
        form.mode = FormMode::Normal;
        true
    }

    /// Insert a character into the current field. Only active in insert mode.
    pub fn form_insert_char(&mut self, ch: char) -> bool {
        let Some(form) = &mut self.form else {
            return false;
        };
        if form.mode != FormMode::Insert {
            return false;
        }
        form.field_value_mut().push(ch);
        true
    }

    /// Backspace the last character of the current field (insert mode only).
    pub fn form_backspace(&mut self) -> bool {
        let Some(form) = &mut self.form else {
            return false;
        };
        if form.mode != FormMode::Insert {
            return false;
        }
        form.field_value_mut().pop().is_some()
    }

    /// Clear the current field and enter insert mode, snapshotting the old
    /// value so `Esc` restores it (the original dbm's `dd` clear).
    pub fn clear_field_and_insert(&mut self) -> bool {
        let Some(form) = &mut self.form else {
            return false;
        };
        form.insert_field_snapshot = Some(form.field_value().to_string());
        form.set_field_value("");
        form.mode = FormMode::Insert;
        form.pending_d_at = None;
        true
    }

    /// Record the first `d` press for the `dd` clear-field chord.
    pub fn set_pending_d(&mut self) -> bool {
        let Some(form) = &mut self.form else {
            return false;
        };
        form.pending_d_at = Some(std::time::Instant::now());
        false
    }
}

impl ConnectionForm {
    /// The current field's value, read-only.
    pub fn field_value(&self) -> &str {
        match self.field {
            FormField::Name => &self.name,
            FormField::Username => &self.username,
            FormField::Database => &self.database,
            FormField::Password => &self.password,
        }
    }

    /// The current field's value, mutable.
    pub fn field_value_mut(&mut self) -> &mut String {
        match self.field {
            FormField::Name => &mut self.name,
            FormField::Username => &mut self.username,
            FormField::Database => &mut self.database,
            FormField::Password => &mut self.password,
        }
    }

    /// Overwrite the current field's value.
    pub fn set_field_value(&mut self, value: &str) {
        match self.field {
            FormField::Name => self.name = value.into(),
            FormField::Username => self.username = value.into(),
            FormField::Database => self.database = value.into(),
            FormField::Password => self.password = value.into(),
        }
    }
}
