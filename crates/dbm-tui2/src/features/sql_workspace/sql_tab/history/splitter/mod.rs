//! `history` child feature: the internal splitter between the history list and
//! the detail preview (splitter B).
//!
//! Unlike the `sql_tab`-level splitters (editor/history A and
//! editor+history/results), this splitter lives *inside* the History pane: it
//! separates the detail preview (left) from the list (right). It owns
//! `detail_pane_width`. The History zone geometry (`history_zone_width`,
//! `history_zone_x`, `history_detail_splitter`) is computed here.

pub mod layout;
pub mod state;
pub mod view;
