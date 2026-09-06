//! Which splitter a press resolved to.
//!
//! A press arms **at most one** splitter, and each variant fixes the axis that
//! drag reads — a vertical splitter owns a width (reads `x`), a horizontal one
//! owns a height (reads `y`) — so a drag can never resize two splits, nor move
//! the dimension it does not own.

use ratatui::layout::Rect;

use crate::app::state::AppState;
use crate::app_shell::pane::Pane;
use crate::features::global_footer::layout as footer_layout;

use crate::app::geometry::{app_explorer_rect, sql_tab_area_for_hit};

/// The splitter a mouse press resolved to, i.e. the one being drag-resized.
///
/// A single `Option` (rather than one flag per splitter) makes a drag mutually
/// exclusive **by construction**: `Down` arms at most one target, and `Drag`
/// dispatches on that target alone, so a drag can never resize a second split.
///
/// Each variant also fixes the axis its drag reads — a vertical splitter owns a
/// width (reads `x`), a horizontal one owns a height (reads `y`) — so dragging
/// a splitter cannot move the other dimension either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SplitterDrag {
    /// App-level Explorer / workspace splitter (vertical: width, reads `x`).
    App,
    /// Explorer instances / objects splitter (horizontal: height, reads `y`).
    Explorer,
    /// Discover targets / results splitter (horizontal: height, reads `y`).
    Discover,
    /// A SQL-tab splitter inside the given tab (axis depends on the variant).
    Sql(
        usize,
        crate::features::sql_workspace::sql_tab::splitter::view::SqlSplitter,
    ),
}

/// Resolve which splitter a press at `(x, y)` starts a drag on.
///
/// Covers the splitters hit-tested **after** the click has re-mapped focus —
/// the app-level Explorer/workspace one, the Explorer instances/objects one and
/// the SQL-tab ones. (The discover splitter is resolved earlier, while focus is
/// still the pre-click one, so it is deliberately not part of this function.)
///
/// Candidates are checked in a fixed priority order and the first hit wins, so
/// a press arms **at most one** splitter: a single drag can never resize two
/// splits, and each target later reads only the axis it owns. Every candidate
/// is gated on actually being rendered, so a drag cannot start on a splitter
/// the user cannot see.
pub(crate) fn resolve_splitter_drag(
    state: &AppState,
    size: ratatui::layout::Size,
    x: u16,
    y: u16,
) -> Option<SplitterDrag> {
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_layout::footer_height(&state.footer, size.width));
    if body_h < 3 {
        return None;
    }

    // The app-level Explorer / workspace splitter is draggable from any focus
    // pane: it separates two peer top-level panes rather than the panes of one
    // feature.
    {
        let body_area = Rect::new(0, body_top, size.width, body_h);
        let layout = crate::features::app_splitter::layout::app_body_layout(
            body_area,
            state.splitter.explorer_pane_width,
        );
        if crate::features::app_splitter::view::splitter_at(&layout, x, y) {
            return Some(SplitterDrag::App);
        }
    }

    // The Explorer instances / objects splitter: only while the Explorer owns
    // focus, since the split lives inside the Explorer pane.
    if matches!(state.focus, Pane::Explorer(_))
        && let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
        && explorer.height >= 3
    {
        let inner = Rect::new(
            explorer.x.saturating_add(1),
            explorer.y.saturating_add(1),
            explorer.width.saturating_sub(2),
            explorer.height.saturating_sub(2),
        );
        let layout = crate::features::explorer::splitter::layout::explorer_body_layout(
            inner,
            state.explorer.splitter.instances_height,
        );
        if crate::features::explorer::splitter::view::splitter_at(&layout, x, y) {
            return Some(SplitterDrag::Explorer);
        }
    }

    // The SQL-tab splitters: only while the SQL workspace owns focus. The
    // feature resolves the point to a splitter; the shell only supplies the
    // area and the coordinates.
    if state.focus == Pane::SQLWorkspace
        && let Some(tab_area) = sql_tab_area_for_hit(size, state)
        && let Some((tab_id, splitter)) =
            crate::features::sql_workspace::sql_tab::splitter::view::sql_tab_splitter_at(
                &state.sql.sql_tab,
                tab_area,
                x,
                y,
            )
    {
        return Some(SplitterDrag::Sql(tab_id, splitter));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app body geometry (top row, height) that mouse hit-testing is
    /// computed against, for a given terminal size.
    fn test_body(state: &AppState, size: ratatui::layout::Size) -> (u16, u16) {
        let body_top = 3u16;
        let body_h = size
            .height
            .saturating_sub(body_top)
            .saturating_sub(footer_layout::footer_height(&state.footer, size.width));
        (body_top, body_h)
    }

    /// The rect of the Explorer instances/objects horizontal splitter.
    fn test_explorer_splitter_rect(
        state: &AppState,
        size: ratatui::layout::Size,
        body_top: u16,
        body_h: u16,
    ) -> Rect {
        let explorer = app_explorer_rect(size, body_top, body_h, state).expect("explorer rect");
        let inner = Rect::new(
            explorer.x.saturating_add(1),
            explorer.y.saturating_add(1),
            explorer.width.saturating_sub(2),
            explorer.height.saturating_sub(2),
        );
        crate::features::explorer::splitter::layout::explorer_body_layout(
            inner,
            state.explorer.splitter.instances_height,
        )
        .splitter
    }

    /// One press arms **at most one** splitter. Regression guard: the drag
    /// targets used to be four independent booleans that were never reset on
    /// `Up`, so a drag of one splitter also resized every split dragged earlier
    /// (a vertical move changing a height, a horizontal move changing a width).
    #[test]
    fn resolve_splitter_drag_arms_exactly_one_target() {
        use crate::features::explorer::state::ExplorerPane;

        let mut state = AppState::default();
        state.term_width = 120;
        state.term_height = 40;
        // The Explorer owns focus, so its instances/objects splitter is
        // eligible — the app-level splitter must still win on its own column.
        state.focus = Pane::Explorer(ExplorerPane::Instances);
        let size = ratatui::layout::Size::new(120, 40);
        let (body_top, body_h) = test_body(&state, size);
        let layout = crate::features::app_splitter::layout::app_body_layout(
            Rect::new(0, body_top, size.width, body_h),
            state.splitter.explorer_pane_width,
        );

        // On the app-level vertical splitter: only the app splitter arms.
        assert_eq!(
            resolve_splitter_drag(&state, size, layout.v_splitter.x, layout.v_splitter.y + 5),
            Some(SplitterDrag::App)
        );

        // On the Explorer's horizontal splitter: only that one arms.
        let ex = test_explorer_splitter_rect(&state, size, body_top, body_h);
        assert_eq!(
            resolve_splitter_drag(&state, size, ex.x + 3, ex.y),
            Some(SplitterDrag::Explorer)
        );

        // Inside a pane (on no splitter at all): nothing arms.
        assert_eq!(resolve_splitter_drag(&state, size, 2, body_top + 1), None);
    }

    /// Every row of the app-level vertical splitter resolves to `App` —
    /// including the row the Explorer's horizontal splitter occupies. The two
    /// own different axes, so a vertical drag must never reach a height split.
    #[test]
    fn vertical_splitter_rows_never_resolve_to_a_height_split() {
        use crate::features::explorer::state::ExplorerPane;

        let mut state = AppState::default();
        state.term_width = 120;
        state.term_height = 40;
        state.focus = Pane::Explorer(ExplorerPane::Instances);
        let size = ratatui::layout::Size::new(120, 40);
        let (body_top, body_h) = test_body(&state, size);
        let layout = crate::features::app_splitter::layout::app_body_layout(
            Rect::new(0, body_top, size.width, body_h),
            state.splitter.explorer_pane_width,
        );
        for dy in 0..layout.v_splitter.height {
            assert_eq!(
                resolve_splitter_drag(&state, size, layout.v_splitter.x, layout.v_splitter.y + dy),
                Some(SplitterDrag::App),
                "row {dy} of the vertical splitter must not resolve to another split"
            );
        }
    }

    /// With the SQL workspace focused, a press on the editor/history splitter
    /// arms that splitter alone — not the app-level one — so a drag there cannot
    /// resize the Explorer width (the reported "horizontal drag moves the
    /// Explorer" symptom).
    #[test]
    fn sql_tab_splitter_arms_without_the_app_splitter() {
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;

        let mut state = AppState::default();
        state.term_width = 120;
        state.term_height = 40;
        state.focus = Pane::SQLWorkspace;
        state.sql.sql_tab.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            None,
            None,
            None,
        );
        state.sql.sql_tab.tabs[0].focus = SqlFocus::Editor;
        let size = ratatui::layout::Size::new(120, 40);
        let tab_area = sql_tab_area_for_hit(size, &state).expect("sql tab area");
        let body = Rect::new(
            tab_area.x,
            tab_area.y.saturating_add(1),
            tab_area.width,
            tab_area.height.saturating_sub(1),
        );
        let tab = &state.sql.sql_tab.tabs[0];
        let layout = crate::features::sql_workspace::sql_tab::layout::sql_tab_layout(
            body,
            tab.splitter.editor_top_height,
            tab.splitter.history_pane_width,
        );

        let got = resolve_splitter_drag(&state, size, layout.v_splitter.x, layout.v_splitter.y + 1);
        assert!(
            matches!(got, Some(SplitterDrag::Sql(_, _))),
            "a press on the SQL editor/history splitter must arm it, got {got:?}"
        );
        assert_ne!(
            got,
            Some(SplitterDrag::App),
            "the app-level width splitter must not arm alongside it"
        );
    }
}
