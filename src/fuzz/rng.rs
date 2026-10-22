//! Sources of randomness for the generator and mutator.
//!
//! Two implementations of [`Source`]:
//! - [`Rng`]: a seeded xoshiro256** generator. Its output is fully determined
//!   by the seed on every platform, which is what makes `--seed` reproducible.
//!   It is implemented here (about 30 lines) rather than taken from a crate so
//!   the sequence can never change under a dependency upgrade.
//! - [`ByteSource`]: draws choices from a byte slice, so a coverage-guided
//!   engine such as libFuzzer can drive the same structure-aware generator.
//!   When the bytes run out it returns 0, which always selects the smallest
//!   option, so generation terminates.

/// A stream of choices.
pub trait Source {
    /// Next raw 64-bit value.
    fn next_u64(&mut self) -> u64;

    /// Uniform-ish value in `0..n` (`n == 0` returns 0). The tiny modulo bias
    /// is irrelevant for fuzzing and keeps the mapping simple and stable.
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next_u64() % n }
    }

    /// `below` for indices.
    fn index(&mut self, n: usize) -> usize {
        usize::try_from(self.below(n as u64)).unwrap_or(0)
    }

    /// True with probability `percent`/100.
    fn chance(&mut self, percent: u32) -> bool {
        self.below(100) < u64::from(percent)
    }

    /// Pick an index according to integer weights (all-zero weights pick 0).
    fn weighted(&mut self, weights: &[u32]) -> usize {
        let total: u64 = weights.iter().map(|w| u64::from(*w)).sum();
        let mut x = self.below(total);
        for (i, w) in weights.iter().enumerate() {
            let w = u64::from(*w);
            if x < w {
                return i;
            }
            x -= w;
        }
        0
    }
}

/// Seeded xoshiro256** (Blackman & Vigna), state expanded with SplitMix64.
#[derive(Debug, Clone)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    /// Create a generator from a seed. Equal seeds give equal streams.
    pub fn new(seed: u64) -> Self {
        let mut sm = seed;
        let mut next = || {
            sm = sm.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = sm;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        Rng { s: [next(), next(), next(), next()] }
    }

    /// Derive an independent child stream (e.g. one per iteration), so that
    /// iteration `i` is reproducible on its own from `(seed, i)`.
    pub fn fork(seed: u64, stream: u64) -> Self {
        Rng::new(seed ^ stream.wrapping_mul(0xD1B5_4A32_D192_ED03).rotate_left(17))
    }
}

impl Source for Rng {
    fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }
}

/// Choices read from raw bytes (8 bytes per choice, little-endian, zero-padded).
#[derive(Debug, Clone)]
pub struct ByteSource<'a> {
    data: &'a [u8],
}

impl<'a> ByteSource<'a> {
    /// Wrap a byte slice.
    pub fn new(data: &'a [u8]) -> Self {
        ByteSource { data }
    }
}

impl Source for ByteSource<'_> {
    fn next_u64(&mut self) -> u64 {
        let take = self.data.len().min(8);
        let mut buf = [0u8; 8];
        buf[..take].copy_from_slice(&self.data[..take]);
        self.data = &self.data[take..];
        u64::from_le_bytes(buf)
    }
}
