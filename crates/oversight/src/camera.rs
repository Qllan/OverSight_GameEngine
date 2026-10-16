//! Camera: which part of the world is on screen.

use crate::types::Vec2;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// World point shown at the centre of the screen
    pub center: Vec2,
    /// Screen pixels per world unit
    pub zoom: f32,
    /// Radians, clockwise
    pub rotation: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            center: Vec2::default(),
            zoom: 1.0,
            rotation: 0.0,
        }
    }
}

impl Camera {
    /// World -> screen transform for a screen of `size` pixels
    pub fn transform(&self, size: (u32, u32)) -> tiny_skia::Transform {
        let (w, h) = (size.0 as f32, size.1 as f32);
        tiny_skia::Transform::from_translate(w / 2.0, h / 2.0)
            .pre_concat(tiny_skia::Transform::from_rotate(
                self.rotation.to_degrees(),
            ))
            .pre_scale(self.zoom, self.zoom)
            .pre_translate(-self.center.x, -self.center.y)
    }

    pub fn world_to_screen(&self, p: Vec2, size: (u32, u32)) -> Vec2 {
        let mut pt = tiny_skia::Point::from_xy(p.x, p.y);
        self.transform(size).map_point(&mut pt);
        Vec2::new(pt.x, pt.y)
    }

    pub fn screen_to_world(&self, p: Vec2, size: (u32, u32)) -> Vec2 {
        let inverse = self.transform(size).invert().unwrap_or_default();
        let mut pt = tiny_skia::Point::from_xy(p.x, p.y);
        inverse.map_point(&mut pt);
        Vec2::new(pt.x, pt.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_screen_round_trip() {
        let camera = Camera {
            center: Vec2::new(100.0, 50.0),
            zoom: 2.0,
            rotation: 0.3,
        };
        let size = (320, 180);
        let close = |a: Vec2, b: Vec2| (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3;
        let center = camera.world_to_screen(Vec2::new(100.0, 50.0), size);
        assert!(close(center, Vec2::new(160.0, 90.0)), "{:?}", center);
        let p = Vec2::new(123.0, 77.0);
        let back = camera.screen_to_world(camera.world_to_screen(p, size), size);
        assert!(close(back, p), "{:?}", back);
        let unrotated = Camera {
            rotation: 0.0,
            ..camera
        };
        assert_eq!(
            unrotated.world_to_screen(Vec2::new(110.0, 50.0), size),
            Vec2::new(180.0, 90.0)
        );
    }
}
