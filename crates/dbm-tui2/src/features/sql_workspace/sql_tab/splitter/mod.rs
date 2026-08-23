//! `sql_tab` child feature: the splitter strips owned by the SQL tab.
//!
//! The SQL tab has two resizable splitters that live at the `sql_tab` level
//! (they separate panes owned directly by the tab, not by a child feature):
//!
//! - **A — editor/history** (`SqlSplitter::EditorHistory`): the vertical strip
//!   between the SQL editor and the History pane. It owns `history_pane_width`.
//! - **Editor/history row vs results** (`SqlSplitter::EditorResults`): the
//!   horizontal strip between the top row and the results pane. It owns
//!   `editor_top_height` (the top-pane height in rows; a percentage is only
//!   materialized for session persistence / resizes).
//!
//! The History-internal detail/list splitter (B) is *not* here — it is a child
//! of the `history` feature (`crate::...::history::splitter`).

pub mod state;
pub mod view;
