//! The sprite animation editor under the Inspector: the entity's sprite
//! sheet with its frame grid, where a click picks the first frame of the
//! clip and a Shift+click its last, and a preview that plays the clip as
//! the game would. Changes go back as Inspector actions, so they undo.

use crate::egui;
use crate::inspector::Action;
use orst::components::{read, SpriteAnimator, SpriteRenderer, SPRITE_ANIMATOR, SPRITE_RENDERER};
use orst::ecs::Entity;
use orst::schema::Value;
use orst::Engine;
use std::collections::HashMap;

pub struct AnimEdit {
    /// Sheet images for egui, by texture id and generation
    textures: HashMap<(u32, u64), egui::TextureHandle>,
    pub preview: bool,
    /// Columns and rows for Fit
    grid: [u32; 2],
}

impl Default for AnimEdit {
    fn default() -> Self {
        AnimEdit {
            textures: HashMap::new(),
            preview: true,
            grid: [4, 1],
        }
    }
}

/// Columns and rows of whole frames in a `w` x `h` sheet
pub fn sheet_grid(a: &SpriteAnimator, w: u32, h: u32) -> (u32, u32) {
    let columns = match a.columns {
        0 => w / a.frame_w.max(1),
        c => c,
    };
    (columns.max(1), (h / a.frame_h.max(1)).max(1))
}

/// The clip after a click on frame `index`: a plain click starts a clip
/// of one frame there, an extending click stretches it to that frame
pub fn pick(a: &SpriteAnimator, index: u32, extend: bool) -> (u32, u32) {
    if !extend {
        return (index, 1);
    }
    let last = a.first + a.count.max(1) - 1;
    if index >= a.first {
        (a.first, index - a.first + 1)
    } else {
        (index, last - index + 1)
    }
}

fn set(component: u32, field: &str, value: Value) -> Action {
    Action::Set {
        component,
        field: field.into(),
        value,
    }
}

impl AnimEdit {
    /// The editor for `e`, when it has a SpriteAnimator and a sprite sheet
    pub fn ui(&mut self, ui: &mut egui::Ui, engine: &Engine, e: Entity) -> Vec<Action> {
        let mut actions = Vec::new();
        let (Some(a), Some(s)) = (
            read::<SpriteAnimator>(&engine.world, e, SPRITE_ANIMATOR),
            read::<SpriteRenderer>(&engine.world, e, SPRITE_RENDERER),
        ) else {
            return actions;
        };
        let Some(texture) = engine.textures.get(s.texture) else {
            ui.weak("Give the SpriteRenderer a sprite sheet");
            return actions;
        };
        let (w, h) = (texture.width(), texture.height());
        let handle = self
            .textures
            .entry((s.texture, texture.generation))
            .or_insert_with(|| {
                let p = &texture.pixmap;
                let image = egui::ColorImage::from_rgba_premultiplied(
                    [p.width() as usize, p.height() as usize],
                    p.data(),
                );
                ui.ctx()
                    .load_texture("sheet", image, egui::TextureOptions::NEAREST)
            })
            .clone();
        let uv_of = |r: orst::Rect| {
            egui::Rect::from_min_max(
                egui::pos2(r.x / w as f32, r.y / h as f32),
                egui::pos2((r.x + r.w) / w as f32, (r.y + r.h) / h as f32),
            )
        };

        // The clip playing, as the game shows it
        ui.horizontal(|ui| {
            let mut shown = a;
            if self.preview {
                shown.time = ui.input(|i| i.time) as f32;
                ui.ctx().request_repaint();
            }
            let scale = (96.0 / a.frame_w.max(a.frame_h).max(1) as f32).clamp(0.25, 4.0);
            let size = egui::vec2(a.frame_w as f32 * scale, a.frame_h as f32 * scale);
            let image = egui::Image::new((handle.id(), size)).uv(uv_of(shown.src(w)));
            ui.add(image);
            ui.vertical(|ui| {
                ui.toggle_value(&mut self.preview, "Preview");
                ui.label(format!(
                    "Frames {}..{} at {} fps",
                    a.first,
                    a.first + a.count.max(1) - 1,
                    a.fps
                ));
            });
        });

        // Cutting the sheet into a grid
        ui.horizontal(|ui| {
            ui.label("Grid");
            ui.add(egui::DragValue::new(&mut self.grid[0]).range(1..=256));
            ui.label("x");
            ui.add(egui::DragValue::new(&mut self.grid[1]).range(1..=256));
            if ui
                .button("Fit")
                .on_hover_text("Cut the sheet into this many columns and rows")
                .clicked()
            {
                let [c, r] = self.grid;
                actions.push(set(SPRITE_ANIMATOR, "frame_w", Value::U32(w / c)));
                actions.push(set(SPRITE_ANIMATOR, "frame_h", Value::U32(h / r)));
                actions.push(set(SPRITE_ANIMATOR, "columns", Value::U32(c)));
            }
        });

        // The sheet: click picks the first frame, Shift+click the last
        let (columns, rows) = sheet_grid(&a, w, h);
        let scale = (ui.available_width() / w.max(1) as f32).min(4.0);
        let size = egui::vec2(w as f32 * scale, h as f32 * scale);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, egui::Color32::from_gray(40));
        painter.image(
            handle.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        let cell = |i: u32| {
            let (x, y) = (i % columns, i / columns);
            egui::Rect::from_min_size(
                rect.min
                    + egui::vec2(
                        (x * a.frame_w) as f32 * scale,
                        (y * a.frame_h) as f32 * scale,
                    ),
                egui::vec2(a.frame_w as f32 * scale, a.frame_h as f32 * scale),
            )
        };
        let line = egui::Stroke::new(1.0, egui::Color32::from_white_alpha(60));
        for i in 0..columns * rows {
            painter.rect_stroke(cell(i), 0.0, line, egui::StrokeKind::Inside);
        }
        let picked = egui::Color32::from_rgba_unmultiplied(80, 160, 255, 70);
        for i in a.first..a.first + a.count.max(1) {
            painter.rect_filled(cell(i), 0.0, picked);
        }
        let at = |pos: egui::Pos2| {
            let local = (pos - rect.min) / scale;
            let (x, y) = (
                local.x as u32 / a.frame_w.max(1),
                local.y as u32 / a.frame_h.max(1),
            );
            (x < columns && y < rows).then_some(y * columns + x)
        };
        if let Some(i) = response.hover_pos().and_then(at) {
            let hover = egui::Stroke::new(1.5, egui::Color32::YELLOW);
            painter.rect_stroke(cell(i), 0.0, hover, egui::StrokeKind::Inside);
        }
        if response.clicked() {
            if let Some(i) = response.interact_pointer_pos().and_then(at) {
                let extend = ui.input(|i| i.modifiers.shift);
                let (first, count) = pick(&a, i, extend);
                actions.push(set(SPRITE_ANIMATOR, "first", Value::U32(first)));
                actions.push(set(SPRITE_ANIMATOR, "count", Value::U32(count)));
                actions.push(set(SPRITE_ANIMATOR, "time", Value::F32(0.0)));
                // The scene shows the clip's first frame
                let shown = SpriteAnimator {
                    first,
                    time: 0.0,
                    ..a
                };
                let r = shown.src(w);
                actions.push(set(SPRITE_RENDERER, "src", Value::Rect(r)));
            }
        }
        ui.weak("Click: first frame. Shift+click: last frame.");
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_pick_the_clip() {
        let a = SpriteAnimator {
            frame_w: 16,
            frame_h: 16,
            first: 4,
            count: 2,
            ..Default::default()
        };
        assert_eq!(sheet_grid(&a, 64, 40), (4, 2), "whole frames only");
        assert_eq!(pick(&a, 7, false), (7, 1));
        assert_eq!(pick(&a, 7, true), (4, 4));
        assert_eq!(pick(&a, 1, true), (1, 5), "back to before the first");
    }
}
