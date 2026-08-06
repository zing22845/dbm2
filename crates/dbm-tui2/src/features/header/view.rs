//! Header feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Style, Modifier};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::HeaderState;

/// The clickable area of the `Discover` button within the header. The header
/// block has `Borders::ALL`, so the interior starts one row/col in and the
/// button is the `" Discover "` cell (one col further in past the leading pad).
/// Mirrored by `render` so hit-testing and drawing agree. Returns `None` when
/// the header is too narrow to show the button.
pub fn discover_button_rect(area: Rect) -> Option<Rect> {
    if area.width < 12 || area.height < 2 {
        return None;
    }
    Some(Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: 10,
        height: 1,
    })
}

/// Render the header: a bordered app title bar with an action-button row.
///
/// The header currently has a single `Discover` button; when it is the focused
/// button it is highlighted with the accent/selection slot. The button's
/// clickable rect is recorded on `state` so mouse hit-testing uses exactly the
/// rect that was drawn (mirrors the original `header_button_rects` in
/// `ui_layout`).
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &mut HeaderState) {
    let p = theme.palette();
    state.discover_button_rect = discover_button_rect(area);
    tracing::debug!(
        rect = ?state.discover_button_rect,
        header_area = ?area,
        "header button rect recorded during render"
    );

    // The `Discover` button is focused when the header cursor points at it.
    let discover_focused = state.button == 0;
    let discover_style = if discover_focused {
        Style::default()
            .fg(p.selection)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(p.fg)
    };

    let line = Line::from(vec![
        Span::raw(" "),
        Span::styled(" Discover ", discover_style),
        Span::raw("  "),
        Span::styled("ENTER: activate", Style::default().fg(p.muted)),
    ]);

    let block = Block::default()
        .title(" dbm ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.border_active));
    frame.render_widget(Paragraph::new(line).block(block), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_button_rect_is_inside_the_bordered_interior() {
        let area = Rect::new(0, 0, 40, 3);
        let r = discover_button_rect(area).expect("header wide enough");
        assert_eq!(r.y, 1); // one row in past the top border
        assert_eq!(r.width, 10);
        assert_eq!(r.x, 2); // one col in past the border + leading pad
        assert!(r.right() < area.right());
    }

    #[test]
    fn discover_button_rect_none_when_too_narrow() {
        assert_eq!(discover_button_rect(Rect::new(0, 0, 8, 3)), None);
        assert_eq!(discover_button_rect(Rect::new(0, 0, 40, 1)), None);
    }

    #[test]
    fn discover_button_rect_respects_area_origin() {
        let area = Rect::new(5, 2, 40, 3);
        let r = discover_button_rect(area).unwrap();
        assert_eq!(r.x, 7);
        assert_eq!(r.y, 3);
    }

    #[test]
    fn rendered_discover_text_matches_button_rect() {
        // Render the header and confirm the literal "Discover" glyphs land
        // exactly inside the recorded clickable rect (so clicking the drawn
        // button activates it).
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let area = Rect::new(0, 0, 40, 3);
        let mut terminal = Terminal::new(TestBackend::new(40, 3)).unwrap();
        let mut state = HeaderState::default();
        let theme = crate::common::view::theme::dracula();
        terminal
            .draw(|frame| render(frame, &theme, area, &mut state))
            .unwrap();
        let buf = terminal.backend().buffer();
        let r = state.discover_button_rect.expect("button drawn");
        // The whole "Discover" glyphs should be inside the rect.
        let text: String = (r.x..r.right())
            .map(|x| {
                buf[(x, r.y)].symbol().chars().next().unwrap_or(' ')
            })
            .collect();
        assert!(text.contains("Discover"), "got: {text:?}");
        // The drawn rect must be at the header's interior.
        assert_eq!(r.y, 1);
        assert_eq!(r.x, 2);
    }
}
