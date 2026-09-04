//! Semantic theming.
//!
//! `Palette` holds every color the UI needs as a **semantic slot** (e.g.
//! `accent`, `error`, `muted`) — features reference slots, never concrete
//! `Color`s, so recoloring is centralized and a whole theme can be swapped by
//! switching the current palette.
//!
//! `Theme` wraps a `dark` and a `light` palette plus a mode flag; a theme is
//! passed to every `view::render` as a read-only rendering context (see the
//! `common/view` convention), which keeps feature views self-contained pure
//! functions: pass a different theme to verify different colors.

use ratatui::style::{Color, Modifier, Style};

/// The semantic color slots available to every view.
///
/// Add slots here (e.g. an extra emphasis color) without touching feature
/// views — they only reference these names.
#[derive(Debug, Clone)]
pub struct Palette {
    /// Primary foreground text color.
    pub fg: Color,
    /// Dimmed / secondary text color.
    pub fg_dim: Color,
    /// Background color.
    pub bg: Color,
    /// Slightly different background used to surface panels/groups.
    pub surface: Color,
    /// Accent color for the active element (selection, cursor, focus border).
    pub accent: Color,
    /// Border color of regular panels.
    pub border: Color,
    /// Border color of the focused/active panel.
    pub border_active: Color,
    /// Dedicated foreground color for the *active* workspace marker in the
    /// explorer tree (the node whose workspace is currently shown). Kept
    /// separate from `accent`/`border_active` so it has its own color and is
    /// chosen to not clash with `selection_bg` (the active row may also be the
    /// cursor row, which layers a selection highlight underneath).
    pub active_fg: Color,
    /// Highlighted list selection (foreground emphasis).
    pub selection: Color,
    /// The background color used to highlight the cursor-selected row in lists
    /// and trees. Kept separate from `selection` so every pane can render the
    /// selected row with a unified, visible background (mirroring the original
    /// dbm's connections selection) instead of relying on foreground alone.
    pub selection_bg: Color,
    /// A stronger background for the active cell inside an already-selected
    /// row (e.g. the cursor cell of a SQL results grid), so the focused field
    /// stands out from its row-mates which share `selection_bg`.
    pub selection_cell_bg: Color,
    /// Foreground color for text in a selected row (non-active cells).
    /// Mirrors the original dbm's `selection_text` — dark grey on a light selection
    /// background so the selected-row label stays legible.
    pub selection_text: Color,
    /// Foreground color for the focused / active cell inside a selected row.
    /// Mirrors the original dbm's `selection_focus_text` — a strong accent (e.g. dark
    /// blue) on the `selection_cell_bg` background to mark the cursor cell.
    pub selection_focus_text: Color,
    /// Success / positive status.
    pub success: Color,
    /// Warning status.
    pub warning: Color,
    /// Error / destructive status.
    pub error: Color,
    /// Informational status.
    pub info: Color,
    /// Muted / de-emphasized text (e.g. footer hints, placeholders).
    pub muted: Color,
}

impl Palette {
    /// The border style for a pane: the active (accent) color when the pane is
    /// on the focus chain, otherwise the regular border color. Pane views use
    /// this instead of hand-rolling `if focused { border_active } else { border }`
    /// so focus highlighting is defined in one place.
    pub fn active_border(&self, active: bool) -> Style {
        Style::default().fg(if active { self.border_active } else { self.border })
    }

    /// Style of a (non-current) search match's text — the uniform accent used
    /// for all hits. Matches the original dbm's `other_match_style`: yellow
    /// foreground with no background. Current vs. other matches are
    /// distinguished differently per pane: the results list frames the current
    /// cell (via `match_cell_border_style`), while the editor fills the current
    /// match's background via [`Self::current_match_style`].
    pub fn match_style(&self) -> Style {
        Style::default().fg(self.accent)
    }

    /// Style of the *current* search match — matches the original dbm's
    /// `current_match_style`: black bold text on a yellow background. Used by
    /// the editor, which has no cell frame to point at the current match.
    pub fn current_match_style(&self) -> Style {
        Style::default()
            .fg(Color::Black)
            .bg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    /// Style of an active search input's text/prompt.
    pub fn search_active_style(&self) -> Style {
        Style::default().fg(self.accent)
    }

    /// Style of the frame drawn around the current-match cell: bold accent so
    /// it stands out from the muted grid. This is the sole "current" indicator
    /// in the results list — the matched text itself uses [`Self::match_style`].
    pub fn match_cell_border_style(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }
}

/// A theme: a `dark` and a `light` palette plus the current mode flag.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Human-readable theme name (for persistence / display).
    pub name: &'static str,
    /// Whether the `dark` palette is currently active.
    pub is_dark: bool,
    dark: Palette,
    light: Palette,
}

impl Theme {
    /// The palette active under the current mode.
    pub fn palette(&self) -> &Palette {
        if self.is_dark {
            &self.dark
        } else {
            &self.light
        }
    }

    /// Toggle between the dark and light palettes.
    pub fn toggle(&mut self) {
        self.is_dark = !self.is_dark;
    }
}

/// Original dbm's search-match yellow (ANSI `Color::Yellow`, not a pure RGB
/// yellow), chosen as the global accent for emphasis slots (search match text,
/// active match, match cell border, active search input).
const ACCENT_YELLOW: Color = Color::Yellow;

/// Primary foreground reset to the terminal default, matching the sql editor's
/// base text (edtui `Style::default()`). Keeps history / results / explorer
/// primary text aligned with the editor.
const FG_RESET: Color = Color::Reset;

/// The app's default theme, with a `dark` and a `light` palette.
///
/// This is a custom theme tuned for dbm2, not an off-the-shelf color scheme:
/// the background/border tones draw on the Dracula dark palette, but the
/// primary foreground follows the terminal default, the accent is fixed to the
/// original dbm's search-match yellow, and the selection layer is a light
/// "island" with dark-grey text across both modes (mirroring the original
/// dbm's results design, independent of the overall light/dark mode).
pub fn default() -> Theme {
    Theme {
        name: "default",
        is_dark: true,
        dark: Palette {
            fg: FG_RESET,                            // editor-aligned default
            fg_dim: Color::Rgb(0x62, 0x64, 0x74),    // comment
            bg: Color::Rgb(0x28, 0x2a, 0x36),        // background
            surface: Color::Rgb(0x21, 0x23, 0x2e),   // current line
            accent: ACCENT_YELLOW,
            border: Color::Rgb(0x44, 0x47, 0x5a),
            border_active: Color::Rgb(0xbd, 0x93, 0xf9),
            // Bright green — readable on the blue-grey selection background.
            active_fg: Color::Rgb(0x50, 0xfa, 0x7b),
            selection: Color::Rgb(0x44, 0x47, 0x5a),
            selection_bg: Color::Rgb(0xfa, 0xfc, 0xff),
            selection_cell_bg: Color::Rgb(0xe4, 0xea, 0xf5),
            selection_text: Color::Rgb(0x34, 0x37, 0x40),
            selection_focus_text: Color::Rgb(0x1c, 0x48, 0x8c),
            success: Color::Rgb(0x50, 0xfa, 0x7b),   // green
            warning: Color::Rgb(0xf1, 0xfa, 0x8c),   // yellow
            error: Color::Rgb(0xff, 0x55, 0x55),     // red
            info: Color::Rgb(0x8b, 0xe9, 0xfd),      // cyan
            muted: Color::Rgb(0x62, 0x64, 0x74),
        },
        light: Palette {
            fg: FG_RESET,                            // editor-aligned default
            fg_dim: Color::Rgb(0x62, 0x74, 0x8f),
            bg: Color::Rgb(0xfa, 0xf7, 0xf2),
            surface: Color::Rgb(0xf1, 0xe8, 0xe2),
            accent: ACCENT_YELLOW,
            border: Color::Rgb(0xcf, 0xc9, 0xc2),
            border_active: Color::Rgb(0xbd, 0x93, 0xf9),
            // Deep green — readable on the light blue-grey selection background.
            active_fg: Color::Rgb(0x1a, 0xb0, 0x4c),
            selection: Color::Rgb(0xcf, 0xc9, 0xc2),
            selection_bg: Color::Rgb(0xfa, 0xfc, 0xff),
            selection_cell_bg: Color::Rgb(0xe4, 0xea, 0xf5),
            selection_text: Color::Rgb(0x34, 0x37, 0x40),
            selection_focus_text: Color::Rgb(0x1c, 0x48, 0x8c),
            success: Color::Rgb(0x1a, 0xb0, 0x4c),
            warning: Color::Rgb(0xa5, 0x8a, 0x00),
            error: Color::Rgb(0xd3, 0x2f, 0x2f),
            info: Color::Rgb(0x0e, 0x74, 0x9a),
            muted: Color::Rgb(0x62, 0x74, 0x8f),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_dark_and_has_palette() {
        let theme = default();
        assert!(theme.is_dark);
        let _ = theme.palette(); // dark palette returned
    }

    #[test]
    fn toggle_switches_palette() {
        let mut theme = default();
        assert!(theme.is_dark);
        theme.toggle();
        assert!(!theme.is_dark);
        // light palette has a light background
        assert_eq!(theme.palette().bg, Color::Rgb(0xfa, 0xf7, 0xf2));
    }

    #[test]
    fn palettes_are_distinct() {
        let theme = default();
        assert_ne!(theme.dark.bg, theme.light.bg);
    }

    #[test]
    fn active_fg_does_not_clash_with_selection_bg() {
        // The active-marker foreground must stay distinct from the row-selection
        // background so an active row that is also the cursor row remains
        // readable.
        let theme = default();
        for p in [theme.dark.clone(), theme.light.clone()] {
            assert_ne!(
                p.active_fg, p.selection_bg,
                "theme {:?} active_fg clashes with selection_bg",
                theme.name
            );
            // active_fg must also be a real color, never Reset.
            assert_ne!(p.active_fg, Color::Reset, "theme {:?} active_fg is Reset", theme.name);
        }
    }
}