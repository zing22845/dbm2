//! Global application state. `AppState` aggregates the state of every feature
//! plus shell-level metadata (focus, quit flag, status, modal).

use crate::app_shell::pane::Pane;
use crate::common::view::theme::Theme;
use crate::features::discover::state::DiscoverState;
use crate::features::explorer::state::ExplorerState;
use crate::features::global_footer::state::FooterState;
use crate::features::header::state::HeaderState;
use crate::features::instance_workspace::state::IwState;
use crate::features::perf_monitor::state::PerfState;
use crate::features::sql_workspace::state::SqlState;

/// Aggregated application state.
#[derive(Debug)]
pub struct AppState {
    /// Which parent pane (and discover sub-pane, if open) owns keyboard input.
    pub focus: Pane,
    /// Whether the application should quit.
    pub should_quit: bool,
    /// A short global status line (rendered by the footer).
    pub global_status: String,
    /// Currently active modal, if any. `None` means no modal is shown.
    pub modal: Option<ModalKind>,
    /// The active theme, injected into every view as a rendering context.
    pub theme: Theme,
    /// Terminal width in columns, updated on resize. Used by explorer
    /// horizontal-scroll to clamp at the content boundary instead of a
    /// fixed cap.
    pub term_width: u16,

    // --- Feature states ---
    pub header: HeaderState,
    pub explorer: ExplorerState,
    pub discover: DiscoverState,
    pub iw: IwState,
    pub sql: SqlState,
    pub footer: FooterState,
    pub perf: PerfState,
}

/// The kind of modal currently displayed. Each variant carries the minimal
/// payload its popup needs to render (the live results/tree state that owns the
/// values lives in the owning feature; the modal stores a snapshot for the
/// popup's lifetime).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalKind {
    /// Choose a rows-per-page limit for the current result.
    ResultsRowLimitPicker { current: usize, limits: Vec<usize> },
    /// Type a specific page number to jump to.
    ResultsPageInput { current_page: usize, total_pages: Option<usize> },
    /// Confirm deleting a stored connection.
    DeleteConnectionConfirm { instance: String, connection: String },
    /// Confirm unregistering an instance.
    UnregisterInstanceConfirm { instance: String },
    /// Preview the edit-batch statements before committing.
    ResultsEditCommitPreview { statements: Vec<String> },
}

impl AppState {
    /// Whether an instance workspace is currently active (its `◆` marker is
    /// set in the explorer tree). This is the single source of truth for "is
    /// the instance workspace shown", matching the original dbm's
    /// `active_workspace` state. The workspace region renders the instance
    /// workspace exactly when this is true.
    pub fn instance_workspace_open(&self) -> bool {
        self.explorer.instances.active_is_instance()
    }
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            focus: Pane::Header,
            should_quit: false,
            global_status: String::new(),
            modal: None,
            term_width: 0,
            theme: crate::common::view::theme::dracula(),
            header: HeaderState::default(),
            explorer: ExplorerState::default(),
            discover: DiscoverState::default(),
            iw: IwState::default(),
            sql: SqlState::default(),
            footer: FooterState::default(),
            perf: PerfState::default(),
        }
    }
}
