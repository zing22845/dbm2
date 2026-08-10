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

use ratatui::style::{Color, Style};
use ratatui_themes::ThemeName;

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
    /// Highlighted list selection (foreground emphasis).
    pub selection: Color,
    /// The background color used to highlight the cursor-selected row in lists
    /// and trees. Kept separate from `selection` so every pane can render the
    /// selected row with a unified, visible background (mirroring the original
    /// dbm's connections selection) instead of relying on foreground alone.
    pub selection_bg: Color,
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
    /// Build a `Palette` from a `ratatui-themes` palette.
    ///
    /// `ratatui-themes` is used purely as a source of curated color data; this
    /// adapter isolates it from the architecture core so feature views keep
    /// depending only on our semantic `Palette`. Fields that `ratatui-themes`
    /// does not expose (`fg_dim`, `surface`, `border`, `border_active`) are
    /// derived from the closest available slot.
    /// The border style for a pane: the active (accent) color when the pane is
    /// on the focus chain, otherwise the regular border color. Pane views use
    /// this instead of hand-rolling `if focused { border_active } else { border }`
    /// so focus highlighting is defined in one place.
    pub fn active_border(&self, active: bool) -> Style {
        Style::default().fg(if active { self.border_active } else { self.border })
    }

    pub fn from_ratatui(p: &ratatui_themes::ThemePalette) -> Self {
        Palette {
            fg: p.fg,
            fg_dim: p.muted,
            bg: p.bg,
            // ratatui-themes has no dedicated surface; reuse the background so
            // panel backgrounds stay consistent.
            surface: p.bg,
            accent: p.accent,
            // No dedicated border slots upstream: borders are drawn with the
            // muted fg and become the accent when focused.
            border: p.muted,
            border_active: p.accent,
            selection: p.selection,
            selection_bg: p.selection,
            success: p.success,
            warning: p.warning,
            error: p.error,
            info: p.info,
            muted: p.muted,
        }
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

    /// Build a theme from two `ratatui-themes` palettes, one for the dark mode
    /// and one for the light mode. If the scheme has no distinct light variant,
    /// pass the same name for both (the mode flag still flips, but both modes
    /// share the palette).
    pub fn from_ratatui(name: &'static str, dark_name: ThemeName, light_name: ThemeName) -> Self {
        let mut dark = Palette::from_ratatui(&dark_name.palette());
        let mut light = Palette::from_ratatui(&light_name.palette());
        // A unified, visible row-selection background per mode: dark gets a
        // mid-blue-grey a step up from the background, light mirrors the
        // original dbm's connections selection (light blue-grey `228,234,245`).
        dark.selection_bg = Color::Rgb(0x3b, 0x42, 0x52);
        light.selection_bg = Color::Rgb(0xe4, 0xea, 0xf5);
        Theme {
            name,
            is_dark: true,
            dark,
            light,
        }
    }
}

/// A dark theme modeled after the Dracula color scheme.
pub fn dracula() -> Theme {
    Theme {
        name: "dracula",
        is_dark: true,
        dark: Palette {
            fg: Color::Rgb(0xf8, 0xf8, 0xf2),       // foreground
            fg_dim: Color::Rgb(0x62, 0x64, 0x74),    // comment
            bg: Color::Rgb(0x28, 0x2a, 0x36),        // background
            surface: Color::Rgb(0x21, 0x23, 0x2e),   // current line
            accent: Color::Rgb(0xbd, 0x93, 0xf9),    // purple
            border: Color::Rgb(0x44, 0x47, 0x5a),
            border_active: Color::Rgb(0xbd, 0x93, 0xf9),
            selection: Color::Rgb(0x44, 0x47, 0x5a),
            selection_bg: Color::Rgb(0x3d, 0x40, 0x52),
            success: Color::Rgb(0x50, 0xfa, 0x7b),   // green
            warning: Color::Rgb(0xf1, 0xfa, 0x8c),   // yellow
            error: Color::Rgb(0xff, 0x55, 0x55),     // red
            info: Color::Rgb(0x8b, 0xe9, 0xfd),      // cyan
            muted: Color::Rgb(0x62, 0x64, 0x74),
        },
        light: Palette {
            fg: Color::Rgb(0x28, 0x2a, 0x36),
            fg_dim: Color::Rgb(0x62, 0x74, 0x8f),
            bg: Color::Rgb(0xfa, 0xf7, 0xf2),
            surface: Color::Rgb(0xf1, 0xe8, 0xe2),
            accent: Color::Rgb(0xbd, 0x93, 0xf9),
            border: Color::Rgb(0xcf, 0xc9, 0xc2),
            border_active: Color::Rgb(0xbd, 0x93, 0xf9),
            selection: Color::Rgb(0xcf, 0xc9, 0xc2),
            selection_bg: Color::Rgb(0xe4, 0xea, 0xf5),
            success: Color::Rgb(0x1a, 0xb0, 0x4c),
            warning: Color::Rgb(0xa5, 0x8a, 0x00),
            error: Color::Rgb(0xd3, 0x2f, 0x2f),
            info: Color::Rgb(0x0e, 0x74, 0x9a),
            muted: Color::Rgb(0x62, 0x74, 0x8f),
        },
    }
}

/// A dark theme modeled after the Nord color scheme.
pub fn nord() -> Theme {
    Theme {
        name: "nord",
        is_dark: true,
        dark: Palette {
            fg: Color::Rgb(0xd8, 0xde, 0xe9),       // nord4
            fg_dim: Color::Rgb(0x4c, 0x56, 0x6a),    // nord3
            bg: Color::Rgb(0x2e, 0x34, 0x40),        // nord0
            surface: Color::Rgb(0x3b, 0x42, 0x52),   // nord1
            accent: Color::Rgb(0x88, 0xc0, 0xd0),    // nord8 (frost cyan)
            border: Color::Rgb(0x4c, 0x56, 0x6a),
            border_active: Color::Rgb(0x88, 0xc0, 0xd0),
            selection: Color::Rgb(0x43, 0x4c, 0x5e),
            selection_bg: Color::Rgb(0x3b, 0x42, 0x52),
            success: Color::Rgb(0xa3, 0xbe, 0x8c),   // nord14
            warning: Color::Rgb(0xeb, 0xcb, 0x8b),   // nord13
            error: Color::Rgb(0xbf, 0x61, 0x6a),     // nord11
            info: Color::Rgb(0x81, 0xa1, 0xc1),      // nord9
            muted: Color::Rgb(0x4c, 0x56, 0x6a),
        },
        light: Palette {
            fg: Color::Rgb(0x2e, 0x34, 0x40),
            fg_dim: Color::Rgb(0x4c, 0x56, 0x6a),
            bg: Color::Rgb(0xec, 0xef, 0xf4),        // nord6
            surface: Color::Rgb(0xe5, 0xe9, 0xf0),
            accent: Color::Rgb(0x88, 0xc0, 0xd0),
            border: Color::Rgb(0xd8, 0xde, 0xe9),
            border_active: Color::Rgb(0x88, 0xc0, 0xd0),
            selection: Color::Rgb(0xd8, 0xde, 0xe9),
            selection_bg: Color::Rgb(0xd8, 0xde, 0xe9),
            success: Color::Rgb(0x5e, 0x81, 0xac),
            warning: Color::Rgb(0xdb, 0xa0, 0x0d),
            error: Color::Rgb(0xbf, 0x61, 0x6a),
            info: Color::Rgb(0x81, 0xa1, 0xc1),
            muted: Color::Rgb(0x4c, 0x56, 0x6a),
        },
    }
}

/// A Solarized theme whose dark/light palettes come from `ratatui-themes`.
pub fn solarized() -> Theme {
    Theme::from_ratatui("solarized", ThemeName::SolarizedDark, ThemeName::SolarizedLight)
}

/// A Catppuccin theme: Mocha for dark, Latte for light.
pub fn catppuccin() -> Theme {
    Theme::from_ratatui(
        "catppuccin",
        ThemeName::CatppuccinMocha,
        ThemeName::CatppuccinLatte,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_dark_and_has_palette() {
        let theme = dracula();
        assert!(theme.is_dark);
        let _ = theme.palette(); // dark palette returned
    }

    #[test]
    fn toggle_switches_palette() {
        let mut theme = dracula();
        assert!(theme.is_dark);
        theme.toggle();
        assert!(!theme.is_dark);
        // light palette has a light background
        assert_eq!(theme.palette().bg, Color::Rgb(0xfa, 0xf7, 0xf2));
    }

    #[test]
    fn palettes_are_distinct() {
        let theme = dracula();
        assert_ne!(theme.dark.bg, theme.light.bg);
    }

    #[test]
    fn from_ratatui_maps_semantic_slots() {
        let p = Palette::from_ratatui(&ThemeName::Dracula.palette());
        // fg/bg/accent map directly from the source palette.
        assert_eq!(p.fg, ThemeName::Dracula.palette().fg);
        assert_eq!(p.bg, ThemeName::Dracula.palette().bg);
        assert_eq!(p.accent, ThemeName::Dracula.palette().accent);
        // derived slots are non-empty (never Reset) so UI stays visible.
        for slot in [p.fg_dim, p.surface, p.border, p.border_active] {
            assert_ne!(slot, Color::Reset);
        }
    }

    #[test]
    fn solarized_from_ratatui_has_distinct_dark_light() {
        let theme = solarized();
        assert!(theme.is_dark);
        assert_ne!(theme.dark.bg, theme.light.bg);
        // toggle flips to the light palette.
        let mut t = theme.clone();
        t.toggle();
        assert!(!t.is_dark);
    }

    #[test]
    fn catppuccin_has_dark_and_light_variants() {
        let theme = catppuccin();
        assert_ne!(theme.dark.bg, theme.light.bg);
    }
}
