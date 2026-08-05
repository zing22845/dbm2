//! Global application state. `AppState` aggregates the state of every feature
//! plus shell-level metadata (focus, quit flag, status, modal).

use crate::app_shell::focus::FocusZone;
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
    /// Which top-level region owns keyboard input.
    pub focus: FocusZone,
    /// Whether the application should quit.
    pub should_quit: bool,
    /// A short global status line (rendered by the footer).
    pub global_status: String,
    /// Currently active modal, if any. `None` means no modal is shown.
    pub modal: Option<ModalKind>,

    // --- Feature states ---
    pub header: HeaderState,
    pub explorer: ExplorerState,
    pub discover: DiscoverState,
    pub iw: IwState,
    pub sql: SqlState,
    pub footer: FooterState,
    pub perf: PerfState,
}

/// The kind of modal currently displayed. New variants (e.g. `Alert`,
/// `Confirm`, `Prompt`) will be added as modals are implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            focus: FocusZone::Header,
            should_quit: false,
            global_status: String::new(),
            modal: None,
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
