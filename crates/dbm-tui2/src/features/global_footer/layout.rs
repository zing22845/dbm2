//! Global footer metrics: how many terminal rows the footer occupies.

use crate::common::layout::text::footer_height as text_height;
use crate::common::view::hints::global_footer_text;

use super::state::FooterState;

/// Estimated number of terminal rows the footer occupies for `cols` columns.
///
/// The footer is the hints line (wrapped to `cols`) plus (when present) one
/// wrapped status line. Both use the same CJK-aware estimate the old TUI used
/// to size its footer.
pub fn footer_height(state: &FooterState, cols: u16) -> u16 {
    let hints_rows = text_height(&global_footer_text(""), cols);
    let status_rows = if state.status.is_empty() {
        0
    } else {
        text_height(&state.status, cols)
    };
    hints_rows + status_rows
}
