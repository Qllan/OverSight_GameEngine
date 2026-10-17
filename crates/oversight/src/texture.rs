//! Textures: decoded images kept as premultiplied pixmaps.

use std::collections::HashMap;

use tiny_skia::{FilterQuality, Pixmap};

use crate::types::Color;

static NEXT_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub struct Texture {
    pub pixmap: Pixmap,
    /// Smooth (bilinear) instead of crisp (nearest) when scaled
    pub smooth: bool,
    /// Tinted copies, made on first use
    tinted: HashMap<(u8, u8, u8), Pixmap>,
    /// Unique per image, so a GPU copy can tell a reloaded or reused id
    /// from the image it uploaded
    pub generation: u64,
    /// A render target: draw calls can go into it
    pub target: bool,
}

impl Texture {
    pub fn from_pixmap(pixmap: Pixmap) -> Texture {
        Texture {
            pixmap,
            smooth: false,
            tinted: HashMap::new(),
            generation: NEXT_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            target: false,
        }
    }

    /// A transparent render target
    pub fn target(width: u32, height: u32) -> Result<Texture, String> {
        let pixmap = Pixmap::new(width, height).ok_or("invalid target size")?;
        Ok(Texture {
            target: true,
            ..Texture::from_pixmap(pixmap)
        })
    }

    /// The pixels were drawn over: drop copies made from the old ones
    pub fn changed(&mut self) {
        self.tinted.clear();
    }

    /// Decode PNG or JPEG bytes
    pub fn decode(bytes: &[u8]) -> Result<Texture, String> {
        let image = image::load_from_memory(bytes)
            .map_err(|e| e.to_string())?
            .to_rgba8();
        Texture::from_rgba(image.width(), image.height(), image.as_raw())
    }

    pub fn load(path: &str) -> Result<Texture, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {}", path, e))?;
        Texture::decode(&bytes).map_err(|e| format!("{}: {}", path, e))
    }

    /// From straight (non-premultiplied) RGBA pixels, row by row
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Texture, String> {
        if rgba.len() != (width as usize) * (height as usize) * 4 {
            return Err("pixel data does not match the size".to_string());
        }
        let mut pixmap =
            Pixmap::new(width, height).ok_or_else(|| "invalid texture size".to_string())?;
        for (dst, src) in pixmap
            .data_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(rgba.as_chunks::<4>().0)
        {
            let a = src[3] as u16;
            dst[0] = ((src[0] as u16 * a + 127) / 255) as u8;
            dst[1] = ((src[1] as u16 * a + 127) / 255) as u8;
            dst[2] = ((src[2] as u16 * a + 127) / 255) as u8;
            dst[3] = src[3];
        }
        Ok(Texture::from_pixmap(pixmap))
    }

    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    pub fn quality(&self) -> FilterQuality {
        if self.smooth {
            FilterQuality::Bilinear
        } else {
            FilterQuality::Nearest
        }
    }

    /// The pixmap multiplied by a tint colour (alpha is applied when drawing)
    pub fn tinted(&mut self, tint: Color) -> &Pixmap {
        if (tint.r, tint.g, tint.b) == (255, 255, 255) {
            return &self.pixmap;
        }
        let source = &self.pixmap;
        self.tinted
            .entry((tint.r, tint.g, tint.b))
            .or_insert_with(|| {
                let mut copy = source.clone();
                for px in copy.data_mut().as_chunks_mut::<4>().0 {
                    px[0] = ((px[0] as u16 * tint.r as u16 + 127) / 255) as u8;
                    px[1] = ((px[1] as u16 * tint.g as u16 + 127) / 255) as u8;
                    px[2] = ((px[2] as u16 * tint.b as u16 + 127) / 255) as u8;
                }
                copy
            })
    }
}

/// Slot store: ids are index + 1, so 0 is never a valid id
pub struct Store<T> {
    items: Vec<Option<T>>,
}

impl<T> Default for Store<T> {
    fn default() -> Self {
        Store { items: Vec::new() }
    }
}

impl<T> Store<T> {
    pub fn insert(&mut self, item: T) -> u32 {
        if let Some(index) = self.items.iter().position(|i| i.is_none()) {
            self.items[index] = Some(item);
            return index as u32 + 1;
        }
        self.items.push(Some(item));
        self.items.len() as u32
    }

    pub fn get(&self, id: u32) -> Option<&T> {
        self.items.get((id as usize).wrapping_sub(1))?.as_ref()
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut T> {
        self.items.get_mut((id as usize).wrapping_sub(1))?.as_mut()
    }

    /// Ids of the items in the store
    pub fn ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.is_some())
            .map(|(i, _)| i as u32 + 1)
    }

    pub fn remove(&mut self, id: u32) -> Option<T> {
        self.items.get_mut((id as usize).wrapping_sub(1))?.take()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.items.iter_mut().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplies_and_tints() {
        let mut tex = Texture::from_rgba(1, 1, &[200, 100, 50, 128]).unwrap();
        assert_eq!(tex.pixmap.data(), &[100, 50, 25, 128]);
        let red_only = tex.tinted(Color::rgba(255, 0, 0, 255));
        assert_eq!(red_only.data(), &[100, 0, 0, 128]);
        assert!(Texture::from_rgba(2, 2, &[0; 4]).is_err());
    }

    #[test]
    fn store_reuses_free_slots() {
        let mut store = Store::default();
        let a = store.insert("a");
        let b = store.insert("b");
        assert_eq!((a, b), (1, 2));
        assert_eq!(store.get(0), None);
        store.remove(a);
        assert_eq!(store.insert("c"), 1);
        assert_eq!(store.get(2), Some(&"b"));
    }
}
