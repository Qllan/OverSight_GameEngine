//! Painting tile maps in the Scene window. With the selected entity's
//! TilemapRenderer and Paint on, a left drag puts the tile picked in the
//! palette (or nothing, with Erase) into the chosen layer. The map is a
//! Tiled file: saving the scene writes the tiles back into it and leaves
//! everything else in the file as it was.

use crate::egui;
use orst::components::{read, TilemapRenderer, TILEMAP_RENDERER};
use orst::ecs::Entity;
use orst::tilemap::Orientation;
use orst::{Engine, Rect, Vec2};
use std::collections::HashMap;

#[derive(Default)]
pub struct TilePaint {
    pub on: bool,
    pub erase: bool,
    pub layer: usize,
    /// The tile id (gid) to paint
    pub tile: u32,
    /// Tiles of each painted map as its file holds them, by map id
    saved: HashMap<u32, Vec<Vec<u32>>>,
    /// Tileset images for the palette, by texture id and generation
    palettes: HashMap<(u32, u64), egui::TextureHandle>,
    /// Where the stroke was last painted, so a fast drag leaves no gaps
    last: Option<Vec2>,
}

/// The map the entity draws, if it is loaded
pub fn map_of(engine: &Engine, e: Entity) -> Option<u32> {
    let r: TilemapRenderer = read(&engine.world, e, TILEMAP_RENDERER)?;
    engine.tilemap(r.map).map(|_| r.map)
}

/// The world rectangle of the cell under `at`, if it is on the map
pub fn cell_rect(engine: &Engine, map: u32, at: Vec2) -> Option<Rect> {
    let m = engine.tilemap(map)?;
    let (x, y) = m.tile_at(at);
    if x < 0 || y < 0 || x as u32 >= m.width || y as u32 >= m.height {
        return None;
    }
    let (tw, th) = (m.tile_w as f32, m.tile_h as f32);
    let top = m.tile_to_world(x as f32, y as f32);
    Some(match m.orientation {
        Orientation::Orthogonal => Rect::new(top.x, top.y, tw, th),
        // The diamond's bounding box
        Orientation::Isometric => Rect::new(top.x - tw / 2.0, top.y, tw, th),
    })
}

impl TilePaint {
    /// Continue the stroke to `at`, painting every cell on the way;
    /// whether a cell changed
    pub fn stroke(&mut self, engine: &mut Engine, map: u32, at: Vec2) -> bool {
        let Some(m) = engine.tilemap(map) else {
            return false;
        };
        let step = (m.tile_w.min(m.tile_h).max(1) as f32) / 2.0;
        let from = self.last.replace(at).unwrap_or(at);
        let (dx, dy) = (at.x - from.x, at.y - from.y);
        let n = ((dx * dx + dy * dy).sqrt() / step).ceil().max(1.0) as u32;
        let mut changed = false;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            changed |= self.paint(engine, map, Vec2::new(from.x + dx * t, from.y + dy * t));
        }
        changed
    }

    /// The stroke ended
    pub fn end_stroke(&mut self) {
        self.last = None;
    }

    /// Put the brush's tile at `at`; whether a cell changed
    pub fn paint(&mut self, engine: &mut Engine, map: u32, at: Vec2) -> bool {
        let Some(m) = engine.tilemap(map) else {
            return false;
        };
        let (x, y) = m.tile_at(at);
        let tile = if self.erase { 0 } else { self.tile };
        let Some(layer) = m.layers.get(self.layer) else {
            return false;
        };
        if x < 0 || y < 0 || x as u32 >= layer.width || y as u32 >= layer.height {
            return false;
        }
        if layer.get(x, y) == tile {
            return false;
        }
        if let std::collections::hash_map::Entry::Vacant(v) = self.saved.entry(map) {
            if let Some(tiles) = crate::history::map_tiles(engine, map) {
                v.insert(tiles);
            }
        }
        engine.tilemap_mut(map).unwrap().layers[self.layer].set(x, y, tile);
        true
    }

    /// Maps with tiles their files do not have yet
    pub fn unsaved(&self, engine: &Engine) -> Vec<u32> {
        let mut maps: Vec<u32> = self
            .saved
            .iter()
            .filter(|(id, tiles)| crate::history::map_tiles(engine, **id).as_ref() != Some(tiles))
            .map(|(id, _)| *id)
            .collect();
        maps.sort();
        maps
    }

    /// Write the tiles of every changed map into its file
    pub fn save(&mut self, engine: &Engine) -> Result<(), String> {
        for id in self.unsaved(engine) {
            let (Some(map), Some(path)) = (engine.tilemap(id), engine.tilemap_file(id)) else {
                continue;
            };
            let io = |e: std::io::Error| format!("{}: {e}", path.display());
            let text = std::fs::read_to_string(path).map_err(io)?;
            let text = orst::tiled::with_tiles(&text, map)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            std::fs::write(path, text).map_err(io)?;
            if let Some(tiles) = crate::history::map_tiles(engine, id) {
                self.saved.insert(id, tiles);
            }
        }
        Ok(())
    }

    /// The tools and the palette for `map`
    pub fn ui(&mut self, ui: &mut egui::Ui, engine: &Engine, map: u32) {
        let Some(m) = engine.tilemap(map) else {
            return;
        };
        ui.horizontal(|ui| {
            ui.toggle_value(&mut self.on, "Paint")
                .on_hover_text("Left drag in the Scene window puts tiles (B)");
            ui.toggle_value(&mut self.erase, "Erase");
        });
        if m.layers.is_empty() {
            ui.weak("The map has no tile layers");
            return;
        }
        self.layer = self.layer.min(m.layers.len() - 1);
        egui::ComboBox::from_label("Layer")
            .selected_text(&m.layers[self.layer].name)
            .show_ui(ui, |ui| {
                for (i, l) in m.layers.iter().enumerate() {
                    ui.selectable_value(&mut self.layer, i, &l.name);
                }
            });
        for set in &m.tilesets {
            let Some(texture) = engine.textures.get(set.texture) else {
                continue;
            };
            let key = (set.texture, texture.generation);
            let handle = self.palettes.entry(key).or_insert_with(|| {
                let p = &texture.pixmap;
                let image = egui::ColorImage::from_rgba_premultiplied(
                    [p.width() as usize, p.height() as usize],
                    p.data(),
                );
                ui.ctx()
                    .load_texture("tileset", image, egui::TextureOptions::NEAREST)
            });
            let (iw, ih) = (texture.width() as f32, texture.height() as f32);
            let sheet = set.sheet;
            let cell = 32.0;
            let columns = sheet.columns.max(1);
            ui.label(&set.name);
            egui::Grid::new(("palette", set.first_gid))
                .spacing([2.0, 2.0])
                .show(ui, |ui| {
                    for i in 0..sheet.count {
                        let (cx, cy) = (i % columns, i / columns);
                        let x = sheet.margin + cx * (sheet.frame_w + sheet.spacing);
                        let y = sheet.margin + cy * (sheet.frame_h + sheet.spacing);
                        let uv = egui::Rect::from_min_max(
                            egui::pos2(x as f32 / iw, y as f32 / ih),
                            egui::pos2(
                                (x + sheet.frame_w) as f32 / iw,
                                (y + sheet.frame_h) as f32 / ih,
                            ),
                        );
                        let gid = set.first_gid + i;
                        let image = egui::Image::new((handle.id(), egui::vec2(cell, cell))).uv(uv);
                        let picked = self.tile == gid && !self.erase;
                        if ui
                            .add(egui::Button::image(image).selected(picked))
                            .clicked()
                        {
                            self.tile = gid;
                            self.erase = false;
                            self.on = true;
                        }
                        if cx + 1 == columns {
                            ui.end_row();
                        }
                    }
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orst::tilemap::Tilemap;

    #[test]
    fn strokes_paint_and_erase_cells() {
        let mut engine = Engine::headless(8, 8).unwrap();
        let mut map = Tilemap::new(Orientation::Orthogonal, 4, 3, 16, 16);
        map.add_layer("Ground");
        let id = engine.add_tilemap(map);
        let mut paint = TilePaint {
            tile: 3,
            ..Default::default()
        };
        assert!(paint.paint(&mut engine, id, Vec2::new(20.0, 40.0)));
        assert!(
            !paint.paint(&mut engine, id, Vec2::new(30.0, 36.0)),
            "same cell"
        );
        assert!(
            !paint.paint(&mut engine, id, Vec2::new(-1.0, 0.0)),
            "off the map"
        );
        assert_eq!(engine.tilemap(id).unwrap().layers[0].get(1, 2), 3);
        assert_eq!(paint.unsaved(&engine), [id]);
        paint.erase = true;
        assert!(paint.paint(&mut engine, id, Vec2::new(20.0, 40.0)));
        assert!(paint.unsaved(&engine).is_empty(), "back as the file has it");

        // A fast drag from one end of the row to the other fills it
        paint.erase = false;
        paint.stroke(&mut engine, id, Vec2::new(1.0, 1.0));
        paint.stroke(&mut engine, id, Vec2::new(63.0, 1.0));
        paint.end_stroke();
        let row: Vec<u32> = (0..4)
            .map(|x| engine.tilemap(id).unwrap().layers[0].get(x, 0))
            .collect();
        assert_eq!(row, [3, 3, 3, 3]);
        let cell = cell_rect(&engine, id, Vec2::new(20.0, 40.0)).unwrap();
        assert_eq!(cell, Rect::new(16.0, 32.0, 16.0, 16.0));
        assert_eq!(cell_rect(&engine, id, Vec2::new(100.0, 0.0)), None);
    }
}
