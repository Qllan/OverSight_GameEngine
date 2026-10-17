//! Entities and components: the game world as plain data.
//!
//! An entity is an id; a component is a block of plain bytes (a C struct,
//! a C# unmanaged struct) attached to it. Games register their own
//! component types by name and size, and query entities that have a given
//! set of them. Because components hold no pointers, the whole world can be
//! copied byte for byte: saved, rolled back, replayed or sent over the
//! network.
//!
//! Storage is a sparse set per component: the data of all entities with it
//! lies packed in one array, so iterating a component touches memory in
//! order. Every operation is deterministic: the same calls in the same
//! order give the same ids and the same iteration order.

use std::collections::HashMap;

/// `(generation << 32) | index`. Never 0, so 0 means "no entity". A
/// despawned entity's index is reused with the next generation, so an old
/// id held somewhere stops matching instead of pointing at a stranger.
pub type Entity = u64;

/// Largest component (bytes) and alignment the world stores
pub const MAX_COMPONENT_SIZE: u32 = 64 * 1024;
pub const MAX_ALIGN: u32 = 16;

const ABSENT: u32 = u32::MAX;

pub fn entity(index: u32, generation: u32) -> Entity {
    (generation as u64) << 32 | index as u64
}

pub fn entity_index(e: Entity) -> u32 {
    e as u32
}

pub fn entity_generation(e: Entity) -> u32 {
    (e >> 32) as u32
}

/// 16 bytes aligned to 16: the unit component data is allocated in, so a
/// pointer to any row is aligned for any C type up to 16
#[derive(Clone, Copy, Default)]
#[repr(C, align(16))]
struct Block([u8; 16]);

/// Packed rows of one component type
#[derive(Clone)]
pub struct Storage {
    pub name: String,
    pub size: u32,
    pub align: u32,
    /// Bytes from one row to the next: size rounded up to the alignment
    stride: usize,
    data: Vec<Block>,
    /// Entity index of each row
    dense: Vec<u32>,
    /// Row of each entity index, or ABSENT
    sparse: Vec<u32>,
}

impl Storage {
    fn new(name: &str, size: u32, align: u32) -> Storage {
        let stride = (size as usize).div_ceil(align as usize) * align as usize;
        Storage {
            name: name.to_string(),
            size,
            align,
            stride,
            data: Vec::new(),
            dense: Vec::new(),
            sparse: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.dense.len()
    }

    pub fn is_empty(&self) -> bool {
        self.dense.is_empty()
    }

    fn row_of(&self, index: u32) -> Option<usize> {
        match self.sparse.get(index as usize) {
            Some(&row) if row != ABSENT => Some(row as usize),
            _ => None,
        }
    }

    fn bytes(&self) -> &[u8] {
        // Safety: Block is 16 plain bytes with no padding
        unsafe { std::slice::from_raw_parts(self.data.as_ptr() as *const u8, self.data.len() * 16) }
    }

    fn bytes_mut(&mut self) -> &mut [u8] {
        unsafe {
            std::slice::from_raw_parts_mut(self.data.as_mut_ptr() as *mut u8, self.data.len() * 16)
        }
    }

    fn row(&self, row: usize) -> &[u8] {
        &self.bytes()[row * self.stride..row * self.stride + self.size as usize]
    }

    fn row_mut(&mut self, row: usize) -> &mut [u8] {
        let (start, size) = (row * self.stride, self.size as usize);
        &mut self.bytes_mut()[start..start + size]
    }

    /// Add or overwrite the entity's row; `bytes` shorter than the size
    /// leave the rest zero
    fn insert(&mut self, index: u32, bytes: &[u8]) {
        let row = match self.row_of(index) {
            Some(row) => row,
            None => {
                let row = self.dense.len();
                self.dense.push(index);
                if self.sparse.len() <= index as usize {
                    self.sparse.resize(index as usize + 1, ABSENT);
                }
                self.sparse[index as usize] = row as u32;
                let blocks = ((row + 1) * self.stride).div_ceil(16);
                if self.data.len() < blocks {
                    self.data
                        .resize(blocks.max(self.data.len() * 2), Block::default());
                }
                row
            }
        };
        let dest = self.row_mut(row);
        dest.fill(0);
        let n = bytes.len().min(dest.len());
        dest[..n].copy_from_slice(&bytes[..n]);
    }

    /// Swap-remove the entity's row
    fn remove(&mut self, index: u32) -> bool {
        let Some(row) = self.row_of(index) else {
            return false;
        };
        let last = self.dense.len() - 1;
        if row != last {
            let stride = self.stride;
            self.bytes_mut()
                .copy_within(last * stride..(last + 1) * stride, row * stride);
            let moved = self.dense[last];
            self.dense[row] = moved;
            self.sparse[moved as usize] = row as u32;
        }
        self.dense.pop();
        self.sparse[index as usize] = ABSENT;
        true
    }

    fn ptr(&mut self, index: u32) -> Option<*mut u8> {
        let row = self.row_of(index)?;
        let stride = self.stride;
        // A zero-size component still gets a valid, aligned pointer
        if self.data.is_empty() {
            return Some(std::ptr::NonNull::<Block>::dangling().as_ptr() as *mut u8);
        }
        Some(unsafe { (self.data.as_mut_ptr() as *mut u8).add(row * stride) })
    }
}

#[derive(Clone, Default)]
pub struct World {
    /// Generation of each index; the entity is alive when `alive[i]`
    generations: Vec<u32>,
    alive: Vec<bool>,
    /// Indexes free for reuse, the most recently freed last
    free: Vec<u32>,
    count: u32,
    storages: Vec<Storage>,
    by_name: HashMap<String, u32>,
}

impl World {
    pub fn new() -> World {
        World::default()
    }

    /// The id of a component type, registering it on first use. Ids start
    /// at 1 and follow registration order. A name registered again must
    /// have the same size and alignment.
    pub fn component(&mut self, name: &str, size: u32, align: u32) -> Result<u32, String> {
        let align = align.max(1);
        if name.is_empty() {
            return Err("a component needs a name".into());
        }
        if !align.is_power_of_two() || align > MAX_ALIGN {
            return Err(format!(
                "component {name}: alignment {align} is not a power of two up to {MAX_ALIGN}"
            ));
        }
        if size > MAX_COMPONENT_SIZE {
            return Err(format!(
                "component {name}: {size} bytes, at most {MAX_COMPONENT_SIZE}"
            ));
        }
        if let Some(&id) = self.by_name.get(name) {
            let s = &self.storages[id as usize - 1];
            if s.size != size || s.align != align {
                return Err(format!(
                    "component {name} is already {} bytes aligned to {}, not {size} aligned to {align}",
                    s.size, s.align
                ));
            }
            return Ok(id);
        }
        self.storages.push(Storage::new(name, size, align));
        let id = self.storages.len() as u32;
        self.by_name.insert(name.to_string(), id);
        Ok(id)
    }

    pub fn component_id(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).copied()
    }

    pub fn storage(&self, component: u32) -> Option<&Storage> {
        self.storages.get((component as usize).wrapping_sub(1))
    }

    fn storage_mut(&mut self, component: u32) -> Option<&mut Storage> {
        self.storages.get_mut((component as usize).wrapping_sub(1))
    }

    pub fn spawn(&mut self) -> Entity {
        let index = match self.free.pop() {
            Some(i) => i,
            None => {
                self.generations.push(1);
                self.alive.push(false);
                (self.generations.len() - 1) as u32
            }
        };
        self.alive[index as usize] = true;
        self.count += 1;
        entity(index, self.generations[index as usize])
    }

    pub fn alive(&self, e: Entity) -> bool {
        let i = entity_index(e) as usize;
        e != 0 && self.alive.get(i) == Some(&true) && self.generations[i] == entity_generation(e)
    }

    /// Remove the entity and all its components
    pub fn despawn(&mut self, e: Entity) -> bool {
        if !self.alive(e) {
            return false;
        }
        let index = entity_index(e);
        for s in &mut self.storages {
            s.remove(index);
        }
        let i = index as usize;
        self.alive[i] = false;
        // Skip 0 on wrap so the id never becomes 0
        self.generations[i] = self.generations[i].wrapping_add(1).max(1);
        self.free.push(index);
        self.count -= 1;
        true
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    /// Attach or overwrite a component; `bytes` shorter than its size
    /// leave the rest zero (empty bytes give a zeroed component)
    pub fn set(&mut self, e: Entity, component: u32, bytes: &[u8]) -> bool {
        if !self.alive(e) {
            return false;
        }
        match self.storage_mut(component) {
            Some(s) => {
                s.insert(entity_index(e), bytes);
                true
            }
            None => false,
        }
    }

    pub fn has(&self, e: Entity, component: u32) -> bool {
        self.alive(e)
            && self
                .storage(component)
                .and_then(|s| s.row_of(entity_index(e)))
                .is_some()
    }

    pub fn get(&self, e: Entity, component: u32) -> Option<&[u8]> {
        if !self.alive(e) {
            return None;
        }
        let s = self.storage(component)?;
        s.row_of(entity_index(e)).map(|row| s.row(row))
    }

    /// Where the component's bytes live, for C and C# to read and write in
    /// place. Valid until the next spawn, despawn, set or remove.
    pub fn ptr(&mut self, e: Entity, component: u32) -> Option<*mut u8> {
        if !self.alive(e) {
            return None;
        }
        self.storage_mut(component)?.ptr(entity_index(e))
    }

    pub fn remove(&mut self, e: Entity, component: u32) -> bool {
        self.alive(e)
            && self
                .storage_mut(component)
                .is_some_and(|s| s.remove(entity_index(e)))
    }

    /// Entities that have every one of `components`, in the row order of
    /// the smallest of them. No components: every living entity, oldest
    /// index first. An unknown component matches nothing.
    pub fn query(&self, components: &[u32]) -> Vec<Entity> {
        let mut storages = Vec::with_capacity(components.len());
        for &c in components {
            match self.storage(c) {
                Some(s) => storages.push(s),
                None => return Vec::new(),
            }
        }
        let Some(smallest) = storages.iter().min_by_key(|s| s.len()) else {
            return (0..self.alive.len() as u32)
                .filter(|&i| self.alive[i as usize])
                .map(|i| entity(i, self.generations[i as usize]))
                .collect();
        };
        smallest
            .dense
            .iter()
            .filter(|&&i| storages.iter().all(|s| s.row_of(i).is_some()))
            .map(|&i| entity(i, self.generations[i as usize]))
            .collect()
    }

    /// Remove every entity; component types stay registered
    pub fn clear(&mut self) {
        let living: Vec<Entity> = self.query(&[]);
        for e in living {
            self.despawn(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(x: f32, y: f32) -> Vec<u8> {
        [x.to_le_bytes(), y.to_le_bytes()].concat()
    }

    fn read_pos(w: &World, e: Entity, c: u32) -> (f32, f32) {
        let b = w.get(e, c).unwrap();
        (
            f32::from_le_bytes(b[0..4].try_into().unwrap()),
            f32::from_le_bytes(b[4..8].try_into().unwrap()),
        )
    }

    #[test]
    fn components_attach_read_and_detach() {
        let mut w = World::new();
        let p = w.component("Pos", 8, 4).unwrap();
        let v = w.component("Vel", 8, 4).unwrap();
        assert_eq!((p, v), (1, 2));
        assert_eq!(w.component("Pos", 8, 4), Ok(1), "same name, same id");
        assert!(w.component("Pos", 12, 4).is_err(), "same name, other size");

        let a = w.spawn();
        let b = w.spawn();
        assert_ne!(a, 0);
        assert!(w.set(a, p, &pos(1.0, 2.0)));
        assert!(w.set(b, p, &pos(3.0, 4.0)));
        assert!(w.set(b, v, &[]));
        assert_eq!(read_pos(&w, a, p), (1.0, 2.0));
        assert_eq!(read_pos(&w, b, v), (0.0, 0.0), "no bytes: zeroed");
        assert!(w.has(b, v) && !w.has(a, v));

        // Writing through the pointer changes the stored value
        let ptr = w.ptr(a, p).unwrap();
        assert_eq!(ptr as usize % 4, 0);
        unsafe { (ptr as *mut f32).write(10.0) };
        assert_eq!(read_pos(&w, a, p), (10.0, 2.0));

        // Removing the first row moves the last one into its place
        assert!(w.remove(a, p));
        assert!(!w.remove(a, p));
        assert_eq!(read_pos(&w, b, p), (3.0, 4.0));
        assert_eq!(w.query(&[p]), vec![b]);
    }

    #[test]
    fn despawned_ids_stop_matching() {
        let mut w = World::new();
        let c = w.component("Tag", 0, 1).unwrap();
        let a = w.spawn();
        w.set(a, c, &[]);
        assert!(
            w.ptr(a, c).is_some(),
            "zero-size components still have a pointer"
        );
        assert!(w.despawn(a));
        assert!(!w.alive(a) && !w.despawn(a) && !w.set(a, c, &[]));
        let b = w.spawn();
        assert_eq!(entity_index(b), entity_index(a), "the index is reused");
        assert_ne!(b, a, "with a new generation");
        assert!(!w.has(b, c), "components of the old entity are gone");
        assert_eq!(w.count(), 1);
    }

    #[test]
    fn queries_match_all_components_in_a_fixed_order() {
        let mut w = World::new();
        let p = w.component("Pos", 8, 4).unwrap();
        let v = w.component("Vel", 8, 4).unwrap();
        let es: Vec<Entity> = (0..6).map(|_| w.spawn()).collect();
        for (i, &e) in es.iter().enumerate() {
            w.set(e, p, &pos(i as f32, 0.0));
            if i % 2 == 0 {
                w.set(e, v, &pos(1.0, 0.0));
            }
        }
        assert_eq!(w.query(&[p, v]), vec![es[0], es[2], es[4]]);
        assert_eq!(w.query(&[]).len(), 6);
        assert!(w.query(&[p, 99]).is_empty());

        // The same calls on a copy give the same order: determinism
        let mut copy = w.clone();
        for world in [&mut w, &mut copy] {
            world.despawn(es[2]);
            let e = world.spawn();
            world.set(e, v, &[]);
            world.set(e, p, &[]);
        }
        assert_eq!(w.query(&[v, p]), copy.query(&[v, p]));
        w.clear();
        assert_eq!((w.count(), w.query(&[]).len()), (0, 0));
        assert_eq!(w.component_id("Vel"), Some(v));
    }

    #[test]
    fn rows_are_aligned_and_survive_growth() {
        let mut w = World::new();
        let c = w.component("Big", 24, 16).unwrap();
        let es: Vec<Entity> = (0..100).map(|_| w.spawn()).collect();
        for (i, &e) in es.iter().enumerate() {
            w.set(e, c, &(i as u64).to_le_bytes());
            assert_eq!(w.ptr(e, c).unwrap() as usize % 16, 0);
        }
        for (i, &e) in es.iter().enumerate() {
            assert_eq!(w.get(e, c).unwrap()[..8], (i as u64).to_le_bytes());
            assert_eq!(w.get(e, c).unwrap().len(), 24);
        }
        assert!(w.component("Odd", 4, 3).is_err());
        assert!(w.component("Huge", MAX_COMPONENT_SIZE + 1, 4).is_err());
    }
}
