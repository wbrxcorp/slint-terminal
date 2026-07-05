//! Grid -> RGBA pixel buffer.
//!
//! Walks the terminal's renderable content, paints cell backgrounds, blends
//! glyphs on top via [`crate::font`], and draws the cursor. Produces a flat
//! RGBA8 buffer sized `cols*cell_w` by `rows*cell_h`.

use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};

use crate::font::FontRaster;
use crate::terminal::TerminalState;

pub struct Renderer {
    font: FontRaster,
    cell_w: usize,
    cell_h: usize,
    buf: Vec<u8>,
    px_w: usize,
    px_h: usize,
    default_fg: [u8; 3],
    default_bg: [u8; 3],
}

impl Renderer {
    pub fn new(font: FontRaster) -> Self {
        let (cell_w, cell_h) = font.cell_size();
        Self {
            font,
            cell_w,
            cell_h,
            buf: Vec::new(),
            px_w: 0,
            px_h: 0,
            default_fg: [0xcc, 0xcc, 0xcc],
            default_bg: [0x10, 0x10, 0x10],
        }
    }

    pub fn cell_size(&self) -> (usize, usize) {
        (self.cell_w, self.cell_h)
    }

    /// Pixel size for a given grid, so callers can size PTY/window to match.
    pub fn pixel_size(&self, cols: usize, rows: usize) -> (u32, u32) {
        ((cols * self.cell_w) as u32, (rows * self.cell_h) as u32)
    }

    /// Render the current grid. Returns `(rgba, width, height)`.
    pub fn render(&mut self, state: &TerminalState) -> (&[u8], u32, u32) {
        let (cols, rows) = state.dimensions();
        let px_w = cols * self.cell_w;
        let px_h = rows * self.cell_h;
        if px_w != self.px_w || px_h != self.px_h || self.buf.len() != px_w * px_h * 4 {
            self.px_w = px_w;
            self.px_h = px_h;
            self.buf.resize(px_w * px_h * 4, 0);
        }

        // Clear to default background.
        for px in self.buf.chunks_exact_mut(4) {
            px[0] = self.default_bg[0];
            px[1] = self.default_bg[1];
            px[2] = self.default_bg[2];
            px[3] = 255;
        }

        let stride = px_w * 4;
        let term = state.term();
        let content = term.renderable_content();
        let display_offset = content.display_offset as i32;
        let colors = content.colors;

        for indexed in content.display_iter {
            let cell = indexed.cell;
            let row = (indexed.point.line.0 + display_offset) as usize;
            let col = indexed.point.column.0;
            if row >= rows || col >= cols {
                continue;
            }

            // The trailing spacer of a wide char carries no glyph of its own.
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }

            let inverse = cell.flags.contains(Flags::INVERSE);
            let mut fg = resolve(cell.fg, colors, self.default_fg, true);
            let mut bg = resolve(cell.bg, colors, self.default_bg, false);
            if inverse {
                std::mem::swap(&mut fg, &mut bg);
            }

            let cells_wide = if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
            let ox = col * self.cell_w;
            let oy = row * self.cell_h;

            // Paint background (over the spanned width for wide chars).
            fill_cell(&mut self.buf, stride, px_h, ox, oy, self.cell_w * cells_wide, self.cell_h, bg);

            let hidden = cell.flags.contains(Flags::HIDDEN);
            if !hidden && cell.c != ' ' && cell.c != '\0' {
                self.font
                    .draw_glyph(&mut self.buf, stride, px_h, ox, oy, cell.c, fg, cells_wide);
            }
        }

        // Cursor: a filled block using the default foreground, glyph re-drawn in bg.
        let cur = content.cursor;
        if !matches!(cur.shape, CursorShape::Hidden) {
            let row = (cur.point.line.0 + display_offset) as i64;
            let col = cur.point.column.0;
            if row >= 0 && (row as usize) < rows && col < cols {
                let ox = col * self.cell_w;
                let oy = row as usize * self.cell_h;
                fill_cell(
                    &mut self.buf, stride, px_h, ox, oy, self.cell_w, self.cell_h, self.default_fg,
                );
            }
        }

        (&self.buf, px_w as u32, px_h as u32)
    }
}

fn fill_cell(
    buf: &mut [u8],
    stride: usize,
    buf_h: usize,
    ox: usize,
    oy: usize,
    w: usize,
    h: usize,
    color: [u8; 3],
) {
    for y in oy..(oy + h).min(buf_h) {
        let base = y * stride + ox * 4;
        for x in 0..w {
            let idx = base + x * 4;
            if idx + 3 >= buf.len() {
                break;
            }
            buf[idx] = color[0];
            buf[idx + 1] = color[1];
            buf[idx + 2] = color[2];
            buf[idx + 3] = 255;
        }
    }
}

/// Resolve an alacritty `Color` to RGB, honoring runtime color overrides in
/// `colors` and falling back to a fixed default palette.
fn resolve(color: Color, colors: &Colors, default: [u8; 3], is_fg: bool) -> [u8; 3] {
    match color {
        Color::Spec(rgb) => [rgb.r, rgb.g, rgb.b],
        Color::Named(named) => {
            if let Some(rgb) = colors[named] {
                return [rgb.r, rgb.g, rgb.b];
            }
            named_default(named).unwrap_or(default)
        }
        Color::Indexed(i) => {
            let _ = is_fg;
            if let Some(rgb) = colors[i as usize] {
                return [rgb.r, rgb.g, rgb.b];
            }
            indexed_default(i)
        }
    }
}

/// A conventional dark-theme 16-color ANSI palette.
const ANSI16: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00], // black
    [0xcc, 0x33, 0x33], // red
    [0x33, 0xcc, 0x33], // green
    [0xcc, 0xcc, 0x33], // yellow
    [0x33, 0x66, 0xcc], // blue
    [0xcc, 0x33, 0xcc], // magenta
    [0x33, 0xcc, 0xcc], // cyan
    [0xcc, 0xcc, 0xcc], // white
    [0x66, 0x66, 0x66], // bright black
    [0xff, 0x66, 0x66], // bright red
    [0x66, 0xff, 0x66], // bright green
    [0xff, 0xff, 0x66], // bright yellow
    [0x66, 0x99, 0xff], // bright blue
    [0xff, 0x66, 0xff], // bright magenta
    [0x66, 0xff, 0xff], // bright cyan
    [0xff, 0xff, 0xff], // bright white
];

fn named_default(named: NamedColor) -> Option<[u8; 3]> {
    let i = named as usize;
    if i < 16 {
        Some(ANSI16[i])
    } else {
        None // Foreground/Background/etc fall through to the caller default.
    }
}

fn indexed_default(i: u8) -> [u8; 3] {
    match i {
        0..=15 => ANSI16[i as usize],
        16..=231 => {
            // 6x6x6 color cube.
            let i = i - 16;
            let r = i / 36;
            let g = (i % 36) / 6;
            let b = i % 6;
            let s = |c: u8| if c == 0 { 0 } else { 55 + c * 40 };
            [s(r), s(g), s(b)]
        }
        232..=255 => {
            let v = 8 + (i - 232) * 10;
            [v, v, v]
        }
    }
}
