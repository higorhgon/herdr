//! Unfocused split pane dimming, modeled on Ghostty's `unfocused-split-opacity`
//! and `unfocused-split-fill`.
//!
//! Ghostty draws a rectangle of the fill color over every unfocused split, at
//! `1 - opacity` alpha. Herdr has no such layer, so it blends every cell of an
//! unfocused pane the same way: `color * opacity + fill * (1 - opacity)`. The
//! host terminal's own palette resolves default and indexed colors first, so
//! the result matches what the overlay would show.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

use crate::protocol::{color_to_u32, u32_to_color, FrameData};
use crate::terminal_theme::{RgbColor, TerminalTheme};

/// Ghostty's floor for unfocused-split-opacity, so a pane never disappears.
const MIN_OPACITY: f32 = 0.15;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UnfocusedPaneDim {
    /// Share of each color that stays; the fill makes up the rest.
    opacity: f32,
    /// What unfocused panes fade toward. None is the terminal background.
    fill: Option<Color>,
}

impl UnfocusedPaneDim {
    /// From the `ui.unfocused_pane_opacity` and `ui.unfocused_pane_fill` settings.
    pub(crate) fn new(opacity: f32, fill: &str) -> Self {
        let opacity = if opacity.is_finite() {
            opacity.clamp(MIN_OPACITY, 1.0)
        } else {
            1.0
        };
        let fill = fill.trim();
        let fill = (!fill.is_empty() && !fill.eq_ignore_ascii_case("background"))
            .then(|| crate::config::parse_color(fill))
            .filter(|color| *color != Color::Reset);
        Self { opacity, fill }
    }

    pub(crate) fn is_enabled(self) -> bool {
        self.opacity < 1.0
    }

    /// Dims the cells of `area` in `frame`.
    pub(crate) fn apply(self, frame: &mut FrameData, area: Rect, host: &TerminalTheme) {
        let width = usize::from(frame.width);
        let left = usize::from(area.x.min(frame.width));
        let right = usize::from(area.right().min(frame.width));
        let bottom = area.bottom().min(frame.height);
        for y in area.y..bottom {
            let row = usize::from(y) * width;
            if let Some(cells) = frame.cells.get_mut(row + left..row + right) {
                self.apply_cells(cells, host);
            }
        }
    }

    /// Dims a run of cells, such as one row of a retained pane surface patch.
    pub(crate) fn apply_cells(self, cells: &mut [crate::protocol::CellData], host: &TerminalTheme) {
        if !self.is_enabled() {
            return;
        }
        let fill = match self.fill {
            Some(color) => resolve(color, host, Layer::Background),
            None => host.background,
        };
        let mut blender = fill.map(|fill| Blender {
            dim: self,
            fill,
            host,
            last: [None; 2],
        });
        for cell in cells {
            match blender.as_mut() {
                Some(blender) => self.dim_cell(cell, blender),
                // Without the target color there is nothing to blend toward;
                // the terminal's faint attribute is the closest stand-in.
                None => cell.modifier |= Modifier::DIM.bits(),
            }
        }
    }

    fn dim_cell(self, cell: &mut crate::protocol::CellData, blender: &mut Blender<'_>) {
        match blender.dimmed(cell.fg, Layer::Foreground) {
            Some(fg) => cell.fg = fg,
            None => cell.modifier |= Modifier::DIM.bits(),
        }
        // A default background already sits under the fill when the fill is
        // that background, and leaving it default keeps a translucent terminal
        // window translucent.
        let reversed = cell.modifier & Modifier::REVERSED.bits() != 0;
        if cell.bg == color_to_u32(Color::Reset) && self.fill.is_none() && !reversed {
            return;
        }
        if let Some(bg) = blender.dimmed(cell.bg, Layer::Background) {
            cell.bg = bg;
        }
    }

    fn blend(self, color: RgbColor, fill: RgbColor) -> u32 {
        let channel = |color: u8, fill: u8| {
            (f32::from(color) * self.opacity + f32::from(fill) * (1.0 - self.opacity)).round() as u8
        };
        color_to_u32(Color::Rgb(
            channel(color.r, fill.r),
            channel(color.g, fill.g),
            channel(color.b, fill.b),
        ))
    }
}

#[derive(Clone, Copy)]
enum Layer {
    Foreground,
    Background,
}

/// Blends packed cell colors, remembering the last one per layer: terminal
/// content comes in runs of one color, so most cells reuse the previous result.
struct Blender<'a> {
    dim: UnfocusedPaneDim,
    fill: RgbColor,
    host: &'a TerminalTheme,
    last: [Option<(u32, Option<u32>)>; 2],
}

impl Blender<'_> {
    fn dimmed(&mut self, packed: u32, layer: Layer) -> Option<u32> {
        if let Some((input, output)) = self.last[layer as usize] {
            if input == packed {
                return output;
            }
        }
        let output = resolve(u32_to_color(packed), self.host, layer)
            .map(|color| self.dim.blend(color, self.fill));
        self.last[layer as usize] = Some((packed, output));
        output
    }
}

fn resolve(color: Color, host: &TerminalTheme, layer: Layer) -> Option<RgbColor> {
    let index = match color {
        Color::Reset => {
            return match layer {
                Layer::Foreground => host.foreground,
                Layer::Background => host.background,
            }
        }
        Color::Rgb(r, g, b) => return Some(RgbColor { r, g, b }),
        Color::Indexed(index) => index,
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
    };
    host.palette[usize::from(index)].or_else(|| Some(xterm_color(index)))
}

/// The xterm default for a palette entry the host did not report.
fn xterm_color(index: u8) -> RgbColor {
    const BASE: [(u8, u8, u8); 16] = [
        (0x00, 0x00, 0x00),
        (0xcd, 0x00, 0x00),
        (0x00, 0xcd, 0x00),
        (0xcd, 0xcd, 0x00),
        (0x00, 0x00, 0xee),
        (0xcd, 0x00, 0xcd),
        (0x00, 0xcd, 0xcd),
        (0xe5, 0xe5, 0xe5),
        (0x7f, 0x7f, 0x7f),
        (0xff, 0x00, 0x00),
        (0x00, 0xff, 0x00),
        (0xff, 0xff, 0x00),
        (0x5c, 0x5c, 0xff),
        (0xff, 0x00, 0xff),
        (0x00, 0xff, 0xff),
        (0xff, 0xff, 0xff),
    ];
    let (r, g, b) = match index {
        0..=15 => BASE[usize::from(index)],
        16..=231 => {
            let cube = index - 16;
            let level = |value: u8| if value == 0 { 0 } else { 55 + value * 40 };
            (level(cube / 36), level(cube / 6 % 6), level(cube % 6))
        }
        232..=255 => {
            let gray = 8 + (index - 232) * 10;
            (gray, gray, gray)
        }
    };
    RgbColor { r, g, b }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::CellData;

    fn rgb(r: u8, g: u8, b: u8) -> RgbColor {
        RgbColor { r, g, b }
    }

    fn host() -> TerminalTheme {
        TerminalTheme::default()
            .with_color(
                crate::terminal_theme::DefaultColorKind::Foreground,
                rgb(200, 200, 200),
            )
            .with_color(
                crate::terminal_theme::DefaultColorKind::Background,
                rgb(0, 0, 0),
            )
            .with_palette_color(1, rgb(250, 50, 100))
    }

    fn dim(opacity: f32, fill: &str) -> UnfocusedPaneDim {
        UnfocusedPaneDim::new(opacity, fill)
    }

    fn frame(cells: Vec<CellData>) -> FrameData {
        let mut frame = FrameData::from_ratatui_buffer_with_hyperlinks(
            &ratatui::buffer::Buffer::empty(Rect::new(0, 0, cells.len() as u16, 1)),
            None,
            &[],
        );
        frame.cells = cells;
        frame
    }

    fn cell(fg: Color, bg: Color, modifier: Modifier) -> CellData {
        CellData {
            symbol: "x".into(),
            fg: color_to_u32(fg),
            bg: color_to_u32(bg),
            modifier: modifier.bits(),
            skip: false,
            hyperlink: None,
        }
    }

    #[test]
    fn opacity_follows_ghostty_defaults_and_bounds() {
        assert!(!dim(1.0, "").is_enabled());
        assert!(!dim(f32::NAN, "").is_enabled());
        assert_eq!(dim(0.0, "").opacity, MIN_OPACITY);
        assert_eq!(dim(3.0, "").opacity, 1.0);
        assert_eq!(dim(0.7, "background").fill, None);
        assert_eq!(dim(0.7, "#102030").fill, Some(Color::Rgb(0x10, 0x20, 0x30)));
    }

    #[test]
    fn colors_fade_toward_the_terminal_background() {
        let mut frame = frame(vec![
            cell(Color::Reset, Color::Reset, Modifier::empty()),
            cell(Color::Red, Color::Rgb(100, 100, 100), Modifier::empty()),
            cell(Color::Indexed(196), Color::Reset, Modifier::empty()),
        ]);
        dim(0.7, "").apply(&mut frame, Rect::new(0, 0, 3, 1), &host());

        let colors = frame
            .cells
            .iter()
            .map(|cell| (u32_to_color(cell.fg), u32_to_color(cell.bg)))
            .collect::<Vec<_>>();
        assert_eq!(
            colors,
            vec![
                // The default background stays default, so translucency survives.
                (Color::Rgb(140, 140, 140), Color::Reset),
                // Named colors resolve through the host palette.
                (Color::Rgb(175, 35, 70), Color::Rgb(70, 70, 70)),
                // Unreported indexed colors use the xterm defaults.
                (Color::Rgb(179, 0, 0), Color::Reset),
            ]
        );
    }

    #[test]
    fn a_custom_fill_also_tints_the_default_background() {
        let mut frame = frame(vec![cell(Color::Reset, Color::Reset, Modifier::empty())]);
        dim(0.5, "#ffffff").apply(&mut frame, Rect::new(0, 0, 1, 1), &host());

        assert_eq!(u32_to_color(frame.cells[0].fg), Color::Rgb(228, 228, 228));
        assert_eq!(u32_to_color(frame.cells[0].bg), Color::Rgb(128, 128, 128));
    }

    #[test]
    fn reversed_cells_dim_the_background_they_show_as_text() {
        let mut frame = frame(vec![cell(Color::Reset, Color::Reset, Modifier::REVERSED)]);
        dim(0.5, "").apply(&mut frame, Rect::new(0, 0, 1, 1), &host());

        assert_eq!(u32_to_color(frame.cells[0].fg), Color::Rgb(100, 100, 100));
        assert_eq!(u32_to_color(frame.cells[0].bg), Color::Rgb(0, 0, 0));
    }

    #[test]
    fn unknown_host_colors_fall_back_to_the_faint_attribute() {
        let mut frame = frame(vec![cell(Color::Red, Color::Reset, Modifier::BOLD)]);
        dim(0.7, "").apply(&mut frame, Rect::new(0, 0, 1, 1), &TerminalTheme::default());

        let cell = &frame.cells[0];
        assert_eq!(u32_to_color(cell.fg), Color::Red);
        assert_eq!(cell.modifier, (Modifier::BOLD | Modifier::DIM).bits());
    }

    #[test]
    fn only_the_given_area_changes() {
        let mut frame = frame(vec![
            cell(Color::Reset, Color::Reset, Modifier::empty()),
            cell(Color::Reset, Color::Reset, Modifier::empty()),
        ]);
        dim(0.7, "").apply(&mut frame, Rect::new(1, 0, 5, 1), &host());

        assert_eq!(u32_to_color(frame.cells[0].fg), Color::Reset);
        assert_eq!(u32_to_color(frame.cells[1].fg), Color::Rgb(140, 140, 140));
    }

    #[test]
    fn xterm_fallback_covers_the_cube_and_gray_ramp() {
        assert_eq!(xterm_color(16), rgb(0, 0, 0));
        assert_eq!(xterm_color(196), rgb(255, 0, 0));
        assert_eq!(xterm_color(231), rgb(255, 255, 255));
        assert_eq!(xterm_color(232), rgb(8, 8, 8));
        assert_eq!(xterm_color(255), rgb(238, 238, 238));
    }
}
