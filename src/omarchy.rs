//! Omarchy (https://omarchy.org) theme integration.
//!
//! Omarchy keeps the colors of the current theme in
//! `~/.local/state/omarchy/current/theme/colors.toml` and replaces that
//! directory whenever the user switches themes. The built-in `omarchy` theme
//! builds Herdr's palette from that file, and [`watch_theme`] notices a switch
//! so the server and the client can repaint and ask the host terminal for its
//! new colors without a restart.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use ratatui::style::Color;

use crate::app::state::Palette;
use crate::terminal_theme::RgbColor;

/// One bounded read per second per process, independent of rendering and pane count.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

fn current_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty())?;
    Some(PathBuf::from(home).join(".local/state/omarchy/current"))
}

fn colors_path() -> Option<PathBuf> {
    current_dir().map(|dir| dir.join("theme/colors.toml"))
}

/// Whether this machine runs Omarchy.
pub(crate) fn is_available() -> bool {
    current_dir().is_some_and(|dir| dir.is_dir())
}

fn read_colors_source() -> Option<String> {
    std::fs::read_to_string(colors_path()?).ok()
}

/// The current Omarchy theme as a Herdr palette, or `None` when there is no
/// readable Omarchy theme.
pub(crate) fn palette() -> Option<Palette> {
    let colors = OmarchyColors::parse(&read_colors_source()?)?;
    Some(colors.palette())
}

/// Calls `on_change` once the current Omarchy theme changes, and once more a
/// second later: Omarchy reloads the terminals after it switches the files,
/// so the terminal answers color queries with the new theme only then.
/// Stops when `on_change` returns false. Does nothing without Omarchy.
pub(crate) fn watch_theme(name: &str, mut on_change: impl FnMut() -> bool + Send + 'static) {
    if !is_available() {
        return;
    }
    let spawned = crate::thread_spawn::spawn_named(name, move || {
        let mut previous = read_colors_source();
        let mut settle_pending = false;
        loop {
            std::thread::sleep(POLL_INTERVAL);
            let current = read_colors_source();
            // A switch replaces the directory, so a missing file is a switch in progress.
            let changed = current.is_some() && current != previous;
            if current.is_some() {
                previous = current;
            }
            if changed || settle_pending {
                settle_pending = changed;
                if !on_change() {
                    break;
                }
            }
        }
    });
    if let Err(err) = spawned {
        tracing::warn!(err = %err, "failed to spawn Omarchy theme watcher");
    }
}

/// The colors of an Omarchy theme, by their `colors.toml` names.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OmarchyColors(HashMap<String, RgbColor>);

impl OmarchyColors {
    fn parse(source: &str) -> Option<Self> {
        let table: toml::Table = toml::from_str(source).ok()?;
        let colors = table
            .into_iter()
            .filter_map(|(key, value)| Some((key, parse_hex(value.as_str()?)?)))
            .collect::<HashMap<_, _>>();
        let colors = Self(colors);
        // Without these two there is no theme to speak of.
        colors.background()?;
        colors.foreground()?;
        Some(colors)
    }

    /// The first of `keys` the theme defines. Older themes only carry the
    /// terminal palette (color0..color15), newer ones name every color.
    fn get(&self, keys: &[&str]) -> Option<RgbColor> {
        keys.iter().find_map(|key| self.0.get(*key).copied())
    }

    fn background(&self) -> Option<RgbColor> {
        self.get(&["background", "color0"])
    }

    fn foreground(&self) -> Option<RgbColor> {
        self.get(&["foreground", "color7"])
    }

    /// Maps the theme onto Herdr's tokens the way the built-in Catppuccin
    /// palette sits on Catppuccin, so Omarchy's Catppuccin theme looks like
    /// Herdr's default.
    fn palette(&self) -> Palette {
        let background = self.background().unwrap_or_default();
        let foreground = self.foreground().unwrap_or_default();
        let between = |amount| mix(background, foreground, amount);

        let blue = self.get(&["blue", "color4"]).unwrap_or(foreground);
        let red = self.get(&["red", "color1"]).unwrap_or(foreground);
        let green = self.get(&["green", "color2"]).unwrap_or(foreground);
        let yellow = self.get(&["yellow", "color3"]).unwrap_or(foreground);
        let magenta = self.get(&["magenta", "color5"]).unwrap_or(blue);
        let cyan = self.get(&["cyan", "color6"]).unwrap_or(blue);
        let orange = self.get(&["orange"]).unwrap_or(yellow);
        let accent = self.get(&["accent"]).unwrap_or(blue);

        let dark_background = self.get(&["dark_background"]).unwrap_or(background);
        let lighter_background = self
            .get(&["lighter_background", "selection_background"])
            .unwrap_or_else(|| between(0.12));
        let selection = self
            .get(&["selection", "selection_background"])
            .unwrap_or_else(|| between(0.2));
        let dark_foreground = self
            .get(&["dark_foreground", "muted", "color8"])
            .unwrap_or_else(|| between(0.45));
        let light_foreground = self
            .get(&["light_foreground"])
            .unwrap_or_else(|| mix(foreground, background, 0.1));

        Palette {
            accent: rgb(accent),
            panel_bg: rgb(dark_background),
            sidebar_bg: Color::Reset,
            active_row_bg: rgb(background),
            selection_bg: rgb(lighter_background),
            surface0: rgb(lighter_background),
            surface1: rgb(selection),
            surface_dim: rgb(background),
            overlay0: rgb(dark_foreground),
            overlay1: rgb(mix(dark_foreground, light_foreground, 0.25)),
            text: rgb(foreground),
            subtext0: rgb(mix(light_foreground, dark_foreground, 0.25)),
            mauve: rgb(magenta),
            green: rgb(green),
            yellow: rgb(yellow),
            red: rgb(red),
            blue: rgb(blue),
            teal: rgb(cyan),
            peach: rgb(orange),
            pane_border: None,
            pane_border_focused: None,
        }
    }
}

fn parse_hex(value: &str) -> Option<RgbColor> {
    let hex = value.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let channel = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).ok();
    Some(RgbColor {
        r: channel(0..2)?,
        g: channel(2..4)?,
        b: channel(4..6)?,
    })
}

fn mix(base: RgbColor, target: RgbColor, amount: f32) -> RgbColor {
    let channel = |base: u8, target: u8| {
        (f32::from(base) + (f32::from(target) - f32::from(base)) * amount).round() as u8
    };
    RgbColor {
        r: channel(base.r, target.r),
        g: channel(base.g, target.g),
        b: channel(base.b, target.b),
    }
}

fn rgb(color: RgbColor) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATPPUCCIN: &str = r##"
mode = "dark"

accent = "#89b4fa"
selection = "#45475a"
muted = "#585b70"

background = "#1e1e2e"
dark_background = "#161622"
darker_background = "#101019"
lighter_background = "#313244"

foreground = "#cdd6f4"
dark_foreground = "#6c7086"
light_foreground = "#bac2de"
bright_foreground = "#cdd6f4"

red = "#f38ba8"
yellow = "#f9e2af"
orange = "#f6b6ab"
green = "#a6e3a1"
cyan = "#94e2d5"
blue = "#89b4fa"
magenta = "#f5c2e7"
"##;

    #[test]
    fn catppuccin_theme_matches_the_builtin_catppuccin_surfaces() {
        let palette = OmarchyColors::parse(CATPPUCCIN).unwrap().palette();
        let builtin = Palette::catppuccin();

        assert_eq!(palette.accent, builtin.accent);
        assert_eq!(palette.active_row_bg, builtin.active_row_bg);
        assert_eq!(palette.selection_bg, builtin.selection_bg);
        assert_eq!(palette.surface0, builtin.surface0);
        assert_eq!(palette.surface1, builtin.surface1);
        assert_eq!(palette.surface_dim, builtin.surface_dim);
        assert_eq!(palette.overlay0, builtin.overlay0);
        // overlay1 is derived, so it lands within rounding of Catppuccin's.
        assert_eq!(palette.overlay1, Color::Rgb(128, 133, 156));
        assert_eq!(palette.text, builtin.text);
        assert_eq!(palette.teal, builtin.teal);
        assert_eq!(palette.panel_bg, Color::Rgb(0x16, 0x16, 0x22));
        assert_eq!(palette.peach, Color::Rgb(0xf6, 0xb6, 0xab));
        assert_eq!(palette.sidebar_bg, Color::Reset);
    }

    #[test]
    fn terminal_palette_only_themes_fall_back_to_numbered_colors() {
        let palette = OmarchyColors::parse(
            r##"
color0 = "#000000"
color1 = "#aa0000"
color2 = "#00aa00"
color3 = "#aaaa00"
color4 = "#0000aa"
color7 = "#ffffff"
"##,
        )
        .unwrap()
        .palette();

        assert_eq!(palette.accent, Color::Rgb(0, 0, 0xaa));
        assert_eq!(palette.red, Color::Rgb(0xaa, 0, 0));
        assert_eq!(palette.peach, Color::Rgb(0xaa, 0xaa, 0));
        assert_eq!(palette.text, Color::Rgb(0xff, 0xff, 0xff));
        assert_eq!(palette.panel_bg, Color::Rgb(0, 0, 0));
    }

    #[test]
    fn themes_without_background_or_foreground_are_rejected() {
        assert!(OmarchyColors::parse(r##"accent = "#89b4fa""##).is_none());
        assert!(OmarchyColors::parse("not toml = = =").is_none());
    }

    #[test]
    fn hex_colors_parse_strictly() {
        assert_eq!(
            parse_hex("#1e1e2e"),
            Some(RgbColor {
                r: 0x1e,
                g: 0x1e,
                b: 0x2e
            })
        );
        assert_eq!(
            parse_hex(" #FFFFFF "),
            Some(RgbColor {
                r: 255,
                g: 255,
                b: 255
            })
        );
        assert_eq!(parse_hex("1e1e2e"), None);
        assert_eq!(parse_hex("#fff"), None);
        assert_eq!(parse_hex("#ééé"), None);
    }
}
