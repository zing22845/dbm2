//! Instance connections feature state.

use dbm_store::InstanceConnection;

/// SSL modes offered by the connection form, in cycle order.
pub const SSL_MODES: [&str; 5] = ["disable", "prefer", "require", "verify-ca", "verify-full"];

/// Default `sslmode` for new connections: TLS off.
pub const DEFAULT_SSL_MODE: &str = "disable";

/// The form field currently being edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormField {
    #[default]
    Name,
    Username,
    Database,
    Password,
    /// The `sslmode` selector — a value chooser, not a text field.
    SslMode,
}

impl FormField {
    /// The next field in the form (wrapping), in display order.
    pub fn next(self) -> Self {
        match self {
            FormField::Name => FormField::Username,
            FormField::Username => FormField::Database,
            FormField::Database => FormField::Password,
            FormField::Password => FormField::SslMode,
            FormField::SslMode => FormField::Name,
        }
    }

    /// The previous field in the form (wrapping), in display order.
    pub fn prev(self) -> Self {
        match self {
            FormField::Name => FormField::SslMode,
            FormField::Username => FormField::Name,
            FormField::Database => FormField::Username,
            FormField::Password => FormField::Database,
            FormField::SslMode => FormField::Password,
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
/// the original dbm (success green, failure red) with a dedicated warning tone
/// for blocked-focus notices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionStatusKind {
    /// No status (footer shows only the operation hints).
    #[default]
    Idle,
    /// A successful action/test (green).
    Success,
    /// A real failure: failed save / failed test (error colour).
    Failure,
    /// A blocked interaction — an unsaved edit form tried to leave (warning
    /// colour), not an operation failure.
    Blocked,
}

/// The saved field values an edit form started from, used to detect which
/// fields the user has modified but not yet saved. Passwords are never stored,
/// so password is represented implicitly: any non-empty value is "modified".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionFormBaseline {
    pub name: String,
    pub username: String,
    pub database: String,
    pub ssl_mode: String,
}

/// A connection add/edit form in progress.
#[derive(Debug, Clone)]
pub struct ConnectionForm {
    pub name: String,
    pub username: String,
    pub database: String,
    pub password: String,
    /// libpq-style `sslmode` chosen in the form; new connections start at
    /// [`DEFAULT_SSL_MODE`] (TLS off).
    pub ssl_mode: String,
    /// When editing, the original connection name (used to look up on save).
    pub edit_original_name: Option<String>,
    /// The saved field values when an edit began, used to detect unsaved
    /// modifications. `None` for an add form (no baseline to compare against).
    pub edit_baseline: Option<ConnectionFormBaseline>,
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

impl ConnectionForm {
    /// Whether an *edit* form has unsaved changes relative to its baseline.
    /// Add forms (no baseline) are never dirty, matching the original dbm: only
    /// a modified edit blocks switching pane.
    pub fn is_edit_dirty(&self) -> bool {
        let Some(base) = &self.edit_baseline else {
            return false;
        };
        self.name.trim() != base.name.trim()
            || self.username.trim() != base.username.trim()
            || self.database.trim() != base.database.trim()
            || !self.password.is_empty()
            || self.ssl_mode != base.ssl_mode
    }

    /// Whether a specific field of an edit form differs from its baseline.
    /// Returns `false` for add forms (no baseline).
    pub fn is_field_modified(&self, field: FormField) -> bool {
        let Some(base) = &self.edit_baseline else {
            return false;
        };
        match field {
            FormField::Name => self.name.trim() != base.name.trim(),
            FormField::Username => self.username.trim() != base.username.trim(),
            FormField::Database => self.database.trim() != base.database.trim(),
            FormField::Password => !self.password.is_empty(),
            FormField::SslMode => self.ssl_mode != base.ssl_mode,
        }
    }

    /// Cycle the form's `sslmode` by `delta` steps within [`SSL_MODES`],
    /// wrapping around. Returns whether the value changed.
    pub fn cycle_ssl_mode(&mut self, delta: i32) -> bool {
        let len = SSL_MODES.len() as i32;
        let index = SSL_MODES
            .iter()
            .position(|mode| mode.eq_ignore_ascii_case(self.ssl_mode.trim()))
            .map(|i| i as i32)
            .unwrap_or(0);
        let next = ((index + delta) % len + len) % len;
        let value = SSL_MODES[next as usize].to_string();
        let changed = self.ssl_mode != value;
        self.ssl_mode = value;
        changed
    }
}

impl Default for ConnectionForm {
    fn default() -> Self {
        Self {
            name: String::new(),
            username: String::new(),
            database: String::new(),
            password: String::new(),
            ssl_mode: DEFAULT_SSL_MODE.to_string(),
            edit_original_name: None,
            edit_baseline: None,
            field: FormField::default(),
            mode: FormMode::default(),
            insert_field_snapshot: None,
            pending_d_at: None,
        }
    }
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
    /// Vertical scroll offset in data rows (0 = top row visible).
    pub scroll: usize,
    /// When true, the discover-style cursor anchor is skipped — the viewport
    /// stays exactly where the user last dragged/scrolled it, even if the
    /// cursor moves outside it. Reset on any MoveUp/Down.
    pub scroll_locked: bool,
    /// A cursor to apply once the connections load. Used by session restore,
    /// which must remember the saved row because the list is still empty at
    /// that point (connections load lazily). Consumed by `Loaded`.
    pub restore_cursor: Option<usize>,
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
        let ssl_mode = if conn.ssl_mode.trim().is_empty() {
            DEFAULT_SSL_MODE.to_string()
        } else {
            conn.ssl_mode.clone()
        };
        self.form = Some(ConnectionForm {
            name: conn.name.clone(),
            username: conn.username.clone(),
            database: conn.database.clone(),
            password: String::new(), // passwords are not stored; a blank keeps the old
            ssl_mode: ssl_mode.clone(),
            edit_original_name: Some(conn.name.clone()),
            edit_baseline: Some(ConnectionFormBaseline {
                name: conn.name.clone(),
                username: conn.username.clone(),
                database: conn.database.clone(),
                ssl_mode,
            }),
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
        // The sslmode field is a value chooser, not editable text; this also
        // covers mouse double-clicks, which funnel through here.
        if form.field == FormField::SslMode {
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
        if form.field == FormField::SslMode {
            return false;
        }
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
            FormField::SslMode => &self.ssl_mode,
        }
    }

    /// The current field's value, mutable.
    pub fn field_value_mut(&mut self) -> &mut String {
        match self.field {
            FormField::Name => &mut self.name,
            FormField::Username => &mut self.username,
            FormField::Database => &mut self.database,
            FormField::Password => &mut self.password,
            FormField::SslMode => &mut self.ssl_mode,
        }
    }

    /// Overwrite the current field's value.
    pub fn set_field_value(&mut self, value: &str) {
        match self.field {
            FormField::Name => self.name = value.into(),
            FormField::Username => self.username = value.into(),
            FormField::Database => self.database = value.into(),
            FormField::Password => self.password = value.into(),
            FormField::SslMode => self.ssl_mode = value.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_forms_default_to_ssl_disabled() {
        assert_eq!(ConnectionForm::default().ssl_mode, DEFAULT_SSL_MODE);
        let mut state = ConnectionsState::default();
        state.begin_add();
        assert_eq!(state.form.unwrap().ssl_mode, "disable");
    }

    #[test]
    fn cycle_ssl_mode_wraps_in_both_directions() {
        let mut form = ConnectionForm::default();
        assert!(form.cycle_ssl_mode(1));
        assert_eq!(form.ssl_mode, "prefer");
        form.ssl_mode = "verify-full".into();
        assert!(form.cycle_ssl_mode(1));
        assert_eq!(form.ssl_mode, "disable");
        assert!(form.cycle_ssl_mode(-1));
        assert_eq!(form.ssl_mode, "verify-full");
    }

    #[test]
    fn ssl_mode_counts_towards_edit_dirty() {
        let mut form = ConnectionForm {
            name: "n".into(),
            username: "u".into(),
            database: "d".into(),
            edit_baseline: Some(ConnectionFormBaseline {
                name: "n".into(),
                username: "u".into(),
                database: "d".into(),
                ssl_mode: "disable".into(),
            }),
            ..ConnectionForm::default()
        };
        assert!(!form.is_edit_dirty());
        form.ssl_mode = "require".into();
        assert!(form.is_edit_dirty());
        assert!(form.is_field_modified(FormField::SslMode));
    }

    #[test]
    fn selector_field_is_not_text_editable() {
        let mut state = ConnectionsState::default();
        state.form = Some(ConnectionForm {
            field: FormField::SslMode,
            ..ConnectionForm::default()
        });
        assert!(!state.begin_field_insert());
        assert!(!state.clear_field_and_insert());
        assert_eq!(state.form.unwrap().mode, FormMode::Normal);
    }
}
