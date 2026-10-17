//! Hot reload: while it is on, the engine checks the files that textures
//! and Tiled maps were loaded from and reloads the ones that changed, in
//! place: ids stay the same, so the game keeps drawing without knowing.
//! Save a PNG in an image editor or a map in Tiled and see it in the
//! running game.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A file and its modification time when it was last read
#[derive(Debug, Clone)]
pub struct Watched {
    pub path: PathBuf,
    stamp: Option<SystemTime>,
}

impl Watched {
    pub fn new(path: &Path) -> Watched {
        Watched {
            path: path.to_path_buf(),
            stamp: stamp(path),
        }
    }

    /// True once per change; a file that vanished counts as unchanged
    /// (editors often delete and rewrite), it is picked up when it is back
    pub fn changed(&mut self) -> bool {
        let now = stamp(&self.path);
        if now.is_some() && now != self.stamp {
            self.stamp = now;
            true
        } else {
            false
        }
    }
}

fn stamp(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Files of a loaded map, and the textures it loaded for its tilesets
#[derive(Debug, Clone, Default)]
pub struct MapFiles {
    pub files: Vec<Watched>,
    /// Tileset image -> texture id, reused when the map is reloaded
    pub images: BTreeMap<PathBuf, u32>,
}

#[derive(Debug, Clone)]
pub struct HotReload {
    pub enabled: bool,
    /// Seconds between checks of the files
    pub interval: f32,
    pub(crate) timer: f32,
    /// Resources reloaded by the last check in `poll`
    pub reloaded: u32,
    pub(crate) textures: BTreeMap<u32, Watched>,
    pub(crate) maps: BTreeMap<u32, MapFiles>,
}

impl Default for HotReload {
    fn default() -> Self {
        HotReload {
            enabled: false,
            interval: 0.5,
            timer: 0.0,
            reloaded: 0,
            textures: BTreeMap::new(),
            maps: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::Engine;
    use crate::types::Color;
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    /// Rewrite a file and move its time forward, so the change is seen
    /// even on file systems with coarse timestamps
    fn rewrite(path: &Path, bytes: &[u8], step: u64) {
        std::fs::write(path, bytes).unwrap();
        let t = SystemTime::now() + Duration::from_secs(step);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    fn png(color: [u8; 4], w: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, 1, image::Rgba(color));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ov_hot_{}_{}", name, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn changed_texture_is_reloaded_in_place() {
        let dir = temp_dir("tex");
        let path = dir.join("a.png");
        rewrite(&path, &png([255, 0, 0, 255], 1), 0);
        let mut e = Engine::headless(8, 8).unwrap();
        let id = e.load_texture(path.to_str().unwrap()).unwrap();
        e.set_texture_smooth(id, true);
        assert_eq!(e.reload_changed(), 0, "nothing changed yet");

        rewrite(&path, &png([0, 0, 255, 255], 2), 10);
        assert_eq!(e.reload_changed(), 1);
        assert_eq!(e.texture_size(id), Some((2, 1)), "same id, new image");
        assert!(e.textures.get(id).unwrap().smooth, "settings kept");
        assert_eq!(e.reload_changed(), 0, "reloaded once per change");

        // A broken file keeps the old image and reports the error
        rewrite(&path, b"not a png", 20);
        assert_eq!(e.reload_changed(), 0);
        assert_eq!(e.texture_size(id), Some((2, 1)));
        assert!(e.take_error().is_some());

        // poll checks on its own while hot reload is on
        rewrite(&path, &png([0, 255, 0, 255], 3), 30);
        e.hot_reload.enabled = true;
        e.hot_reload.interval = 0.0;
        e.poll();
        assert_eq!(e.hot_reload.reloaded, 1);
        e.set_space(crate::types::Space::Screen);
        e.begin_frame(Color::BLACK);
        e.draw_texture(id, 0.0, 0.0);
        e.end_frame();
        assert_eq!(e.pixel(0, 0), Some(Color::rgba(0, 255, 0, 255)));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn changed_map_is_reloaded_in_place() {
        let dir = temp_dir("map");
        rewrite(&dir.join("t.png"), &png([255, 255, 255, 255], 16), 0);
        let map = |gid: u32| {
            format!(
                r#"{{"orientation": "orthogonal", "width": 2, "height": 1,
                "tilewidth": 8, "tileheight": 8,
                "tilesets": [{{"firstgid": 1, "source": "t.tsj"}}],
                "layers": [{{"type": "tilelayer", "name": "g", "width": 2, "height": 1,
                             "data": [{}, 0]}}]}}"#,
                gid
            )
        };
        let tsj = r#"{"name": "t", "image": "t.png", "imagewidth": 16, "imageheight": 1,
            "tilewidth": 8, "tileheight": 8, "columns": 2, "tilecount": 2}"#;
        rewrite(&dir.join("t.tsj"), tsj.as_bytes(), 0);
        let path = dir.join("m.tmj");
        rewrite(&path, map(1).as_bytes(), 0);

        let mut e = Engine::headless(8, 8).unwrap();
        let id = e.load_tilemap(path.to_str().unwrap()).unwrap();
        let texture = e.tilemap(id).unwrap().tilesets[0].texture;
        assert_eq!(e.tilemap(id).unwrap().files.len(), 2, "map and tileset");

        rewrite(&path, map(2).as_bytes(), 10);
        assert_eq!(e.reload_changed(), 1);
        let m = e.tilemap(id).unwrap();
        assert_eq!(m.layers[0].get(0, 0), 2, "new tiles under the same id");
        assert_eq!(
            m.tilesets[0].texture, texture,
            "tileset image not loaded twice"
        );

        // Changing the external tileset reloads the map too
        rewrite(&dir.join("t.tsj"), tsj.as_bytes(), 20);
        assert_eq!(e.reload_changed(), 1);

        // Freeing the map frees the textures it loaded
        e.free_tilemap(id);
        assert!(e.texture_size(texture).is_none());
        std::fs::remove_dir_all(dir).ok();
    }
}
