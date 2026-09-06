//! Results pane geometry.

use super::list::view as list_view;
use super::splitter::view as splitter_view;
use ratatui::layout::Rect;

/// The Results feature's full layout, resolved once so the renderer and every
/// hit-test path share identical geometry.
pub struct ResultsLayout {
    /// The content band narrowed to the list side of the list|detail split.
    pub list: Rect,
    /// The list|detail splitter strip, when the detail is open.
    pub splitter: Option<Rect>,
    /// The detail preview area (within the content band), when the detail is open.
    pub detail: Option<Rect>,
    /// The full-width pagination toolbar, when there are rows to paginate.
    pub pagination: Option<Rect>,
    /// The full-width list footer.
    pub footer: Rect,
}

/// Resolve the Results Block's `inner` area into its full layout: a vertical
/// split over the whole width (content band + full-width pagination toolbar +
/// full-width footer), then a horizontal split of the content band into the
/// list | splitter | detail. See [`ResultsLayout`].
pub fn compute_results_layout(
    inner: Rect,
    detail_open: bool,
    detail_pane_width: u16,
    row_count: usize,
    search_active: bool,
    sql_status: &str,
) -> ResultsLayout {
    let (content, pagination, footer) =
        list_view::results_vertical_layout(inner, row_count, search_active, sql_status);
    let (list, splitter, detail) =
        splitter_view::split_inner(content, detail_open, detail_pane_width);
    ResultsLayout {
        list,
        splitter,
        detail,
        pagination,
        footer,
    }
}
