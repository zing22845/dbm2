//! Results action bar: Refresh / Edit / Inst / Dup / Del / Commit / Rollback.
//!
//! Pure model + layout + drawing shared by the Results list. Theme-aware via
//! the semantic `Palette`: enabled buttons use the `selection` slot and the
//! active Edit button forces a red foreground.

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

/// Lay out action buttons in `area`, offset by `h_scroll` (shared with the results grid).
pub fn layout_action_bar(
    area: Rect,
    model: &ResultsToolbarModel,
    h_scroll: u16,
) -> Vec<(ResultsAction, Rect, bool)> {
    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    let buttons = action_buttons(model);
    let mut out = Vec::with_capacity(buttons.len());
    let mut virt_x = area.x as i32 - i32::from(h_scroll);
    let area_left = i32::from(area.x);
    let area_right = area_left + i32::from(area.width);
    for (i, btn) in buttons.iter().enumerate() {
        let text = padded_label(&btn.label);
        let w = text.chars().count() as i32;
        let left = virt_x.max(area_left);
        let right = (virt_x + w).min(area_right);
        if right > left {
            out.push((
                btn.action,
                Rect {
                    x: left as u16,
                    y: area.y,
                    width: (right - left) as u16,
                    height: 1,
                },
                btn.enabled,
            ));
        }
        virt_x += w;
        if i + 1 < buttons.len() {
            virt_x += i32::from(BTN_GAP);
        }
    }
    out
}

/// Enabled/disabled chrome shared with the Results action bar.
pub fn action_button_style(enabled: bool, palette: &Palette) -> Style {
    if enabled {
        Style::default().bg(palette.selection)
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

/// Enabled = selection background; disabled = default. Edit mode on keeps the
/// selection background but forces a red foreground on the Edit button.
fn button_style(action: ResultsAction, enabled: bool, edit_active: bool, palette: &Palette) -> Style {
    if !enabled {
        return Style::default();
    }
    let base = Style::default().bg(palette.selection);
    if matches!(action, ResultsAction::Edit) && edit_active {
        base.fg(Color::Red)
    } else {
        base
    }
}

pub fn draw_action_bar(
    frame: &mut Frame,
    area: Rect,
    model: &ResultsToolbarModel,
    h_scroll: u16,
    palette: &Palette,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(Paragraph::new(" "), area);

    let buttons = action_buttons(model);
    let mut virt_x = area.x as i32 - i32::from(h_scroll);
    let area_left = i32::from(area.x);
    let area_right = area_left + i32::from(area.width);

    for (i, btn) in buttons.iter().enumerate() {
        let text = padded_label(&btn.label);
        let w = text.chars().count() as i32;
        let left = virt_x.max(area_left);
        let right = (virt_x + w).min(area_right);
        if right > left {
            let skip = (left - virt_x) as usize;
            let take = (right - left) as usize;
            let visible: String = text.chars().skip(skip).take(take).collect();
            let style = button_style(btn.action, btn.enabled, model.edit_active, palette);
            frame.render_widget(
                Paragraph::new(visible).style(style),
                Rect {
                    x: left as u16,
                    y: area.y,
                    width: take as u16,
                    height: 1,
                },
            );
        }
        virt_x += w;
        if i + 1 < buttons.len() {
            virt_x += i32::from(BTN_GAP);
        }
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
}
