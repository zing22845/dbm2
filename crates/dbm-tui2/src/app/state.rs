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
    /// Terminal height in rows, updated on resize. Used to derive the SQL tab
    /// body height when converting a persisted horizontal-split percentage back
    /// to absolute rows (and vice-versa).
    pub term_height: u16,
    /// App-level splitter state: the width of the Explorer pane (left) vs the
    /// workspace region (right).
    pub splitter: crate::features::app_splitter::state::AppSplitterState,

    /// Splitter hover highlight state — which splitter the mouse cursor is
    /// currently over. Updated on every `MouseMove` event; renders passively
    /// read these booleans to decide which style the splitter line uses.
    pub splitter_hover: SplitterHoverState,

    // --- Feature states ---
    pub header: HeaderState,
    pub explorer: ExplorerState,
    pub discover: DiscoverState,
    pub iw: IwState,
    pub sql: SqlState,
    pub footer: FooterState,
    pub perf: PerfState,
}

/// Which splitter regions the mouse cursor currently hovers over.
/// Each field maps to one splitter line drawn on screen.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SplitterHoverState {
    // --- Hover booleans (set by MouseMove hit-testing) ---
    /// App-level Explorer / workspace vertical splitter.
    pub app_splitter: bool,
    /// Explorer instances / objects horizontal splitter.
    pub explorer_splitter: bool,
    /// Discover targets / results horizontal splitter.
    pub discover_splitter: bool,
    /// SQL tab horizontal splitter (editor+history row vs results).
    pub sql_editor_results: bool,
    /// SQL tab vertical splitter (editor vs history).
    pub sql_editor_history: bool,
    /// History detail / list internal vertical splitter.
    pub history_detail: bool,
    /// Results detail / list internal vertical splitter.
    pub results_detail: bool,

    // --- Drag booleans (set on splitter Down/Drag/Up events) ---
    /// The app-level Explorer / workspace splitter is being dragged.
    pub app_splitter_drag: bool,
    /// The explorer instances / objects splitter is being dragged.
    pub explorer_splitter_drag: bool,
    /// The discover targets / results splitter is being dragged.
    pub discover_splitter_drag: bool,
    /// The SQL horizontal splitter (editor+history vs results) is being dragged.
    pub sql_editor_results_drag: bool,
    /// The SQL vertical splitter (editor vs history) is being dragged.
    pub sql_editor_history_drag: bool,
    /// The history detail / list internal splitter is being dragged.
    pub sql_history_detail_drag: bool,
    /// The results detail / list internal splitter is being dragged.
    pub sql_results_detail_drag: bool,
}

impl SplitterHoverState {
    /// A copy of just the drag booleans, used to preserve active drags across a
    /// hover recompute (see `update_splitter_hover`).
    pub fn dragging_flags(&self) -> [bool; 7] {
        [
            self.app_splitter_drag,
            self.explorer_splitter_drag,
            self.discover_splitter_drag,
            self.sql_editor_results_drag,
            self.sql_editor_history_drag,
            self.sql_history_detail_drag,
            self.sql_results_detail_drag,
        ]
    }

    /// Restore the drag booleans from a previously saved copy.
    pub fn set_dragging_flags(&mut self, flags: [bool; 7]) {
        self.app_splitter_drag = flags[0];
        self.explorer_splitter_drag = flags[1];
        self.discover_splitter_drag = flags[2];
        self.sql_editor_results_drag = flags[3];
        self.sql_editor_history_drag = flags[4];
        self.sql_history_detail_drag = flags[5];
        self.sql_results_detail_drag = flags[6];
    }
}

impl AppState {
    /// The single choke point for changing the active pane. Keeps the focus
    /// (used to route keyboard input) and each feature's displayed sub-pane
    /// (`explorer.pane`, `iw.pane`) in lockstep, so rendering and input can
    /// never disagree about which sub-pane is active.
    ///
    /// Every focus change — the shell `FocusChanged` message, session restore,
    /// and intent-driven jumps — must go through this method. Setting
    /// `state.focus` directly while separately assigning a feature sub-pane
    /// (as session restore once did) is what lets the two drift apart.
    pub fn set_focus(&mut self, pane: Pane) {
        self.focus = pane;
        if let Pane::Explorer(sub) = pane
            && self.explorer.pane != sub
        {
            self.explorer.pane = sub;
        }
        if let Pane::InstanceWorkspace(sub) = pane
            && self.iw.pane != sub
        {
            self.iw.pane = sub;
        }
    }
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
    /// Whether an instance workspace is currently active (its active highlight
    /// is set in the explorer tree). This is the single source of truth for "is
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
            term_height: 0,
            splitter: crate::features::app_splitter::state::AppSplitterState::default(),
            splitter_hover: SplitterHoverState::default(),
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
