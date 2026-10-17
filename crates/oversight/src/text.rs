//! Text: TrueType fonts rasterized with fontdue, glyphs cached as pixmaps.

use std::collections::HashMap;

use fontdue::{Font as FontData, FontSettings};
use tiny_skia::Pixmap;

use crate::types::{Color, Vec2};

/// DejaVu Sans (Latin, Cyrillic, Greek, ...), font id 1 in every engine
pub const DEFAULT_FONT: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");

pub struct Glyph {
    /// Coloured glyph image (None for blank glyphs such as space)
    pub pixmap: Option<Pixmap>,
    /// Offset of the image's top-left from the pen position on the baseline
    pub offset: Vec2,
    pub advance: f32,
}

pub struct Font {
    data: FontData,
    glyphs: HashMap<(char, u32, Color), Glyph>,
}

/// One glyph to draw: image key and top-left position relative to the
/// text's top-left corner
pub struct PlacedGlyph {
    pub key: (char, u32, Color),
    pub pos: Vec2,
}

impl Font {
    pub fn from_bytes(bytes: &[u8]) -> Result<Font, String> {
        let data =
            FontData::from_bytes(bytes, FontSettings::default()).map_err(|e| e.to_string())?;
        Ok(Font {
            data,
            glyphs: HashMap::new(),
        })
    }

    pub fn load(path: &str) -> Result<Font, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {}", path, e))?;
        Font::from_bytes(&bytes).map_err(|e| format!("{}: {}", path, e))
    }

    fn line_metrics(&self, size: f32) -> (f32, f32) {
        match self.data.horizontal_line_metrics(size) {
            Some(m) => (m.ascent, m.new_line_size),
            None => (size * 0.8, size * 1.2),
        }
    }

    fn glyph(&mut self, ch: char, size: f32, color: Color) -> &Glyph {
        let key = (ch, size.to_bits(), color);
        let data = &self.data;
        self.glyphs.entry(key).or_insert_with(|| {
            let (metrics, coverage) = data.rasterize(ch, size);
            let pixmap = Pixmap::new(metrics.width as u32, metrics.height as u32).map(|mut p| {
                for (px, &c) in p.data_mut().chunks_exact_mut(4).zip(&coverage) {
                    let a = (c as u16 * color.a as u16 + 127) / 255;
                    px[0] = ((color.r as u16 * a + 127) / 255) as u8;
                    px[1] = ((color.g as u16 * a + 127) / 255) as u8;
                    px[2] = ((color.b as u16 * a + 127) / 255) as u8;
                    px[3] = a as u8;
                }
                p
            });
            Glyph {
                pixmap,
                offset: Vec2::new(
                    metrics.xmin as f32,
                    -(metrics.height as f32 + metrics.ymin as f32),
                ),
                advance: metrics.advance_width,
            }
        })
    }

    pub fn cached(&self, key: &(char, u32, Color)) -> Option<&Glyph> {
        self.glyphs.get(key)
    }

    /// Lay out text ("\n" starts a new line); returns glyphs and the size
    pub fn layout(&mut self, text: &str, size: f32, color: Color) -> (Vec<PlacedGlyph>, Vec2) {
        let (ascent, line_height) = self.line_metrics(size);
        let mut placed = Vec::new();
        let (mut x, mut width, mut lines) = (0.0f32, 0.0f32, 1);
        let mut previous: Option<char> = None;
        for ch in text.chars() {
            if ch == '\n' {
                width = width.max(x);
                x = 0.0;
                lines += 1;
                previous = None;
                continue;
            }
            if let Some(prev) = previous {
                x += self.data.horizontal_kern(prev, ch, size).unwrap_or(0.0);
            }
            let baseline = ascent + (lines - 1) as f32 * line_height;
            let glyph = self.glyph(ch, size, color);
            if glyph.pixmap.is_some() {
                placed.push(PlacedGlyph {
                    key: (ch, size.to_bits(), color),
                    pos: Vec2::new(x + glyph.offset.x, baseline + glyph.offset.y),
                });
            }
            x += glyph.advance;
            previous = Some(ch);
        }
        width = width.max(x);
        (placed, Vec2::new(width, lines as f32 * line_height))
    }

    pub fn measure(&mut self, text: &str, size: f32) -> Vec2 {
        self.layout(text, size, Color::WHITE).1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_font_lays_out_cyrillic_and_lines() {
        let mut font = Font::from_bytes(DEFAULT_FONT).unwrap();
        let one = font.measure("Привет", 20.0);
        let two = font.measure("Привет\nмир", 20.0);
        assert!(one.x > 40.0 && one.x < 100.0, "{:?}", one);
        assert_eq!(two.x, one.x);
        assert!((two.y - 2.0 * one.y).abs() < 0.01);
        let (glyphs, _) = font.layout("a b", 20.0, Color::WHITE);
        assert_eq!(glyphs.len(), 2, "space has no image");
    }
}
