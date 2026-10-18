//! Plain data types shared with C, C++ and C# (all `#[repr(C)]`).

/// RGBA colour, 0-255 per channel
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const WHITE: Color = Color::rgba(255, 255, 255, 255);
    pub const BLACK: Color = Color::rgba(0, 0, 0, 255);
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }

    pub fn to_skia(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.r, self.g, self.b, self.a)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }
}

/// Axis-aligned rectangle: top-left corner and size
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// Strict overlap (touching edges do not count)
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// The overlapping part of two rectangles
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        if !self.overlaps(other) {
            return None;
        }
        let (x, y) = (self.x.max(other.x), self.y.max(other.y));
        let (r, b) = (
            self.right().min(other.right()),
            self.bottom().min(other.bottom()),
        );
        Some(Rect::new(x, y, r - x, b - y))
    }
}

/// How a sprite is drawn. `src` is the texture region (w or h <= 0: the
/// whole texture); the sprite's `origin` (in source pixels) lands on
/// `pos`, and is the pivot for rotation and scale.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sprite {
    pub src: Rect,
    pub pos: Vec2,
    pub origin: Vec2,
    pub scale: Vec2,
    /// Radians, clockwise
    pub rotation: f32,
    pub flip_x: bool,
    pub flip_y: bool,
    pub tint: Color,
    /// Draw order: lower layers first
    pub layer: i32,
    /// Order within a layer (use the sprite's y for y-sorting)
    pub depth: f32,
}

impl Default for Sprite {
    fn default() -> Self {
        Sprite {
            src: Rect::default(),
            pos: Vec2::default(),
            origin: Vec2::default(),
            scale: Vec2::new(1.0, 1.0),
            rotation: 0.0,
            flip_x: false,
            flip_y: false,
            tint: Color::WHITE,
            layer: 0,
            depth: 0.0,
        }
    }
}

/// Coordinate space of draw calls
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Space {
    /// Transformed by the camera
    #[default]
    World = 0,
    /// Screen pixels (UI)
    Screen = 1,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_overlap_and_intersection() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(6.0, -2.0, 10.0, 5.0);
        assert_eq!(a.intersection(&b), Some(Rect::new(6.0, 0.0, 4.0, 3.0)));
        let touching = Rect::new(10.0, 0.0, 5.0, 5.0);
        assert!(!a.overlaps(&touching));
        assert_eq!(a.intersection(&touching), None);
    }
}
