//! Seeded random numbers for scatter patterns.
//!
//! The same seed must give the same coil on every OS, compiler and release,
//! because the seed is saved in the design and the preview has to match the
//! machine. A library RNG can change its stream between versions, so this is
//! a fixed xoshiro256** seeded through SplitMix64 (both public-domain
//! reference algorithms).

#[derive(Debug, Clone)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut x = seed;
        let mut next = || {
            x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        Rng {
            s: [next(), next(), next(), next()],
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.s;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// Uniform in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform in [lo, hi). Returns `lo` when the range is empty.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        if hi <= lo {
            lo
        } else {
            lo + (hi - lo) * self.next_f64()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_is_fixed() {
        // Pinned so a change to the generator is caught: it would silently
        // change every saved scatter design.
        let mut r = Rng::new(1234);
        let first: Vec<u64> = (0..3).map(|_| r.next_u64()).collect();
        let mut again = Rng::new(1234);
        assert_eq!(first, (0..3).map(|_| again.next_u64()).collect::<Vec<_>>());
        assert_eq!(first, PINNED_1234);
    }

    #[test]
    fn floats_stay_in_range() {
        let mut r = Rng::new(7);
        for _ in 0..10_000 {
            let v = r.range(1.2, 1.8);
            assert!((1.2..1.8).contains(&v));
        }
    }

    const PINNED_1234: [u64; 3] = [
        840842556444225107,
        15548185570577040190,
        12744864379734484625,
    ];
}
