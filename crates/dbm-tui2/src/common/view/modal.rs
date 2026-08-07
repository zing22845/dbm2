//! Shared modal/popup rendering and footer hints.
//!
//! Provides a centered, bordered popup renderer plus the `ModalKind` footer
//! hints shared across the confirm/picker popups (row limit, page input,
//! delete/unregister confirm, edit-commit preview). The Discover modal draws
//! its own zone footer and leaves `modal_footer_text` empty.

use ratatui::Frame;
use ratatui::layout::Rect;
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
}
