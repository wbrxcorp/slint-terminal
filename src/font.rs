//! Font loading and glyph rasterization via fontdue.
//!
//! For the PoC we use a single monospace font that already carries both Latin
//! and CJK glyphs (`Noto Sans Mono CJK JP`), resolved through fontconfig. That
//! keeps the font stack to one face while still satisfying the "Japanese must
//! render" requirement. A real fallback chain can be layered on later.

use std::collections::HashMap;
use std::process::Command;

use fontdue::{Font, FontSettings};

/// A rasterized glyph: coverage bitmap plus placement metrics.
struct Glyph {
    /// Grayscale coverage, `width * height` bytes, 0..=255.
    bitmap: Vec<u8>,
    width: usize,
    height: usize,
    /// Offset from the pen origin to the top-left of the bitmap.
    xmin: i32,
    ymin: i32,
    /// Horizontal advance (unused for the fixed grid, kept for reference).
    advance: f32,
}

/// Monospace font with a glyph cache and precomputed cell metrics.
pub struct FontRaster {
    font: Font,
    px: f32,
    /// Cell dimensions in pixels.
    cell_w: usize,
    cell_h: usize,
    /// Baseline measured from the top of the cell.
    baseline: i32,
    cache: HashMap<char, Glyph>,
}

impl FontRaster {
    /// Load the default monospace font at `px` pixels/em.
    pub fn new(px: f32) -> Result<Self, String> {
        let (path, index) = resolve_monospace()?;
        let bytes = std::fs::read(&path).map_err(|e| format!("read {path}: {e}"))?;
        let font = Font::from_bytes(
            bytes,
            FontSettings {
                collection_index: index,
                scale: px,
                ..FontSettings::default()
            },
        )?;

        // Cell metrics. Width comes from a representative ASCII glyph advance;
        // height/baseline from the font's line metrics so descenders fit.
        let m = font.metrics('M', px);
        let cell_w = m.advance_width.ceil().max(1.0) as usize;

        let line = font
            .horizontal_line_metrics(px)
            .ok_or("font has no horizontal line metrics")?;
        let cell_h = (line.ascent - line.descent + line.line_gap).ceil().max(1.0) as usize;
        let baseline = line.ascent.ceil() as i32;

        Ok(Self {
            font,
            px,
            cell_w,
            cell_h,
            baseline,
            cache: HashMap::new(),
        })
    }

    pub fn cell_size(&self) -> (usize, usize) {
        (self.cell_w, self.cell_h)
    }

    fn glyph(&mut self, ch: char) -> &Glyph {
        let px = self.px;
        self.cache.entry(ch).or_insert_with(|| {
            let (metrics, bitmap) = self.font.rasterize(ch, px);
            Glyph {
                bitmap,
                width: metrics.width,
                height: metrics.height,
                xmin: metrics.xmin,
                ymin: metrics.ymin,
                advance: metrics.advance_width,
            }
        })
    }

    /// Blend the glyph for `ch` into `dst` (an RGBA buffer of size
    /// `stride/4 * ...`) at cell origin `(ox, oy)` in pixels, using solid
    /// foreground color `fg` over whatever background was already painted.
    /// `cells_wide` is 1 for normal and 2 for wide (CJK) glyphs; it only
    /// affects clamping, not the raster.
    pub fn draw_glyph(
        &mut self,
        dst: &mut [u8],
        stride: usize,
        buf_h: usize,
        ox: usize,
        oy: usize,
        ch: char,
        fg: [u8; 3],
        cells_wide: usize,
    ) {
        let baseline = self.baseline;
        let cell_w = self.cell_w * cells_wide;
        let g = self.glyph(ch);
        if g.width == 0 || g.height == 0 {
            return;
        }

        // Pen origin: cursor at cell left, baseline `baseline` px down.
        // fontdue's ymin is the offset of the bitmap bottom below the baseline.
        let gx0 = ox as i32 + g.xmin;
        let gy0 = oy as i32 + baseline - (g.height as i32 + g.ymin);

        for row in 0..g.height {
            let py = gy0 + row as i32;
            if py < 0 || py as usize >= buf_h {
                continue;
            }
            for col in 0..g.width {
                let px = gx0 + col as i32;
                if px < 0 || px as usize >= ox + cell_w {
                    continue;
                }
                let cov = g.bitmap[row * g.width + col];
                if cov == 0 {
                    continue;
                }
                let idx = py as usize * stride + px as usize * 4;
                if idx + 3 >= dst.len() {
                    continue;
                }
                let a = cov as u32;
                for c in 0..3 {
                    let bg = dst[idx + c] as u32;
                    let fgc = fg[c] as u32;
                    dst[idx + c] = ((fgc * a + bg * (255 - a)) / 255) as u8;
                }
                dst[idx + 3] = 255;
            }
        }
        let _ = g.advance;
    }
}

/// Resolve the monospace font file and TrueType-collection index via
/// fontconfig. Falls back to a couple of well-known paths if `fc-match` is
/// unavailable.
fn resolve_monospace() -> Result<(String, u32), String> {
    if let Ok(out) = Command::new("fc-match")
        .args(["--format=%{file}:%{index}", "monospace"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some((file, index)) = s.rsplit_once(':') {
                let index = index.trim().parse::<u32>().unwrap_or(0);
                if !file.is_empty() {
                    return Ok((file.to_string(), index));
                }
            }
        }
    }

    for cand in [
        "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    ] {
        if std::path::Path::new(cand).exists() {
            return Ok((cand.to_string(), 0));
        }
    }
    Err("could not resolve a monospace font (fc-match failed, no fallback found)".into())
}
