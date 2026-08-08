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
/// button it is highlighted with the accent/selection slot. This is a pure
/// `state -> view` function: it never mutates state (mouse hit-testing calls
/// the standalone [`discover_button_rect`], not a field written here).
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &HeaderState, focused: bool) {
    let p = theme.palette();

    // The `Discover` button is focused when the header cursor points at it.
    let discover_focused = state.button == 0;
    let discover_style = if discover_focused {
        Style::default()
            .fg(p.selection)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(p.fg)
    };

    // Draw the bordered title bar (no text: the button and hint are placed
    // explicitly below, using the same single source of truth as hit-testing).
    // The border is highlighted only when the header owns the shell focus.
    let border = if focused { p.border_active } else { p.border };
    let block = Block::default()
        .title(" dbm ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border));
    frame.render_widget(block, area);

    // The button glyphs come from `discover_button_rect`, the same rect used
    // for mouse hit-testing — render and hit-test can never drift apart.
    let button_text = " Discover ";
    if let Some(rect) = discover_button_rect(area) {
        let visible: String = button_text
            .chars()
            .take(rect.width as usize)
            .collect();
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(visible, discover_style))),
            rect,
        );
        // A short hint to the right of the button.
        let hint_rect = Rect {
            x: rect.right().saturating_add(1),
            y: rect.y,
            width: area.right().saturating_sub(rect.right().saturating_add(1)),
            height: 1,
        };
        if hint_rect.width >= "ENTER: activate".len() as u16 {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "ENTER: activate",
                    Style::default().fg(p.muted),
                ))),
                hint_rect,
            );
        }
    }
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
        // exactly inside the clickable rect (so clicking the drawn button
        // activates it). The view stays a pure `state -> view` function.
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let area = Rect::new(0, 0, 40, 3);
        let mut terminal = Terminal::new(TestBackend::new(40, 3)).unwrap();
        let state = HeaderState::default();
        let theme = crate::common::view::theme::dracula();
        terminal
            .draw(|frame| render(frame, &theme, area, &state, false))
            .unwrap();
        let buf = terminal.backend().buffer();
        let r = discover_button_rect(area).expect("button drawn");
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

        // The "ENTER: activate" hint must render just to the right of the button.
        let hint: String = (r.right().saturating_add(1)..buf.area().right())
            .map(|x| {
                buf[(x, r.y)].symbol().chars().next().unwrap_or(' ')
            })
            .collect();
        assert!(hint.contains("ENTER: activate"), "got: {hint:?}");
    }
}
