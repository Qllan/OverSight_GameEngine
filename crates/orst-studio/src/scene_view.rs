//! The Scene window: the edited world drawn by the engine itself, on the
//! editor's GPU device, with a grid, markers for entities that draw
//! nothing, the selection outlined and its gizmo. Middle or right drag
//! pans, the wheel zooms around the cursor, a click selects, a left drag
//! works the gizmo or moves what it starts on. Assets dropped from the
//! Project window land where they are let go.

use crate::egui;
use crate::gizmo::{self, Drag, Tool};
use orst::components::{read, Name, SpriteRenderer, Transform, NAME, SPRITE_RENDERER, TRANSFORM};
use orst::ecs::Entity;
use orst::ffi::OrstEngine;
use orst::{Camera, Color, Engine, Vec2};

const BACKGROUND: Color = Color::rgba(38, 40, 46, 255);
const GRID: Color = Color::rgba(56, 59, 67, 255);
const AXIS_X: Color = Color::rgba(150, 60, 60, 255);
const AXIS_Y: Color = Color::rgba(60, 140, 60, 255);
const MARKER: Color = Color::rgba(200, 200, 210, 255);
const SELECTED: Color = Color::rgba(255, 170, 40, 255);
/// Screen pixels around a marker that still pick it
const MARKER_PICK: f32 = 8.0;
const MIN_ZOOM: f32 = 1.0 / 64.0;
const MAX_ZOOM: f32 = 64.0;

pub struct SceneView {
    /// Holds the edited world. Boxed with a C handle, so C# scripts
    /// work on it through the C API.
    pub engine: Box<OrstEngine>,
    pub tool: Tool,
    /// The editor's own camera; the game's camera is the engine's
    pub camera: Camera,
    /// The entity being dragged, and how
    drag: Option<(Entity, Drag)>,
    shown: Shown,
    /// Left drags paint tiles instead of selecting and moving
    pub painting: bool,
    /// The world point under the pointer, if it is over the view
    pub hover: Option<Vec2>,
    /// A rectangle to outline: the cell the brush would paint
    pub brush: Option<orst::Rect>,
}

/// An asset path under Assets, dragged out of the Project window
pub struct AssetPath(pub String);

/// What happened in the view this frame
#[derive(Debug, Clone, PartialEq)]
pub enum SceneEvent {
    /// A click on an entity or on empty space
    Pick(Option<Entity>),
    /// A gizmo drag gave the entity this transform
    Transform(Entity, Transform),
    /// An asset was dropped at this world point
    Drop(String, Vec2),
    /// Paint at this world point (every frame the button is held)
    Paint(Vec2),
    /// The paint stroke ended
    PaintEnd,
}

/// How the canvas reaches egui
enum Shown {
    /// The engine draws on egui's own device; the texture is registered
    /// as it is, and again after a resize makes a new one
    Gpu {
        render_state: eframe::egui_wgpu::RenderState,
        id: Option<(egui::TextureId, (u32, u32))>,
    },
    /// No GPU to share: the software canvas is copied every frame
    Software(Option<egui::TextureHandle>),
}

impl SceneView {
    pub fn new(wgpu: Option<&eframe::egui_wgpu::RenderState>) -> Result<SceneView, String> {
        let (engine, shown) = match wgpu {
            Some(rs) => (
                Engine::on_device(64, 64, rs.device.clone(), rs.queue.clone())?,
                Shown::Gpu {
                    render_state: rs.clone(),
                    id: None,
                },
            ),
            None => (Engine::headless(64, 64)?, Shown::Software(None)),
        };
        let mut view = SceneView {
            engine: OrstEngine::new(engine),
            tool: Tool::default(),
            camera: Camera::default(),
            drag: None,
            shown,
            painting: false,
            hover: None,
            brush: None,
        };
        view.engine.set_camera(Camera::default());
        Ok(view)
    }

    /// The canvas filling `ui`, sized to it in physical pixels
    fn canvas(
        &mut self,
        ui: &mut egui::Ui,
        sense: egui::Sense,
    ) -> (egui::Rect, egui::Response, (u32, u32)) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), sense);
        let ppp = ui.ctx().pixels_per_point();
        let size = (
            (rect.width() * ppp).round().max(1.0) as u32,
            (rect.height() * ppp).round().max(1.0) as u32,
        );
        if self.engine.size() != size {
            self.engine.resize_canvas(size.0, size.1);
        }
        (rect, response, size)
    }

    /// Show the last frame in `rect`, with the engine's error if any
    fn present(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        if let Some(id) = self.texture(ui.ctx()) {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(id, rect, uv, egui::Color32::WHITE);
        }
        if let Some(e) = self.engine.take_error() {
            ui.painter().text(
                rect.left_top() + egui::vec2(8.0, 8.0),
                egui::Align2::LEFT_TOP,
                e,
                egui::FontId::proportional(14.0),
                ui.visuals().error_fg_color,
            );
        }
    }

    /// The Game window: the world as the game sees it, through `camera`
    /// The game through `camera`; `playing`: keys and the mouse go to it
    pub fn game_ui(&mut self, ui: &mut egui::Ui, camera: Camera, playing: bool) {
        let (rect, response, _) = self.canvas(ui, egui::Sense::click());
        if response.clicked() {
            // Away from a button, which Space would press
            response.request_focus();
        }
        if playing {
            crate::game_input::feed(ui, rect, &mut self.engine.input);
        }
        self.engine.set_camera(camera);
        self.engine.begin_frame(Color::BLACK);
        self.engine.draw_entities();
        self.engine.end_frame();
        self.present(ui, rect);
    }

    /// The view filling `ui`
    pub fn ui(&mut self, ui: &mut egui::Ui, selected: Option<Entity>) -> Vec<SceneEvent> {
        let (rect, response, size) = self.canvas(ui, egui::Sense::click_and_drag());
        let ppp = ui.ctx().pixels_per_point();
        let to_canvas =
            |p: egui::Pos2| Vec2::new((p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp);
        let mut events = Vec::new();

        let mut camera = self.camera;
        if response.dragged_by(egui::PointerButton::Middle)
            || response.dragged_by(egui::PointerButton::Secondary)
        {
            let d = response.drag_delta() * ppp;
            camera = pan(camera, Vec2::new(d.x, d.y));
        }
        if let Some(pointer) = response.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                camera = zoom_at(camera, to_canvas(pointer), size, (scroll * 0.002).exp());
            }
        }
        self.camera = camera;
        self.engine.set_camera(camera);

        if !ui.ctx().egui_wants_keyboard_input() {
            ui.input(|i| {
                for (key, tool) in [
                    (egui::Key::W, Tool::Move),
                    (egui::Key::E, Tool::Rotate),
                    (egui::Key::R, Tool::Scale),
                ] {
                    if i.key_pressed(key) && i.modifiers.is_none() {
                        self.tool = tool;
                    }
                }
            });
        }

        self.hover = response
            .hover_pos()
            .map(|p| self.engine.screen_to_world(to_canvas(p)));
        let held = ui.input(|i| i.pointer.primary_down());
        if self.painting {
            if response.is_pointer_button_down_on() && held {
                if let Some(p) = response.interact_pointer_pos() {
                    events.push(SceneEvent::Paint(self.engine.screen_to_world(to_canvas(p))));
                }
            } else if ui.input(|i| i.pointer.primary_released()) {
                events.push(SceneEvent::PaintEnd);
            }
        } else if response.clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                let world = self.engine.screen_to_world(to_canvas(p));
                events.push(SceneEvent::Pick(pick(&self.engine, world)));
            }
        }
        if response.drag_started_by(egui::PointerButton::Primary) && !self.painting {
            let origin = ui.input(|i| i.pointer.press_origin());
            if let Some(origin) = origin {
                self.drag = self.start_drag(to_canvas(origin), selected, &mut events);
            }
        }
        if let (Some((e, drag)), Some(p)) = (self.drag, response.interact_pointer_pos()) {
            if response.dragged_by(egui::PointerButton::Primary) {
                let at = self.engine.screen_to_world(to_canvas(p));
                let snap = ui
                    .input(|i| i.modifiers.command)
                    .then(|| gizmo::move_snap(camera.zoom));
                events.push(SceneEvent::Transform(e, gizmo::apply(&drag, at, snap)));
            }
        }
        if !response.dragged() {
            self.drag = None;
        }
        if let Some(asset) = response.dnd_release_payload::<AssetPath>() {
            if let Some(p) = response.hover_pos() {
                let at = self.engine.screen_to_world(to_canvas(p));
                events.push(SceneEvent::Drop(asset.0.clone(), at));
            }
        }

        let gizmo = selected.map(|_| (self.tool, self.drag.map(|(_, d)| d.handle)));
        let gizmo = gizmo.filter(|_| !self.painting);
        let brush = self.brush.filter(|_| self.painting);
        draw(&mut self.engine, selected, gizmo, brush);
        self.present(ui, rect);
        events
    }

    /// A left drag from the canvas point `at`: a handle of the selection's
    /// gizmo, or else (with the move tool) whatever entity it starts on
    fn start_drag(
        &self,
        at: Vec2,
        selected: Option<Entity>,
        events: &mut Vec<SceneEvent>,
    ) -> Option<(Entity, Drag)> {
        let start = self.engine.screen_to_world(at);
        let world = &self.engine.world;
        if let Some(e) = selected {
            if let Some(t) = read::<Transform>(world, e, TRANSFORM) {
                if let Some(handle) = gizmo::hit(&self.engine, self.tool, &t, at) {
                    let drag = Drag {
                        tool: self.tool,
                        handle,
                        start,
                        from: t,
                    };
                    return Some((e, drag));
                }
            }
        }
        if self.tool != Tool::Move {
            return None;
        }
        let e = pick(&self.engine, start)?;
        let t = read::<Transform>(world, e, TRANSFORM)?;
        if Some(e) != selected {
            events.push(SceneEvent::Pick(Some(e)));
        }
        let drag = Drag {
            tool: Tool::Move,
            handle: gizmo::Handle::Center,
            start,
            from: t,
        };
        Some((e, drag))
    }

    /// The egui texture showing the last frame
    fn texture(&mut self, ctx: &egui::Context) -> Option<egui::TextureId> {
        match &mut self.shown {
            Shown::Gpu { render_state, id } => {
                let (texture, view) = self.engine.gpu_canvas()?;
                let size = (texture.width(), texture.height());
                let mut renderer = render_state.renderer.write();
                match id {
                    Some((tex, shown)) if *shown == size => {}
                    Some((tex, shown)) => {
                        // A resize made a new texture; keep the same id
                        renderer.update_egui_texture_from_wgpu_texture(
                            &render_state.device,
                            view,
                            orst::wgpu::FilterMode::Nearest,
                            *tex,
                        );
                        *shown = size;
                    }
                    None => {
                        let tex = renderer.register_native_texture(
                            &render_state.device,
                            view,
                            orst::wgpu::FilterMode::Nearest,
                        );
                        *id = Some((tex, size));
                    }
                }
                id.map(|(tex, _)| tex)
            }
            Shown::Software(handle) => {
                let canvas = &self.engine.renderer.canvas;
                let image = egui::ColorImage::from_rgba_premultiplied(
                    [canvas.width() as usize, canvas.height() as usize],
                    canvas.data(),
                );
                let options = egui::TextureOptions::NEAREST;
                match handle {
                    Some(h) => h.set(image, options),
                    None => *handle = Some(ctx.load_texture("scene", image, options)),
                }
                handle.as_ref().map(|h| h.id())
            }
        }
    }
}

/// Move the camera with the cursor by `delta` canvas pixels
pub fn pan(mut camera: Camera, delta: Vec2) -> Camera {
    let (s, c) = (-camera.rotation).sin_cos();
    let d = Vec2::new(delta.x * c - delta.y * s, delta.x * s + delta.y * c);
    camera.center.x -= d.x / camera.zoom;
    camera.center.y -= d.y / camera.zoom;
    camera
}

/// Zoom by `factor`, keeping the world point under `at` where it is
pub fn zoom_at(camera: Camera, at: Vec2, size: (u32, u32), factor: f32) -> Camera {
    let before = camera.screen_to_world(at, size);
    let mut zoomed = camera;
    zoomed.zoom = (camera.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    let after = zoomed.screen_to_world(at, size);
    zoomed.center.x += before.x - after.x;
    zoomed.center.y += before.y - after.y;
    zoomed
}

/// World units between grid lines: a power of ten at least 16 pixels apart
pub fn grid_step(zoom: f32) -> f32 {
    10f32.powf((16.0 / zoom).log10().ceil()).max(1.0)
}

/// The corners of the entity's sprite in the world, clockwise from the
/// sprite's top left; None for entities that draw nothing
pub fn sprite_corners(engine: &Engine, e: Entity) -> Option<[Vec2; 4]> {
    let t: Transform = read(&engine.world, e, TRANSFORM)?;
    let s: SpriteRenderer = read(&engine.world, e, SPRITE_RENDERER)?;
    let (w, h) = if s.src.w > 0.0 && s.src.h > 0.0 {
        (s.src.w, s.src.h)
    } else {
        let (w, h) = engine.texture_size(s.texture)?;
        (w as f32, h as f32)
    };
    let (sin, cos) = t.rotation.sin_cos();
    let corner = |x: f32, y: f32| {
        let (lx, ly) = ((x - s.origin.x) * t.scale.x, (y - s.origin.y) * t.scale.y);
        Vec2::new(t.pos.x + lx * cos - ly * sin, t.pos.y + lx * sin + ly * cos)
    };
    Some([
        corner(0.0, 0.0),
        corner(w, 0.0),
        corner(w, h),
        corner(0.0, h),
    ])
}

/// The topmost entity at `world`: sprites by layer, then the markers of
/// entities that draw nothing; the newest wins a tie
pub fn pick(engine: &Engine, world: Vec2) -> Option<Entity> {
    let reach = MARKER_PICK / engine.camera().zoom;
    let mut best: Option<(i32, Entity)> = None;
    for e in engine.world.query(&[TRANSFORM]) {
        let layer = match sprite_corners(engine, e) {
            Some(corners) if inside(&corners, world) => {
                read::<SpriteRenderer>(&engine.world, e, SPRITE_RENDERER).map_or(0, |s| s.layer)
            }
            Some(_) => continue,
            None => {
                let Some(t) = read::<Transform>(&engine.world, e, TRANSFORM) else {
                    continue;
                };
                if (t.pos.x - world.x).hypot(t.pos.y - world.y) > reach {
                    continue;
                }
                i32::MAX
            }
        };
        if best.is_none_or(|(l, _)| layer >= l) {
            best = Some((layer, e));
        }
    }
    best.map(|(_, e)| e)
}

/// Is `p` inside the convex quad (either winding)?
fn inside(quad: &[Vec2; 4], p: Vec2) -> bool {
    let mut sign = 0.0f32;
    for i in 0..4 {
        let (a, b) = (quad[i], quad[(i + 1) % 4]);
        let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        if cross != 0.0 {
            if sign != 0.0 && cross.signum() != sign {
                return false;
            }
            sign = cross.signum();
        }
    }
    true
}

/// One frame of the edited world; `gizmo`: the tool for the selection and
/// the handle being dragged
pub fn draw(
    engine: &mut Engine,
    selected: Option<Entity>,
    gizmo: Option<(Tool, Option<gizmo::Handle>)>,
    brush: Option<orst::Rect>,
) {
    engine.begin_frame(BACKGROUND);
    let camera = engine.camera();
    let px = 1.0 / camera.zoom;
    let area = camera.visible_world(engine.size());

    engine.set_layer(i32::MIN);
    let step = grid_step(camera.zoom);
    let mut x = (area.x / step).floor() * step;
    while x <= area.x + area.w {
        engine.draw_line(
            Vec2::new(x, area.y),
            Vec2::new(x, area.y + area.h),
            px,
            GRID,
        );
        x += step;
    }
    let mut y = (area.y / step).floor() * step;
    while y <= area.y + area.h {
        engine.draw_line(
            Vec2::new(area.x, y),
            Vec2::new(area.x + area.w, y),
            px,
            GRID,
        );
        y += step;
    }
    engine.draw_line(
        Vec2::new(area.x, 0.0),
        Vec2::new(area.x + area.w, 0.0),
        px,
        AXIS_X,
    );
    engine.draw_line(
        Vec2::new(0.0, area.y),
        Vec2::new(0.0, area.y + area.h),
        px,
        AXIS_Y,
    );

    engine.set_layer(0);
    // Animated sprites show their clip's frame, not the whole sheet
    engine.animate_sprites(0.0);
    engine.draw_entities();

    engine.set_layer(i32::MAX);
    for e in engine.world.query(&[TRANSFORM]) {
        let color = if Some(e) == selected {
            SELECTED
        } else {
            MARKER
        };
        match sprite_corners(engine, e) {
            Some(corners) => {
                if Some(e) == selected {
                    for i in 0..4 {
                        engine.draw_line(corners[i], corners[(i + 1) % 4], 2.0 * px, color);
                    }
                }
            }
            None => {
                let Some(t) = read::<Transform>(&engine.world, e, TRANSFORM) else {
                    continue;
                };
                let r = 6.0 * px;
                engine.draw_circle(t.pos, r, 1.5 * px, color);
                engine.draw_line(
                    Vec2::new(t.pos.x - r, t.pos.y),
                    Vec2::new(t.pos.x + r, t.pos.y),
                    px,
                    color,
                );
                engine.draw_line(
                    Vec2::new(t.pos.x, t.pos.y - r),
                    Vec2::new(t.pos.x, t.pos.y + r),
                    px,
                    color,
                );
            }
        }
    }
    if let (Some(e), Some((tool, active))) = (selected, gizmo) {
        if let Some(t) = read::<Transform>(&engine.world, e, TRANSFORM) {
            gizmo::draw(engine, tool, &t, active);
        }
    }
    if let Some(r) = brush {
        let corners = [
            Vec2::new(r.x, r.y),
            Vec2::new(r.x + r.w, r.y),
            Vec2::new(r.x + r.w, r.y + r.h),
            Vec2::new(r.x, r.y + r.h),
        ];
        for i in 0..4 {
            engine.draw_line(corners[i], corners[(i + 1) % 4], 2.0 * px, SELECTED);
        }
    }
    engine.set_layer(0);
    engine.end_frame();
}

/// What the Hierarchy shows for an entity
pub fn label(engine: &Engine, e: Entity) -> String {
    match read::<Name>(&engine.world, e, NAME) {
        Some(n) if !n.as_str().is_empty() => n.as_str().to_string(),
        _ => format!("Entity {}", e & 0xffff_ffff),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orst::components::write;
    use orst::Rect;

    fn sprite_at(engine: &mut Engine, pos: Vec2, layer: i32) -> Entity {
        let e = engine.world.spawn();
        let t = Transform {
            pos,
            ..Default::default()
        };
        write(&mut engine.world, e, TRANSFORM, &t);
        let s = SpriteRenderer {
            src: Rect::new(0.0, 0.0, 20.0, 10.0),
            origin: Vec2::new(10.0, 5.0),
            layer,
            ..Default::default()
        };
        write(&mut engine.world, e, SPRITE_RENDERER, &s);
        e
    }

    #[test]
    fn clicks_pick_the_topmost_entity() {
        let mut engine = Engine::headless(200, 100).unwrap();
        let back = sprite_at(&mut engine, Vec2::new(0.0, 0.0), 1);
        let front = sprite_at(&mut engine, Vec2::new(5.0, 0.0), 2);
        let marker = engine.world.spawn();
        let t = Transform {
            pos: Vec2::new(40.0, 0.0),
            ..Default::default()
        };
        write(&mut engine.world, marker, TRANSFORM, &t);
        assert_eq!(pick(&engine, Vec2::new(-8.0, 0.0)), Some(back));
        assert_eq!(
            pick(&engine, Vec2::new(4.0, 0.0)),
            Some(front),
            "higher layer wins"
        );
        assert_eq!(
            pick(&engine, Vec2::new(-8.0, 6.0)),
            None,
            "outside the sprite"
        );
        assert_eq!(pick(&engine, Vec2::new(45.0, 3.0)), Some(marker));

        // A sprite turned a quarter: 10 wide and 20 tall now
        let mut t: Transform = read(&engine.world, back, TRANSFORM).unwrap();
        t.rotation = std::f32::consts::FRAC_PI_2;
        write(&mut engine.world, back, TRANSFORM, &t);
        assert_eq!(pick(&engine, Vec2::new(-3.0, -9.0)), Some(back));
        assert_eq!(pick(&engine, Vec2::new(-8.0, 0.0)), None);
        draw(&mut engine, Some(back), Some((Tool::Rotate, None)), None);
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor() {
        let size = (800, 600);
        let camera = Camera {
            center: Vec2::new(30.0, -20.0),
            zoom: 2.0,
            rotation: 0.4,
        };
        let at = Vec2::new(100.0, 450.0);
        let before = camera.screen_to_world(at, size);
        let zoomed = zoom_at(camera, at, size, 1.7);
        let after = zoomed.screen_to_world(at, size);
        assert!((zoomed.zoom - 3.4).abs() < 1e-5);
        assert!((before.x - after.x).abs() < 1e-3 && (before.y - after.y).abs() < 1e-3);
        assert_eq!(zoom_at(camera, at, size, 1e9).zoom, MAX_ZOOM);

        // Panning drags the world with the cursor
        let moved = pan(camera, Vec2::new(10.0, 0.0));
        let p = moved.world_to_screen(before, size);
        assert!((p.x - at.x - 10.0).abs() < 1e-3 && (p.y - at.y).abs() < 1e-3);
    }

    #[test]
    fn grid_lines_stay_readable_at_any_zoom() {
        assert_eq!(grid_step(1.0), 100.0);
        assert_eq!(grid_step(2.0), 10.0);
        assert_eq!(grid_step(16.0), 1.0);
        assert_eq!(grid_step(0.1), 1000.0);
    }
}
