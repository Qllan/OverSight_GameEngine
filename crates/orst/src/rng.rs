//! A small seeded random number generator (SplitMix64). The same seed
//! gives the same numbers on every machine, which replays, saves and
//! network games rely on; the game's own Random would differ between runs.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng { state: seed }
    }

    pub fn state(&self) -> u64 {
        self.state
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// 0 <= x < 1, in steps of 2^-24 (every value is exact in f32)
    pub fn float(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u32 << 24) as f32
    }

    /// min <= x < max (max <= x < min when they are swapped)
    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.float()
    }

    /// min <= x <= max, both ends included; the ends may come in any order
    pub fn int(&mut self, min: i32, max: i32) -> i32 {
        let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
        let span = (hi as i64 - lo as i64 + 1) as u64;
        // Multiply-shift: no modulo bias worth noticing for spans below 2^32
        (lo as i64 + ((self.next_u32() as u64 * span) >> 32) as i64) as i32
    }
}

impl Default for Rng {
    fn default() -> Rng {
        Rng::new(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_numbers() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        let mut c = Rng::new(43);
        let xs: Vec<u64> = (0..100).map(|_| a.next_u64()).collect();
        let ys: Vec<u64> = (0..100).map(|_| b.next_u64()).collect();
        assert_eq!(xs, ys);
        assert_ne!(xs[0], c.next_u64());
        // Pinned: a change here breaks every saved replay
        assert_eq!(Rng::new(1).next_u64(), 0x910A_2DEC_8902_5CC1);
    }

    #[test]
    fn ranges_hold_their_ends() {
        let mut r = Rng::new(7);
        let mut seen = [false; 7];
        for _ in 0..10_000 {
            let f = r.float();
            assert!((0.0..1.0).contains(&f));
            let x = r.range(-2.0, 3.0);
            assert!((-2.0..3.0).contains(&x));
            let i = r.int(3, -3);
            assert!((-3..=3).contains(&i));
            seen[(i + 3) as usize] = true;
        }
        assert!(seen.iter().all(|&s| s), "every value of -3..=3 comes up");
        assert_eq!(r.int(5, 5), 5);
        let full = r.int(i32::MIN, i32::MAX);
        let _ = full;
    }
}
