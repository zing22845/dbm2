//! Shared modal/popup rendering and footer hints.
//!
//! Provides a centered, bordered popup renderer plus the `ModalKind` footer
//! hints shared across the confirm/picker popups (row limit, page input,
//! delete/unregister confirm, edit-commit preview). The Discover modal draws
//! its own footer and leaves `modal_footer_text` empty.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::text::{Line, Span};

use crate::app::state::ModalKind;
use crate::common::view::overlay_clear::clear_overlay;
use crate::common::view::theme::Theme;

/// Render a centered popup of `width_pct` × `height_pct` over `base`, clearing
/// the overlay so wide glyphs below don't bleed through, and delegating the
/// popup's interior (block + body) to `inner`.
///
/// When `dim_base` is `true`, the whole `base` is covered with an opaque
/// background before drawing the popup (modal focus). When `false`, only the
/// popup's own rectangle is cleared/filled, leaving the surrounding content
/// visible (used by the discover close-confirmation, which overlays the still
/// visible discover pane).
pub fn render_popup<F>(
    frame: &mut Frame,
    base: Rect,
    width_pct: u16,
    height_pct: u16,
    dim_base: bool,
    inner: F,
) where
    F: FnOnce(&mut Frame, Rect),
{
    if base.width == 0 || base.height == 0 {
        return;
    }
    let w = (base.width * width_pct) / 100;
    let h = (base.height * height_pct) / 100;
    if w < 2 || h < 2 {
        return;
    }
    let popup = Rect {
        x: base.x + (base.width - w) / 2,
        y: base.y + (base.height - h) / 2,
        width: w,
        height: h,
    };
    // Clear + fill either the whole modal region (dim_base) or only the popup's
    // own rect (leaving the surroundings visible). The `inner` renderer supplies
    // the popup's own block/surface background.
    let cover = if dim_base { base } else { popup };
    clear_overlay(frame, cover);
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Black)),
        cover,
    );
    inner(frame, popup);
}

/// Render a titled, bordered popup containing `body_lines` (no header row).
pub fn render_titled_popup(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    title: &str,
    body: Vec<Line<'static>>,
) {
    let p = theme.palette();
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.border_active))
        .style(Style::default().bg(p.surface));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(body), inner);
}

/// The footer hint line for a modal (data popup).
pub fn modal_footer_text(modal: Option<&ModalKind>) -> String {
    match modal {
        None => String::new(),
        Some(ModalKind::ResultsRowLimitPicker { .. }) => {
            "Select: ENTER · Move: j/k · Custom: c · Close: ESC".to_string()
        }
        Some(ModalKind::ResultsPageInput { .. }) => {
            "Go: ENTER · Close: ESC".to_string()
        }
        Some(ModalKind::DeleteConnectionConfirm { .. })
        | Some(ModalKind::UnregisterInstanceConfirm { .. })
        | Some(ModalKind::ResultsEditCommitPreview { .. }) => {
            "Confirm: y · Cancel: n/ESC".to_string()
        }
    }
}

/// Whether a `ModalKind` is a "confirm-style" popup (y/n prompt).
pub fn is_confirm_modal(modal: &ModalKind) -> bool {
    matches!(
        modal,
        ModalKind::DeleteConnectionConfirm { .. }
            | ModalKind::UnregisterInstanceConfirm { .. }
            | ModalKind::ResultsEditCommitPreview { .. }
    )
}

/// Title for a confirm/picker popup, used by popup rendering.
pub fn modal_title(modal: &ModalKind) -> String {
    match modal {
        ModalKind::ResultsRowLimitPicker { .. } => "Rows per page".to_string(),
        ModalKind::ResultsPageInput { .. } => "Jump to page".to_string(),
        ModalKind::DeleteConnectionConfirm { instance, connection } => {
            format!("Delete connection {connection} ({instance})?")
        }
        ModalKind::UnregisterInstanceConfirm { instance } => {
            format!("Unregister instance {instance}?")
        }
        ModalKind::ResultsEditCommitPreview { .. } => "Commit preview".to_string(),
    }
}

/// The Yes/No button rects of a confirm popup, used for both rendering and
/// mouse hit-testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConfirmButtons {
    pub yes_rect: Rect,
    pub no_rect: Rect,
}

/// The centered rect of a confirm popup over `area` (pure geometry). Shared by
/// the renderer and the mouse hit-tester so both agree on the popup position.
pub fn confirm_popup_rect(area: Rect) -> Rect {
    let popup_w = area.width.clamp(28, 44);
    let popup_h = 5u16;
    Rect {
        x: area.x.saturating_add(area.width.saturating_sub(popup_w) / 2),
        y: area.y.saturating_add(area.height.saturating_sub(popup_h) / 2),
        width: popup_w,
        height: popup_h.min(area.height),
    }
}

/// Compute the Yes/No button rects inside a confirm popup (pure layout).
pub fn confirm_buttons(popup: Rect) -> ConfirmButtons {
    let yes_label = " Yes ";
    let no_label = " No ";
    let gap = 3u16;
    let yes_w = yes_label.chars().count() as u16;
    let no_w = no_label.chars().count() as u16;
    let total = yes_w.saturating_add(gap).saturating_add(no_w);
    let start_x = popup
        .x
        .saturating_add(popup.width.saturating_sub(total) / 2);
    let yes_rect = Rect {
        x: start_x,
        y: popup.y.saturating_add(2), // block top(1) + one body row inside inner
        width: yes_w.min(popup.width),
        height: 1,
    };
    let no_rect = Rect {
        x: start_x.saturating_add(yes_w).saturating_add(gap),
        y: yes_rect.y,
        width: no_w,
        height: 1,
    };
    ConfirmButtons { yes_rect, no_rect }
}

/// Render a centered confirm popup: `title` in the border, `body` lines above,
/// and a Yes/No button row at the bottom (`highlight_yes` picks the emphasized
/// button). Returns the button rects so the caller can route mouse clicks.
/// Mirrors the original dbm's confirm dialogs (Yes/No buttons, surface+border).
pub fn render_confirm_popup(
    frame: &mut Frame,
    theme: &Theme,
    base: Rect,
    title: &str,
    body: Vec<Line<'static>>,
    highlight_yes: bool,
) -> ConfirmButtons {
    let p = theme.palette();
    let popup = confirm_popup_rect(base);
    if popup.width == 0 || popup.height == 0 {
        return ConfirmButtons::default();
    }

    clear_overlay(frame, popup);
    frame.render_widget(
        Block::default().style(Style::default().bg(p.surface)),
        popup,
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.border))
        .style(Style::default().bg(p.surface));
    let inner = block.inner(popup);
    frame.render_widget(&block, popup);
    if inner.width == 0 || inner.height < 2 {
        return confirm_buttons(popup);
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(body).style(Style::default().bg(p.surface)),
        chunks[0],
    );

    // Yes/No button row. Use the shared `confirm_buttons` geometry so rendering
    // and mouse hit-testing always agree.
    let buttons = confirm_buttons(popup);
    let yes_style = if highlight_yes {
        Style::default()
            .fg(p.selection)
            .add_modifier(ratatui::style::Modifier::BOLD)
    } else {
        Style::default().bg(p.surface)
    };
    let no_style = if highlight_yes {
        Style::default().bg(p.surface)
    } else {
        Style::default()
            .fg(p.selection)
            .add_modifier(ratatui::style::Modifier::BOLD)
    };
    frame.render_widget(Paragraph::new(" Yes ").style(yes_style), buttons.yes_rect);
    if buttons.no_rect.right() <= popup.right() {
        frame.render_widget(Paragraph::new(" No ").style(no_style), buttons.no_rect);
    }
    buttons
}

/// A convenience span builder so popup bodies read like the original hints.rs.
pub fn key(desc: &str, key_name: &str) -> String {
    format!("{desc}: {key_name}")
}

/// Join hint parts with a visible separator.
pub fn join(parts: &[&str]) -> String {
    parts.join("  ")
}

/// A muted footer hint line (simple `String`).
pub fn key_line(desc: &str, key_name: &str) -> Line<'static> {
    Line::from(Span::raw(key(desc, key_name)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Position;

    #[test]
    fn no_modal_has_empty_footer() {
        assert_eq!(modal_footer_text(None), "");
    }

    #[test]
    fn picker_and_confirm_have_expected_footers() {
        let picker = modal_footer_text(Some(&ModalKind::ResultsRowLimitPicker {
            current: 100,
            limits: vec![50, 100, 200],
        }));
        assert!(picker.contains("Select: ENTER"));
        assert!(picker.contains("Close: ESC"));

        let confirm = modal_footer_text(Some(&ModalKind::DeleteConnectionConfirm {
            instance: "i".into(),
            connection: "c".into(),
        }));
        assert!(confirm.contains("Confirm: y"));
        assert!(confirm.contains("Cancel: n/ESC"));
    }

    #[test]
    fn confirm_modal_classification() {
        assert!(is_confirm_modal(&ModalKind::DeleteConnectionConfirm {
            instance: "i".into(),
            connection: "c".into(),
        }));
        assert!(is_confirm_modal(&ModalKind::UnregisterInstanceConfirm {
            instance: "i".into(),
        }));
        assert!(is_confirm_modal(&ModalKind::ResultsEditCommitPreview {
            statements: Vec::new(),
        }));
        assert!(!is_confirm_modal(&ModalKind::ResultsRowLimitPicker {
            current: 1,
            limits: vec![1],
        }));
    }

    #[test]
    fn modal_titles_include_payload() {
        let t = modal_title(&ModalKind::DeleteConnectionConfirm {
            instance: "pg".into(),
            connection: "default".into(),
        });
        assert!(t.contains("default"));
        assert!(t.contains("pg"));
    }

    #[test]
    fn confirm_popup_is_centered_and_clamped() {
        let area = Rect::new(100, 0, 120, 30);
        let popup = confirm_popup_rect(area);
        // 28..=44 wide, 5 tall, horizontally centered.
        assert!(popup.width >= 28 && popup.width <= 44);
        assert_eq!(popup.height, 5);
        let left_gap = popup.x - area.x;
        let right_gap = area.right() - popup.right();
        assert_eq!(left_gap, right_gap);
    }

    #[test]
    fn confirm_buttons_are_side_by_side_centered() {
        let popup = Rect::new(30, 10, 40, 5);
        let b = confirm_buttons(popup);
        // Yes then No, on the same row, not overlapping.
        assert_eq!(b.yes_rect.y, b.no_rect.y);
        assert!(b.yes_rect.x < b.no_rect.x);
        assert!(b.no_rect.x >= b.yes_rect.right());
        // Centered: both fit within the popup width.
        assert!(b.yes_rect.x >= popup.x);
        assert!(b.no_rect.right() <= popup.right());
        assert!(b.yes_rect.contains(Position::new(b.yes_rect.x + 1, b.yes_rect.y)));
        assert!(b.no_rect.contains(Position::new(b.no_rect.x + 1, b.no_rect.y)));
    }
}
