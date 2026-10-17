//! Debug overlay: an FPS and stats panel plus shapes a game marks for
//! debugging (hitboxes, rays, paths). Everything is drawn on top of the
//! frame, only while the overlay is on; when it is off, debug calls cost
//! nothing more than a check. F3 toggles it by default.

use crate::input::Key;
use crate::render::{DrawKind, Renderer, Shape};
use crate::tilemap::{Orientation, Tilemap};
use crate::types::{Color, Rect, Space, Vec2};

/// Draw layers of the overlay: above anything a game uses
pub const DEBUG_LAYER: i32 = i32::MAX - 1;
const PANEL_LAYER: i32 = i32::MAX;

pub const SOLID_COLOR: Color = Color::rgba(255, 60, 60, 220);
pub const PLATFORM_COLOR: Color = Color::rgba(255, 210, 60, 220);
pub const OBJECT_COLOR: Color = Color::rgba(80, 220, 255, 220);

enum Item {
    Shape(Shape, Color, Space),
    Text(String, Vec2, Color, Space),
}

pub struct DebugOverlay {
    pub enabled: bool,
    /// Key that turns the overlay on and off (None: only from code)
    pub toggle_key: Option<Key>,
    /// Show the stats panel in the top-right corner
    pub show_panel: bool,
    items: Vec<Item>,
}

impl Default for DebugOverlay {
    fn default() -> Self {
        DebugOverlay {
            enabled: false,
            toggle_key: Some(Key::F3),
            show_panel: true,
            items: Vec::new(),
        }
    }
}

/// Numbers shown in the panel
pub struct Stats {
    pub fps: f32,
    pub dt: f32,
    pub commands: usize,
    pub sprites: u32,
    pub camera: Vec2,
    pub mouse_world: Vec2,
    pub renderer: String,
}

impl DebugOverlay {
    pub fn shape(&mut self, shape: Shape, color: Color, space: Space) {
        if self.enabled {
            self.items.push(Item::Shape(shape, color, space));
        }
    }

    pub fn text(&mut self, text: &str, pos: Vec2, color: Color, space: Space) {
        if self.enabled {
            self.items
                .push(Item::Text(text.to_string(), pos, color, space));
        }
    }

    pub fn rect(&mut self, rect: Rect, color: Color, space: Space) {
        // Inset by half a pixel so a 1px outline lands on whole pixels
        let rect = Rect::new(rect.x + 0.5, rect.y + 0.5, rect.w - 1.0, rect.h - 1.0);
        let shape = Shape::Rect {
            rect,
            filled: false,
            thickness: 1.0,
        };
        self.shape(shape, color, space);
    }

    /// Outlines of solid cells, one-way platforms and objects of a map
    /// in the camera's view, with object names
    pub fn tilemap(&mut self, map: &Tilemap, view: Rect) {
        if !self.enabled {
            return;
        }
        let (x0, y0, x1, y1) = map.cell_range(view);
        for y in y0..y1 {
            for x in x0..x1 {
                let color = if map.is_solid(x, y) {
                    SOLID_COLOR
                } else if map.is_platform(x, y) {
                    PLATFORM_COLOR
                } else {
                    continue;
                };
                self.cell(map, x, y, color);
            }
        }
        for layer in &map.object_layers {
            for o in &layer.objects {
                let r = o.rect;
                if o.point || (r.w <= 0.0 && r.h <= 0.0) {
                    let c = Vec2::new(r.x, r.y);
                    self.shape(
                        Shape::Circle {
                            center: c,
                            radius: 3.0,
                            filled: false,
                            thickness: 1.0,
                        },
                        OBJECT_COLOR,
                        Space::World,
                    );
                } else {
                    self.rect(r, OBJECT_COLOR, Space::World);
                }
                if !o.name.is_empty() {
                    let pos = Vec2::new(r.x + 2.0, r.y - 10.0);
                    self.text(&o.name, pos, OBJECT_COLOR, Space::World);
                }
            }
        }
    }

    fn cell(&mut self, map: &Tilemap, x: i32, y: i32, color: Color) {
        match map.orientation {
            Orientation::Orthogonal => {
                let p = map.tile_to_world(x as f32, y as f32);
                let r = Rect::new(p.x, p.y, map.tile_w as f32, map.tile_h as f32);
                self.rect(r, color, Space::World);
            }
            Orientation::Isometric => {
                let (fx, fy) = (x as f32, y as f32);
                let corners = [
                    map.tile_to_world(fx, fy),
                    map.tile_to_world(fx + 1.0, fy),
                    map.tile_to_world(fx + 1.0, fy + 1.0),
                    map.tile_to_world(fx, fy + 1.0),
                ];
                for i in 0..4 {
                    let line = Shape::Line {
                        from: corners[i],
                        to: corners[(i + 1) % 4],
                        thickness: 1.0,
                    };
                    self.shape(line, color, Space::World);
                }
            }
        }
    }

    /// Queue this frame's debug items and the panel, then forget them
    pub fn flush(&mut self, renderer: &mut Renderer, stats: &Stats) {
        let items = std::mem::take(&mut self.items);
        if !self.enabled {
            return;
        }
        for item in items {
            match item {
                Item::Shape(shape, color, space) => {
                    renderer.push(DEBUG_LAYER, 0.0, space, DrawKind::Shape { shape, color })
                }
                Item::Text(text, pos, color, space) => renderer.push(
                    DEBUG_LAYER,
                    0.0,
                    space,
                    DrawKind::Text {
                        font: crate::engine::DEFAULT_FONT_ID,
                        text,
                        pos,
                        size: 8.0,
                        color,
                    },
                ),
            }
        }
        if !self.show_panel {
            return;
        }
        let lines = [
            format!("FPS {:.0}  {:.1} ms", stats.fps, stats.dt * 1000.0),
            format!("draws {}  sprites {}", stats.commands, stats.sprites),
            format!("camera {:.0}, {:.0}", stats.camera.x, stats.camera.y),
            format!(
                "mouse {:.0}, {:.0}",
                stats.mouse_world.x, stats.mouse_world.y
            ),
            stats.renderer.chars().take(22).collect(),
        ];
        let (w, line_h) = (112.0, 10.0);
        let x = renderer.size().0 as f32 - w - 2.0;
        let panel = Rect::new(x, 2.0, w, line_h * lines.len() as f32 + 4.0);
        renderer.push(
            PANEL_LAYER,
            0.0,
            Space::Screen,
            DrawKind::Shape {
                shape: Shape::Rect {
                    rect: panel,
                    filled: true,
                    thickness: 0.0,
                },
                color: Color::rgba(0, 0, 0, 170),
            },
        );
        for (i, line) in lines.into_iter().enumerate() {
            renderer.push(
                PANEL_LAYER,
                1.0,
                Space::Screen,
                DrawKind::Text {
                    font: crate::engine::DEFAULT_FONT_ID,
                    text: line,
                    pos: Vec2::new(x + 3.0, 3.0 + i as f32 * line_h),
                    size: 8.0,
                    color: Color::rgba(180, 255, 180, 255),
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::{Engine, InputEvent};
    use crate::input::Key;
    use crate::types::{Color, Rect, Space, Vec2};

    #[test]
    fn overlay_draws_on_top_only_when_on() {
        let mut e = Engine::headless(160, 90).unwrap();
        e.set_space(Space::Screen);
        let frame = |e: &mut Engine| {
            e.poll();
            e.begin_frame(Color::BLACK);
            e.set_layer(1000);
            e.fill_rect(
                Rect::new(0.0, 0.0, 160.0, 90.0),
                Color::rgba(0, 0, 255, 255),
            );
            e.debug_rect(
                Rect::new(10.0, 10.0, 20.0, 20.0),
                Color::rgba(255, 0, 0, 255),
            );
            e.end_frame();
        };
        frame(&mut e);
        assert_eq!(
            e.pixel(10, 20),
            Some(Color::rgba(0, 0, 255, 255)),
            "off by default"
        );

        e.inject(InputEvent::Key(Key::F3, true));
        frame(&mut e);
        assert!(e.debug.enabled, "F3 turns it on");
        let edge = e.pixel(10, 20).unwrap();
        assert!(
            edge.r > 150 && edge.b < 100,
            "outline over the game: {:?}",
            edge
        );
        // The panel darkens the top-right corner and has text in it
        let corner = e.pixel(150, 5).unwrap();
        assert!(corner.b < 255, "{:?}", corner);

        e.debug.toggle_key = None;
        e.inject(InputEvent::Key(Key::F3, false));
        frame(&mut e);
        e.inject(InputEvent::Key(Key::F3, true));
        frame(&mut e);
        assert!(e.debug.enabled, "no toggle key: F3 does nothing");
        e.set_debug(false);
        frame(&mut e);
        assert_eq!(e.pixel(10, 20), Some(Color::rgba(0, 0, 255, 255)));
        assert_eq!(e.pixel(150, 5), Some(Color::rgba(0, 0, 255, 255)));
    }

    #[test]
    fn tilemap_cells_and_objects_are_outlined() {
        use crate::anim::SpriteSheet;
        use crate::tilemap::{
            MapObject, ObjectLayer, Orientation, Properties, Property, Tilemap, Tileset,
        };
        let mut map = Tilemap::new(Orientation::Orthogonal, 4, 4, 8, 8);
        let mut set = Tileset::new(1, 0, SpriteSheet::grid(8, 8, 8, 8, 0, 0));
        set.tile_properties.insert(
            0,
            Properties::from([("solid".to_string(), Property::Bool(true))]),
        );
        map.add_tileset(set);
        let l = map.add_layer("l");
        map.layers[l].set(1, 1, 1);
        map.object_layers.push(ObjectLayer {
            name: "o".into(),
            objects: vec![MapObject {
                name: "door".into(),
                rect: Rect::new(20.0, 20.0, 8.0, 8.0),
                ..MapObject::default()
            }],
            ..ObjectLayer::default()
        });
        let mut e = Engine::headless(32, 32).unwrap();
        let id = e.add_tilemap(map);
        e.set_camera(crate::camera::Camera {
            center: Vec2::new(16.0, 16.0),
            ..Default::default()
        });
        e.set_debug(true);
        e.debug.show_panel = false;
        e.begin_frame(Color::BLACK);
        e.debug_tilemap(id);
        e.end_frame();
        let solid = e.pixel(8, 12).unwrap();
        assert!(solid.r > 150 && solid.g < 100, "{:?}", solid);
        let door = e.pixel(20, 24).unwrap();
        assert!(door.b > 150 && door.r < 150, "{:?}", door);
        assert_eq!(e.pixel(2, 2), Some(Color::BLACK));
    }
}
