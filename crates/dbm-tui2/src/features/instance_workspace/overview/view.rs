//! Instance overview feature rendering.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::common::utils::text_width::wrapped_line_count;
use crate::common::view::pane_scrollbar::{
    ActiveScrollbar, PaneScrollLayout, RowHeights, draw_vertical_pane_scrollbar, pane_anchor,
    pane_scroll_layout,
};
use crate::common::view::theme::Theme;
use dbm_store::ManagedInstance;

use super::state::OverviewState;

/// The overview rows shown for an instance, matching the original dbm: a list
/// of `(label, value)` pairs laid out as `"{label:20} {value}"`. The `value` is
/// `—` when the instance has no data for an optional field.
pub fn overview_rows(inst: &ManagedInstance, conn_count: usize) -> Vec<(String, String)> {
    let query_ready = if conn_count > 0 { "Ready" } else { "Degraded" };
    let scope = management_scope_labels(inst, conn_count);

    vec![
        ("Name".into(), inst.name.clone()),
        ("Engine".into(), inst.engine.to_string()),
        (
            "Version".into(),
            inst.version_short.clone().unwrap_or_else(|| "—".into()),
        ),
        (
            "Version (full)".into(),
            inst.version_full.clone().unwrap_or_else(|| "—".into()),
        ),
        (
            "Version checked".into(),
            inst.version_checked_at
                .clone()
                .unwrap_or_else(|| "—".into()),
        ),
        ("Instance ID".into(), inst.id.clone()),
        ("Fingerprint".into(), inst.fingerprint.clone()),
        ("Host".into(), inst.host.clone()),
        ("Port".into(), inst.port.to_string()),
        (
            "Socket".into(),
            inst.socket_path.clone().unwrap_or_else(|| "—".into()),
        ),
        (
            "Data directory".into(),
            inst.data_dir.clone().unwrap_or_else(|| "—".into()),
        ),
        (
            "Environment".into(),
            inst.env_label.clone().unwrap_or_else(|| "—".into()),
        ),
        ("Registered at".into(), inst.registered_at.clone()),
        ("Connections".into(), format!("{conn_count} registered")),
        ("Management scope".into(), scope),
        (
            "Lifecycle checked".into(),
            inst.lifecycle_checked_at
                .clone()
                .unwrap_or_else(|| "—".into()),
        ),
        ("Query readiness".into(), query_ready.into()),
    ]
}

/// The management scope labels for an instance, matching the original dbm:
/// lifecycle scope from the lifecycle status plus query/monitor/backup when the
/// instance has connections, or `—` when nothing applies.
fn management_scope_labels(inst: &ManagedInstance, conn_count: usize) -> String {
    let mut labels = Vec::new();
    match inst.lifecycle_status.as_deref() {
        Some("ready") => labels.push("lifecycle".to_string()),
        Some("degraded") => labels.push("lifecycle (degraded)".to_string()),
        _ => {}
    }
    if conn_count > 0 {
        labels.extend(["query".into(), "monitor".into(), "backup".into()]);
    }
    if labels.is_empty() {
        "—".into()
    } else {
        labels.join(", ")
    }
}

/// Result of [`compute_overview_viewport`]: shared by render and
/// v_scrollbar_hit so they agree on geometry and scroll position.
pub struct OverviewViewport {
    pub layout: PaneScrollLayout,
    pub content: Rect,
    pub viewport: usize,
    pub start: usize,
    pub total: usize,
    pub max_scroll: usize,
}

/// Shared viewport computation. Applies discover-style cursor anchoring
/// (skipped when `scroll_locked`).
///
/// Overview rows render with `Paragraph::wrap`, so a single data row can span
/// several pixel lines. The scroll math must therefore be **pixel-aware**: we
/// measure each row's wrapped height and keep the cursor's pixel span inside
/// the content area. Using a naive data-row `viewport` (= content height) would
/// over-count visible rows and let the cursor drift below the visible region
/// once any in-view row wraps. `state.scroll` / `state.cursor` stay in data-row
/// units (consumed by the scrollbar drag and keyboard nav), but the anchored
/// `start` and the rendered `viewport` are derived from per-row pixel heights.
pub fn compute_overview_viewport(
    area: Rect,
    state: &OverviewState,
    conn_count: usize,
) -> Option<OverviewViewport> {
    let inst = state.instance.as_ref()?;
    let rows = overview_rows(inst, conn_count);
    let total = rows.len();
    if total == 0 {
        return None;
    }

    // pane_scroll_layout reserves 1 col for the v_scrollbar when the data rows
    // overflow the area height. The v_scrollbar decision only depends on row
    // count vs. height, so the 80-col content-width estimate is fine here.
    let content_width_est = 80u16;
    let layout = pane_scroll_layout(area, content_width_est, total, area.height.max(1) as usize);
    let content = layout.content_area;
    // Real content width (after the scrollbar column is reserved) for accurate
    // wrapping — matches what `row_at` and the renderer's Paragraph use.
    let width = content.width.max(1);
    let content_height = content.height.max(1) as usize;

    // Per-row wrapped pixel heights (terminal rows each data row occupies). The
    // unified `pane_anchor` is height-aware: a wrapped row is counted by its
    // true extent and the cursor never drifts below the visible region, while
    // the window always aligns to whole logical rows (no row split across the
    // top/bottom edge).
    let heights: Vec<usize> = rows
        .iter()
        .map(|(label, value)| wrapped_line_count(&format!("{label:20} {value}"), width) as usize)
        .collect();

    let anchor = pane_anchor(
        total,
        RowHeights::Variable(&heights),
        content_height,
        state.scroll,
        state.cursor,
        state.scroll_locked,
    );

    Some(OverviewViewport {
        layout,
        content,
        viewport: anchor.visible,
        start: anchor.start,
        total,
        max_scroll: anchor.max_scroll,
    })
}

use crate::common::view::pane_scrollbar::ScrollbarHitInfo;

/// Hit-test the overview pane's vertical scrollbar — delegates to shared helper.
/// Takes `conn_count` because `compute_overview_viewport` needs it to build rows.
pub fn v_scrollbar_hit(
    area: Rect,
    state: &OverviewState,
    conn_count: usize,
    x: u16,
    y: u16,
) -> Option<ScrollbarHitInfo> {
    let ov = compute_overview_viewport(area, state, conn_count)?;
    crate::common::view::pane_scrollbar::v_scrollbar_hit(&ov.layout, ov.max_scroll, x, y)
}

/// Map a click Y coordinate (in the overview pane's `area`) to a data row index.
///
/// Rows render with `Paragraph::wrap`, so a long row can occupy more than one
/// pixel line. Unlike the connections list (which draws one row per pixel line),
/// a linear `row = start + rel` mapping would drift after any wrapped row. We
/// instead walk the visible data rows accumulating each row's wrapped pixel
/// height (via the same `wrapped_line_count` the renderer's Paragraph uses) so
/// the clicked pixel maps to exactly the highlighted data row. Returns `None`
/// for clicks on the scrollbar gutter or in the blank area below the data rows.
/// `conn_count` only affects row text, not the row count, so callers may pass
/// `0` for hit-testing geometry.
pub fn row_at(area: Rect, state: &OverviewState, conn_count: usize, y: u16) -> Option<usize> {
    let ov = compute_overview_viewport(area, state, conn_count)?;
    let content = ov.content;
    if y < content.y || y >= content.y.saturating_add(content.height) {
        return None;
    }
    let rel = usize::from(y.saturating_sub(content.y));
    let inst = state.instance.as_ref()?;
    let rows = overview_rows(inst, conn_count);
    let width = content.width.max(1);

    let mut pixel_top = 0usize;
    for (idx, (label, value)) in rows.iter().enumerate().skip(ov.start) {
        let h = wrapped_line_count(&format!("{label:20} {value}"), width) as usize;
        if rel < pixel_top + h {
            return Some(idx);
        }
        pixel_top += h;
        // Stop at the wrapped content fitting the visible height.
        if pixel_top >= content.height as usize {
            break;
        }
    }
    None
}

/// Render the instance overview body: the instance's attribute rows laid out
/// like the original dbm (`label:20 value`), with the cursor row highlighted.
/// Long rows wrap onto the next line when the pane is too narrow, so no
/// horizontal scrolling is needed. Drawn inside the workspace's single outer
/// border (the tab bar and pane footer are rendered by the instance-workspace
/// parent), so no border or footer is drawn here.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &OverviewState,
    conn_count: usize,
    _focused: bool,
    active_scrollbar: Option<ActiveScrollbar>,
) {
    let p = theme.palette();

    let Some(inst) = &state.instance else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "(select an instance in the explorer)",
                Style::default().fg(p.muted),
            ))),
            area,
        );
        return;
    };

    let ov = match compute_overview_viewport(area, state, conn_count) {
        Some(v) => v,
        None => return,
    };

    let rows = overview_rows(inst, conn_count);
    let content = ov.content;
    let start = ov.start;

    let mut lines = Vec::new();
    for (idx, (label, value)) in rows.iter().enumerate().skip(start).take(ov.viewport) {
        let selected = idx == state.cursor;
        let style = if selected {
            Style::default()
                .fg(p.selection_text)
                .bg(p.selection_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg)
        };
        let text = format!("{label:20} {value}");
        lines.push(Line::from(Span::styled(text, style)));
    }

    // Wrap long rows so a narrow pane shows the full value instead of cropping
    // it; the wrapped continuation keeps the same style as the row.
    frame.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
        content,
    );

    // ---- VERTICAL SCROLLBAR ----
    if let Some(bar) = ov.layout.v_scrollbar {
        draw_vertical_pane_scrollbar(
            frame,
            bar,
            ov.start,
            ov.viewport,
            ov.max_scroll,
            p,
            matches!(active_scrollbar, Some(ActiveScrollbar::OverviewV)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbm_core::Engine;

    fn inst() -> ManagedInstance {
        ManagedInstance {
            id: "id-1".into(),
            fingerprint: "fp-1".into(),
            name: "postgres".into(),
            engine: Engine::Postgres,
            host: "127.0.0.1".into(),
            port: 5432,
            socket_path: Some("/tmp/pg.sock".into()),
            data_dir: Some("/var/lib/pg".into()),
            env_label: Some("prod".into()),
            registered_at: "2026-01-01".into(),
            version_full: Some("17.2".into()),
            version_short: Some("17".into()),
            version_checked_at: Some("2026-01-02".into()),
            lifecycle_status: Some("ready".into()),
            lifecycle_checked_at: Some("2026-01-03".into()),
            lifecycle_detail: None,
        }
    }

    #[test]
    fn overview_rows_cover_original_fields() {
        let rows = overview_rows(&inst(), 3);
        let labels: Vec<&str> = rows.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Name",
                "Engine",
                "Version",
                "Version (full)",
                "Version checked",
                "Instance ID",
                "Fingerprint",
                "Host",
                "Port",
                "Socket",
                "Data directory",
                "Environment",
                "Registered at",
                "Connections",
                "Management scope",
                "Lifecycle checked",
                "Query readiness",
            ]
        );
        assert_eq!(rows[13].1, "3 registered");
        assert_eq!(rows[16].1, "Ready");
        assert!(rows[14].1.contains("lifecycle"));
        assert!(rows[14].1.contains("query"));
    }

    #[test]
    fn overview_rows_no_connections_is_degraded_with_dash() {
        let rows = overview_rows(&inst(), 0);
        assert_eq!(rows[13].1, "0 registered");
        assert_eq!(rows[16].1, "Degraded");
        assert_eq!(rows[14].1, "lifecycle");
    }

    #[test]
    fn overview_rows_optional_fields_fall_back_to_dash() {
        let mut i = inst();
        i.version_short = None;
        i.socket_path = None;
        i.lifecycle_status = None;
        let rows = overview_rows(&i, 0);
        assert_eq!(rows[2].1, "—");
        assert_eq!(rows[9].1, "—");
        assert_eq!(rows[14].1, "—");
    }

    #[test]
    fn lifecycle_change_repaints_nonzero_cells() {
        use crate::common::view::theme;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let theme = theme::default();
        let area = Rect::new(0, 0, 60, 20);
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();

        let mut a = inst();
        a.lifecycle_checked_at = Some("2026-01-03 10:00:00".into());
        let s1 = OverviewState {
            instance: Some(a.clone()),
            ..Default::default()
        };
        terminal
            .draw(|f| render(f, &theme, area, &s1, 0, true, None))
            .unwrap();
        let buf1 = terminal.backend().buffer().clone();

        a.lifecycle_checked_at = Some("2026-01-03 10:00:01".into());
        let s2 = OverviewState {
            instance: Some(a),
            ..Default::default()
        };
        terminal
            .draw(|f| render(f, &theme, area, &s2, 0, true, None))
            .unwrap();
        let buf2 = terminal.backend().buffer().clone();

        let changed = buf1
            .content()
            .iter()
            .zip(buf2.content().iter())
            .filter(|(a, b)| a.symbol() != b.symbol())
            .count();
        assert!(
            changed > 0,
            "lifecycle change must repaint cells, got {changed}"
        );
    }

    #[test]
    fn narrow_width_wraps_long_rows_instead_of_cropping() {
        use crate::common::view::theme;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let theme = theme::default();
        let area = Rect::new(0, 0, 16, 30);
        let mut terminal = Terminal::new(TestBackend::new(16, 30)).unwrap();

        let mut a = inst();
        a.version_full = Some("PostgreSQL 17.2 on x86_64-pc-linux-gnu, compiled by gcc".into());
        let state = OverviewState {
            instance: Some(a),
            ..Default::default()
        };
        terminal
            .draw(|f| render(f, &theme, area, &state, 0, true, None))
            .unwrap();
        let buf = terminal.backend().buffer().clone();

        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(
            text.contains("x86_64-pc-linux-gnu"),
            "long value should wrap into view, buffer contains: {text:?}"
        );
    }

    // ---- New v_scrollbar tests ----

    #[test]
    fn viewport_anchors_cursor_and_respects_scroll_locked() {
        let mut s = OverviewState {
            instance: Some(inst()),
            cursor: 16, // last row out of 17
            scroll: 0,
            scroll_locked: false,
            ..Default::default()
        };
        let area = Rect::new(0, 0, 80, 6); // v_scrollbar reserves 1 col → 59 rows wide? No, scrollbar only on overflow
        // area.height=6, rows=17 → overflow → v_scrollbar → content.height=5
        let ov = compute_overview_viewport(area, &s, 0).expect("some viewport");
        assert_eq!(
            ov.start, 12,
            "anchor: cursor=16, viewport=5 -> start=16-5+1"
        );

        // scroll_locked skips anchor.
        s.scroll_locked = true;
        s.scroll = 5;
        s.cursor = 0;
        let ov = compute_overview_viewport(area, &s, 0).expect("some viewport");
        assert_eq!(
            ov.start, 5,
            "scroll_locked keeps manual scroll even if cursor is at top"
        );
    }

    #[test]
    fn v_scrollbar_hit_returns_none_when_no_scroll_needed() {
        let s = OverviewState {
            instance: Some(inst()),
            cursor: 0,
            ..Default::default()
        };
        let area = Rect::new(0, 0, 80, 50); // plenty of height for all 17 rows
        let hit = v_scrollbar_hit(area, &s, 0, 79, 5);
        assert!(hit.is_none());
    }

    #[test]
    fn row_at_maps_click_to_data_row_and_respects_scroll() {
        // Scroll to start=5 so clicking the visible top row maps to data row 5
        // (not 0), and a click far below the rendered rows is None.
        let s = OverviewState {
            instance: Some(inst()),
            cursor: 0,
            scroll: 5,
            scroll_locked: true,
            ..Default::default()
        };
        let area = Rect::new(0, 0, 80, 10);
        let ov = compute_overview_viewport(area, &s, 0).expect("some viewport");
        let top = ov.content.y;
        // Click on the first visible data row must select start (scroll=5).
        assert_eq!(row_at(area, &s, 0, top), Some(5));
        // Click on the second visible row -> start + 1.
        assert_eq!(row_at(area, &s, 0, top + 1), Some(6));
        // A click well below the rendered rows (data row >= total=17) -> None.
        let beyond = top + (ov.total - ov.start) as u16 + 2;
        assert!(row_at(area, &s, 0, beyond).is_none());
        // No instance loaded -> None.
        let empty = OverviewState::default();
        assert!(row_at(area, &empty, 0, top).is_none());
    }

    #[test]
    fn row_at_selects_top_data_row_when_scrollbar_present() {
        // With more rows than height a v_scrollbar appears; the top data row is
        // still selectable and maps to row 0 at the scroll start.
        let s = OverviewState {
            instance: Some(inst()),
            cursor: 0,
            ..Default::default()
        };
        let area = Rect::new(0, 0, 80, 5); // 17 rows overflow 5 rows
        let ov = compute_overview_viewport(area, &s, 0).unwrap();
        assert!(ov.layout.v_scrollbar.is_some());
        let row_y = ov.content.y;
        assert_eq!(row_at(area, &s, 0, row_y), Some(0));
    }

    #[test]
    fn row_at_accounts_for_wrapped_rows() {
        // A narrow pane makes a long value (e.g. Version full) wrap onto two
        // pixel lines. row_at must return the data row under the cursor, not a
        // linear pixel->row mapping that drifts by one after the wrapped row.
        let mut a = inst();
        a.version_full =
            Some("PostgreSQL 17.2 on x86_64-pc-linux-gnu, compiled by gcc x86_64".into());
        let s = OverviewState {
            instance: Some(a),
            cursor: 0,
            ..Default::default()
        };
        let area = Rect::new(0, 0, 40, 25); // narrow enough that Version full wraps
        let ov = compute_overview_viewport(area, &s, 0).expect("some viewport");
        let top = ov.content.y;

        // Version full is data row 3. With wrapping it spans pixel lines; a click
        // on its first wrapped line must still map to data row 3.
        // Compute where row 3 actually begins in pixels.
        let rows = overview_rows(s.instance.as_ref().unwrap(), 0);
        let width = ov.content.width;
        let mut pixel = 0usize;
        let mut row3_pixel = None;
        for (idx, (label, value)) in rows.iter().enumerate() {
            if idx == 3 {
                row3_pixel = Some(pixel);
                break;
            }
            pixel += wrapped_line_count(&format!("{label:20} {value}"), width) as usize;
        }
        let row3_px = row3_pixel.expect("row 3 found");
        let row3_h = wrapped_line_count(&format!("{:20} {}", rows[3].0, rows[3].1), width) as usize;
        assert!(row3_h >= 2, "row 3 should wrap, got {row3_h} lines");

        // Click on the (now wrapped) Version full row maps to data row 3, and a
        // click on the following first pixel line (row 4's top) maps to row 4.
        assert_eq!(row_at(area, &s, 0, top + row3_px as u16), Some(3));
        // The very next pixel line is still within row 3's wrapped span.
        if row3_h > 1 {
            assert_eq!(row_at(area, &s, 0, top + (row3_px + 1) as u16), Some(3));
        }
        // The pixel line right after row 3's wrapped span is row 4.
        assert_eq!(
            row_at(area, &s, 0, top + (row3_px + row3_h) as u16),
            Some(4)
        );
    }

    #[test]
    fn cursor_stays_within_pixel_viewport_when_rows_wrap() {
        // Regression: with a wrapped row AND a vertical scrollbar (short pane),
        // the anchored `start` must keep the cursor's *pixel* span inside the
        // content area, not just its data-row index. The old data-row `viewport`
        // over-counted visible rows and let the cursor drift below the visible
        // region once an in-view row wrapped.
        let mut a = inst();
        a.version_full =
            Some("PostgreSQL 17.2 on x86_64-pc-linux-gnu, compiled by gcc x86_64-64".into());
        let total = overview_rows(&a, 0).len();
        let area = Rect::new(0, 0, 30, 6); // narrow (wraps) + short (v_scrollbar)

        for cursor in 0..total {
            let s = OverviewState {
                instance: Some(a.clone()),
                cursor,
                ..Default::default()
            };
            let ov = compute_overview_viewport(area, &s, 0).expect("some viewport");
            assert!(
                ov.layout.v_scrollbar.is_some(),
                "cursor={cursor}: pane should show a v_scrollbar"
            );
            // Cursor must be within the rendered data-row window.
            assert!(
                cursor >= ov.start && cursor < ov.start + ov.viewport,
                "cursor={cursor}: cursor {cursor} outside rendered window [{}, {})",
                ov.start,
                ov.start + ov.viewport
            );
            // Cursor's pixel span must fit inside the content area height.
            let rows = overview_rows(s.instance.as_ref().unwrap(), 0);
            let width = ov.content.width;
            let mut cum = vec![0usize; rows.len() + 1];
            for (i, (label, value)) in rows.iter().enumerate() {
                cum[i + 1] =
                    cum[i] + wrapped_line_count(&format!("{label:20} {value}"), width) as usize;
            }
            let content_h = ov.content.height as usize;
            assert!(
                cum[cursor] >= cum[ov.start] && cum[cursor + 1] <= cum[ov.start] + content_h,
                "cursor={cursor}: cursor pixel span [{}, {}] exceeds visible window [{}, {}]",
                cum[cursor],
                cum[cursor + 1],
                cum[ov.start],
                cum[ov.start] + content_h
            );
        }
    }
}
