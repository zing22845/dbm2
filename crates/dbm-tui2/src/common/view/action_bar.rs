//! Results action bar: Refresh / Edit / Inst / Dup / Del / Commit / Rollback.
//!
//! Pure model + layout + drawing shared by the Results list. Theme-aware via
//! the semantic `Palette`: enabled buttons use [`Palette::available_button_style`]
//! (the header Discover look) and the active Edit button forces a red
//! foreground.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;

use super::theme::Palette;

pub const RESULTS_ACTION_BAR_HEIGHT: u16 = 1;

/// A discrete action offered by the Results toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultsAction {
    Refresh,
    Edit,
    Commit,
    Rollback,
    AddRow,
    DupRow,
    DelRow,
}

/// What the toolbar can enable/disable and how many edits are pending commit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultsToolbarModel {
    pub refresh_enabled: bool,
    pub edit_enabled: bool,
    /// True while Results is in Edit mode (`[EDIT]`) — styles the Edit action
    /// and gates the row mutations.
    pub edit_active: bool,
    pub commit_enabled: bool,
    pub rollback_enabled: bool,
    pub commit_n: usize,
    /// Why Edit is disabled (kept for status / future tooltip).
    #[allow(dead_code)]
    pub edit_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ActionButton {
    pub action: ResultsAction,
    pub label: String,
    pub enabled: bool,
}

const BTN_GAP: u16 = 1;

/// Buttons in display order: Refresh, Edit, Inst, Dup, Del, Commit, Rollback.
pub fn action_buttons(model: &ResultsToolbarModel) -> Vec<ActionButton> {
    vec![
        ActionButton {
            action: ResultsAction::Refresh,
            label: "[C-r]Refresh".into(),
            enabled: model.refresh_enabled,
        },
        ActionButton {
            action: ResultsAction::Edit,
            label: "[i]Edit".into(),
            enabled: model.edit_enabled,
        },
        ActionButton {
            action: ResultsAction::AddRow,
            label: "[C-i]Inst".into(),
            enabled: model.edit_active,
        },
        ActionButton {
            action: ResultsAction::DupRow,
            label: "[C-p]Dup".into(),
            enabled: model.edit_active,
        },
        ActionButton {
            action: ResultsAction::DelRow,
            label: "[dd]Del".into(),
            enabled: model.edit_active,
        },
        ActionButton {
            action: ResultsAction::Commit,
            label: format!("[C-s]Commit {}", model.commit_n),
            enabled: model.commit_enabled,
        },
        ActionButton {
            action: ResultsAction::Rollback,
            label: "[C-u]Rollback".into(),
            enabled: model.rollback_enabled,
        },
    ]
}

fn padded_label(label: &str) -> String {
    format!(" {label} ")
}

pub fn action_bar_width(model: &ResultsToolbarModel) -> u16 {
    let buttons = action_buttons(model);
    if buttons.is_empty() {
        return 0;
    }
    let mut total = 0u16;
    for (i, btn) in buttons.iter().enumerate() {
        total = total.saturating_add(padded_label(&btn.label).chars().count() as u16);
        if i + 1 < buttons.len() {
            total = total.saturating_add(BTN_GAP);
        }
    }
    total
}

/// A button's placed position in the wrapped action bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacedButton {
    pub action: ResultsAction,
    pub enabled: bool,
    /// Column offset from the action-bar area's left edge.
    pub x: u16,
    /// 0-based row inside the action bar (which is [`action_bar_height`] tall).
    pub row: u16,
    /// Padded label width in columns.
    pub width: u16,
}

/// Greedily lay the action buttons out in `width` columns, wrapping whole
/// buttons onto a new row whenever one no longer fits — a narrow Results pane
/// never clips or scrolls the toolbar, it simply uses more rows.
pub fn place_action_bar(model: &ResultsToolbarModel, width: u16) -> Vec<PlacedButton> {
    let buttons = action_buttons(model);
    let mut out = Vec::with_capacity(buttons.len());
    let mut row = 0u16;
    let mut used = 0u16;
    for btn in buttons {
        let w = padded_label(&btn.label).chars().count() as u16;
        if used > 0 && used + BTN_GAP + w > width.max(1) {
            row = row.saturating_add(1);
            used = 0;
        }
        let x = if used > 0 { used + BTN_GAP } else { used };
        out.push(PlacedButton {
            action: btn.action,
            enabled: btn.enabled,
            x,
            row,
            width: w,
        });
        used = x + w;
    }
    out
}

/// The number of rows the wrapped action bar occupies at `width` (at least 1).
pub fn action_bar_height(model: &ResultsToolbarModel, width: u16) -> u16 {
    place_action_bar(model, width)
        .iter()
        .map(|b| b.row)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

/// Map [`place_action_bar`] positions into screen rects inside `area`.
/// `h_scroll` is accepted for signature compatibility (the wrapped layout is
/// always fully visible, so it is unused).
pub fn layout_action_bar(
    area: Rect,
    model: &ResultsToolbarModel,
    _h_scroll: u16,
) -> Vec<(ResultsAction, Rect, bool)> {
    place_action_bar(model, area.width)
        .into_iter()
        .map(|b| {
            (
                b.action,
                Rect {
                    x: area.x.saturating_add(b.x),
                    y: area.y.saturating_add(b.row),
                    width: b.width,
                    height: 1,
                },
                b.enabled,
            )
        })
        .collect()
}

/// Enabled/disabled chrome shared with the Results action bar. Enabled buttons
/// use the palette's available-button style (the header Discover look); a
/// disabled button is unstyled.
pub fn action_button_style(enabled: bool, palette: &Palette) -> Style {
    if enabled {
        palette.available_button_style()
    } else {
        Style::default()
    }
}

pub fn draw_action_button_label(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    enabled: bool,
    palette: &Palette,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let text = padded_label(label);
    let visible: String = text.chars().take(area.width as usize).collect();
    frame.render_widget(
        Paragraph::new(visible).style(action_button_style(enabled, palette)),
        area,
    );
}

/// Enabled = the palette's available-button background (header Discover look);
/// disabled = default. Edit mode on keeps that background but forces a red
/// foreground on the Edit button.
fn button_style(
    action: ResultsAction,
    enabled: bool,
    edit_active: bool,
    palette: &Palette,
) -> Style {
    if !enabled {
        return Style::default();
    }
    let base = palette.available_button_style();
    if matches!(action, ResultsAction::Edit) && edit_active {
        base.fg(Color::Red)
    } else {
        base
    }
}

/// Draw the action bar, wrapping buttons across [`action_bar_height`] rows.
/// `area` must be sized with [`action_bar_height`] (the geometry helper
/// `results_list_regions` does this) so every row has room.
pub fn draw_action_bar(
    frame: &mut Frame,
    area: Rect,
    model: &ResultsToolbarModel,
    _h_scroll: u16,
    palette: &Palette,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(ratatui::widgets::Clear, area);

    let buttons = action_buttons(model);
    let places = place_action_bar(model, area.width);
    for (btn, place) in buttons.iter().zip(&places) {
        if place.row >= area.height || place.width == 0 {
            continue;
        }
        let left = area.x.saturating_add(place.x);
        let right = area.right().min(left.saturating_add(place.width));
        if right <= left {
            continue;
        }
        let visible: String = padded_label(&btn.label)
            .chars()
            .take((right - left) as usize)
            .collect();
        let style = button_style(btn.action, btn.enabled, model.edit_active, palette);
        frame.render_widget(
            Paragraph::new(visible).style(style),
            Rect {
                x: left,
                y: area.y.saturating_add(place.row),
                width: right - left,
                height: 1,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_model() -> ResultsToolbarModel {
        ResultsToolbarModel {
            refresh_enabled: true,
            edit_enabled: true,
            edit_active: true,
            commit_enabled: true,
            rollback_enabled: true,
            commit_n: 2,
            edit_reason: None,
        }
    }

    #[test]
    fn action_order_is_refresh_edit_inst_dup_del_commit_rollback() {
        let buttons = action_buttons(&sample_model());
        assert_eq!(
            buttons.iter().map(|b| b.action).collect::<Vec<_>>(),
            vec![
                ResultsAction::Refresh,
                ResultsAction::Edit,
                ResultsAction::AddRow,
                ResultsAction::DupRow,
                ResultsAction::DelRow,
                ResultsAction::Commit,
                ResultsAction::Rollback,
            ]
        );
        assert_eq!(buttons[2].label, "[C-i]Inst");
        assert_eq!(buttons[5].label, "[C-s]Commit 2");
    }

    #[test]
    fn action_bar_width_sums_padded_buttons_and_gaps() {
        let model = sample_model();
        let buttons = action_buttons(&model);
        let expected: u16 = buttons
            .iter()
            .map(|b| padded_label(&b.label).chars().count() as u16)
            .sum::<u16>()
            + BTN_GAP * (buttons.len() as u16 - 1);
        assert_eq!(action_bar_width(&model), expected);
    }

    #[test]
    fn layout_clips_with_h_scroll() {
        let model = sample_model();
        let area = Rect::new(0, 0, 20, 1);
        let full = layout_action_bar(area, &model, 0);
        assert!(!full.is_empty());
        assert_eq!(full[0].0, ResultsAction::Refresh);

        let scrolled = layout_action_bar(area, &model, 10);
        assert!(scrolled.iter().all(|(_, r, _)| r.x < area.x + area.width));
    }

    #[test]
    fn inst_dup_del_gated_by_edit_active() {
        let mut model = sample_model();
        model.edit_active = false;
        let buttons = action_buttons(&model);
        assert!(!buttons[2].enabled);
        assert!(!buttons[3].enabled);
        assert!(!buttons[4].enabled);
    }

    #[test]
    fn enabled_button_matches_the_header_discover_background() {
        let theme = crate::common::view::theme::default();
        let p = theme.palette();
        // Enabled chrome = the Discover available-button look (light selection
        // island); disabled chrome is unstyled.
        let enabled = action_button_style(true, p);
        assert_eq!(enabled.bg, Some(p.selection_bg));
        assert_eq!(enabled.fg, Some(p.selection_text));
        let disabled = action_button_style(false, p);
        assert_eq!(disabled.bg, None);
    }

    #[test]
    fn action_bar_wraps_onto_more_rows_when_narrow() {
        let model = sample_model();
        assert_eq!(
            action_bar_height(&model, 500),
            1,
            "a wide pane keeps the whole bar on one row"
        );
        let narrow_rows = action_bar_height(&model, 15);
        assert!(
            narrow_rows > 1,
            "a narrow pane must wrap buttons onto extra rows"
        );
        // Every button is still placed, and its row fits inside the reserved
        // height.
        let places = place_action_bar(&model, 15);
        assert_eq!(places.len(), action_buttons(&model).len());
        assert!(
            places.iter().all(|b| b.row < narrow_rows),
            "placed rows must fit the computed bar height"
        );
    }

    #[test]
    fn wrapped_layout_maps_to_rows_inside_the_area() {
        let model = sample_model();
        let height = action_bar_height(&model, 15);
        let area = Rect::new(2, 3, 15, height);
        let placed = layout_action_bar(area, &model, 0);
        assert!(!placed.is_empty());
        assert!(
            placed.iter().all(|(_, r, _)| {
                r.y >= area.y && r.y < area.y + area.height && r.x < area.right()
            }),
            "wrapped button rects must live inside the action-bar area"
        );
    }
}
