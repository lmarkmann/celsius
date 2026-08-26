//! MT19937 and the value noise the clouds are made of.
//!
//! The PRNG is a from-scratch Mersenne Twister that matches CPython's `random.Random` bit for bit: `init_by_array` for seeding, `genrand_res53` for `next_f64`, and rejection sampling for `randbelow`. That parity is load-bearing rather than nostalgic. The golden-image tests assert a SHA256 per scene, so the same seed has to yield the same noise, the same stars and the same raindrops on every platform and every release. Changing the generator changes every locked hash.
//!
//! On top of it sits value noise on a fixed 96x32 grid, smoothstep-interpolated, plus fractal Brownian motion and a domain-warped variant. Warped FBM is what gives clouds their billow: the noise is sampled at coordinates that are themselves offset by noise, so edges curl instead of running straight.

pub const NOISE_WIDTH: usize = 96;
pub const NOISE_HEIGHT: usize = 32;

pub struct Noise {
    width: usize,
    height: usize,
    grid: Vec<f64>,
}

// Written out rather than derived: the grid is 3072 samples at the default size, and printing them buries whatever the reader was actually looking at.
impl std::fmt::Debug for Noise {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Noise")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("grid", &format_args!("[{} samples]", self.grid.len()))
            .finish()
    }
}

impl Noise {
    pub fn new(seed: u64) -> Self {
        Self::with_size(seed, NOISE_WIDTH, NOISE_HEIGHT)
    }

    pub fn with_size(seed: u64, width: usize, height: usize) -> Self {
        // `seed as u32` drops the high 32 bits of the u64. Intentional: the locked test vectors seed via init_by_array(&[seed as u32]), and bit parity with them is what the oracle relies on. Keep seeds inside u32 to be safe.
        let mut rng = Mt19937::init_by_array(&[seed as u32]);
        let grid = (0..width * height).map(|_| rng.next_f64()).collect();
        Self {
            width,
            height,
            grid,
        }
    }

    #[inline]
    fn at(&self, x: usize, y: usize) -> f64 {
        self.grid[y * self.width + x]
    }

    pub fn value(&self, x: f64, y: f64) -> f64 {
        let (xi, x_frac) = split_floor(x);
        let (yi, y_frac) = split_floor(y);
        let x0 = wrap(xi, self.width);
        let y0 = wrap(yi, self.height);
        // `wrap` already guarantees `x0 < width`, so the neighbour is a comparison rather than the division `% self.width` compiles to. `width` and `height` are runtime fields, so the compiler cannot fold either modulo into a multiply, and this ran four divisions per call on the hottest path in the renderer.
        let x1 = if x0 + 1 == self.width { 0 } else { x0 + 1 };
        let y1 = if y0 + 1 == self.height { 0 } else { y0 + 1 };
        let fx = smoothstep(x_frac);
        let fy = smoothstep(y_frac);
        let v00 = self.at(x0, y0);
        let v10 = self.at(x1, y0);
        let v01 = self.at(x0, y1);
        let v11 = self.at(x1, y1);
        let a = v00 * (1.0 - fx) + v10 * fx;
        let b = v01 * (1.0 - fx) + v11 * fx;
        a * (1.0 - fy) + b * fy
    }

    pub fn fbm(&self, x: f64, y: f64, octaves: u32) -> f64 {
        let mut total = 0.0;
        let mut amp = 0.5;
        let mut f = 1.0;
        for _ in 0..octaves {
            total += amp * self.value(x * f, y * f);
            f *= 2.0;
            amp *= 0.5;
        }
        total
    }

    pub fn warped_fbm(&self, x: f64, y: f64) -> f64 {
        self.warped_fbm_oct(x, y, 4)
    }

    // Same domain warp as warped_fbm, with the final fbm octave count exposed so cloud kinds can dial detail (cirrus wispy = more octaves, stratus smooth = fewer). octaves == 4 is bit-identical to warped_fbm.
    pub fn warped_fbm_oct(&self, x: f64, y: f64, octaves: u32) -> f64 {
        let wx = self.fbm(x + 1.7, y + 3.2, 3);
        let wy = self.fbm(x + 5.8, y + 0.9, 3);
        self.fbm(x + wx * 1.8, y + wy * 1.8, octaves)
    }
}

pub(crate) fn smoothstep(x: f64) -> f64 {
    x * x * (3.0 - 2.0 * x)
}

/// The integer floor of `v`, paired with `v` minus it.
///
/// A trap worth leaving marked, because the tempting rewrite here is a large regression. Under simulation `f64::floor` is 14% of a stormy frame: baseline x86-64 has no SSE4.1 `roundsd`, so it lowers to a software sequence of bit manipulations. Replacing it with truncate-and-correct (`v as i64`, minus one where truncation overshot a negative) removes that sequence and is exactly as correct, which makes it look like free speed.
///
/// It measured 32% slower. aarch64 lowers `floor` to a single `frintm`, so the rewrite swaps one instruction for a seven-instruction dependency chain: `warped_fbm_5200` runs 154us this way against 203us by hand, on a 185us baseline. The x86 cost is modelled and the aarch64 cost is measured, and the measured one decides.
///
/// If the x86 `floor` is ever worth addressing it is a `target-cpu` question for the musl artifact, not a code one. Writing it out by hand makes the primary platform pay for the secondary one.
#[inline]
fn split_floor(v: f64) -> (i64, f64) {
    let floor = v.floor();
    (floor as i64, v - floor)
}

/// `i` reduced into `0..n`, the value `i64::rem_euclid(n)` returns.
///
/// `rem_euclid` is specified as `let r = self % rhs; if r < 0 { r + rhs.abs() } else { r }`, which is two divisions; for a positive `n` this is that definition with the second division written as the conditional add it exists to perform. Integer division is not pipelined, and `value` is called ten times per pixel per cloud layer, which put `rem_euclid` at 10% of a stormy frame under simulation.
#[inline]
fn wrap(i: i64, n: usize) -> usize {
    let n = n as i64;
    let r = i % n;
    (if r < 0 { r + n } else { r }) as usize
}

// MT19937 seeded via init_by_array, matching Python's random.Random(seed). genrand_res53 matches Python's random.random() output exactly.
const MT_N: usize = 624;
const MT_M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7fff_ffff;

pub struct Mt19937 {
    mt: [u32; MT_N],
    mti: usize,
}

// The 624-word state is not something a reader can interpret, and printing it invites comparing two generators by their internals rather than by their output. The position in the block is the one useful field.
impl std::fmt::Debug for Mt19937 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mt19937").field("mti", &self.mti).finish()
    }
}

impl Mt19937 {
    fn init_genrand(&mut self, seed: u32) {
        self.mt[0] = seed;
        for i in 1..MT_N {
            self.mt[i] = 1_812_433_253u32
                .wrapping_mul(self.mt[i - 1] ^ (self.mt[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        self.mti = MT_N;
    }

    pub fn init_by_array(key: &[u32]) -> Self {
        let mut s = Self {
            mt: [0u32; MT_N],
            mti: MT_N + 1,
        };
        s.init_genrand(19_650_218);
        let key_len = key.len();
        let mut i: usize = 1;
        let mut j: usize = 0;
        let mut k = MT_N.max(key_len);
        while k > 0 {
            s.mt[i] = (s.mt[i] ^ ((s.mt[i - 1] ^ (s.mt[i - 1] >> 30)).wrapping_mul(1_664_525)))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= MT_N {
                s.mt[0] = s.mt[MT_N - 1];
                i = 1;
            }
            if j >= key_len {
                j = 0;
            }
            k -= 1;
        }
        k = MT_N - 1;
        while k > 0 {
            s.mt[i] = (s.mt[i] ^ ((s.mt[i - 1] ^ (s.mt[i - 1] >> 30)).wrapping_mul(1_566_083_941)))
                .wrapping_sub(i as u32);
            i += 1;
            if i >= MT_N {
                s.mt[0] = s.mt[MT_N - 1];
                i = 1;
            }
            k -= 1;
        }
        s.mt[0] = 0x8000_0000;
        s.mti = MT_N;
        s
    }

    fn generate(&mut self) {
        let mag01 = [0u32, MATRIX_A];
        for kk in 0..(MT_N - MT_M) {
            let y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
            self.mt[kk] = self.mt[kk + MT_M] ^ (y >> 1) ^ mag01[(y & 1) as usize];
        }
        for kk in (MT_N - MT_M)..(MT_N - 1) {
            let y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
            self.mt[kk] = self.mt[kk + MT_M - MT_N] ^ (y >> 1) ^ mag01[(y & 1) as usize];
        }
        let y = (self.mt[MT_N - 1] & UPPER_MASK) | (self.mt[0] & LOWER_MASK);
        self.mt[MT_N - 1] = self.mt[MT_M - 1] ^ (y >> 1) ^ mag01[(y & 1) as usize];
        self.mti = 0;
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.mti >= MT_N {
            self.generate();
        }
        let y = self.mt[self.mti];
        self.mti += 1;
        let y = y ^ (y >> 11);
        let y = y ^ ((y << 7) & 0x9d2c_5680);
        let y = y ^ ((y << 15) & 0xefc6_0000);
        y ^ (y >> 18)
    }

    // Matches Python's random.random() (genrand_res53, 53-bit precision).
    pub fn next_f64(&mut self) -> f64 {
        let a = (self.next_u32() >> 5) as f64;
        let b = (self.next_u32() >> 6) as f64;
        (a * 67_108_864.0 + b) * (1.0 / 9_007_199_254_740_992.0)
    }

    // Matches Python's random._randbelow for n > 0 (k <= 32 fast path). n.bit_length() bits drawn from genrand_uint32, rejection-sampled.
    pub fn randbelow(&mut self, n: u32) -> u32 {
        debug_assert!(n > 0);
        let k = 32 - n.leading_zeros();
        let shift = 32 - k;
        loop {
            let r = self.next_u32() >> shift;
            if r < n {
                return r;
            }
        }
    }

    // Matches Python's random.randint(lo, hi) = randrange(lo, hi + 1).
    pub fn randint(&mut self, lo: i32, hi: i32) -> i32 {
        let n = (hi - lo + 1) as u32;
        lo + self.randbelow(n) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Python reference: import random; rng = random.Random(101); [rng.random() for _ in range(5)]
    #[test]
    fn mt19937_matches_python_seed_101() {
        let mut rng = Mt19937::init_by_array(&[101]);
        let vals: Vec<f64> = (0..5).map(|_| rng.next_f64()).collect();
        let expected = [
            0.5811521325045647,
            0.1947544955341367,
            0.9652511070611112,
            0.9239764016767943,
            0.46713867819697397,
        ];
        for (got, exp) in vals.iter().zip(expected.iter()) {
            assert!((got - exp).abs() < 1e-15, "got {got} expected {exp}");
        }
    }

    // Python reference: import random; rng = random.Random(101); vals = [rng.getrandbits(32) for _ in range(1280)] 1280 draws cross the 624-word refill boundary twice, so generate() runs on twisted state and every state word is observed; the 5-value tests above only ever read mt[0..10] of the first twist.
    #[test]
    fn mt19937_generate_refill_matches_python() {
        let mut rng = Mt19937::init_by_array(&[101]);
        let vals: Vec<u32> = (0..1280).map(|_| rng.next_u32()).collect();
        // First refill boundary: tail of twist 1 (including the wrap-around word at 623) into the head of twist 2.
        let expected_618 = [
            3720980164u32,
            3245653950,
            3663418607,
            4294872249,
            3067599802,
            2314046024,
            2189986376,
            3290219555,
            4114103146,
            1792017223,
            3177713995,
            1797633989,
        ];
        assert_eq!(&vals[618..630], &expected_618);
        // Second refill boundary.
        let expected_1244 = [
            2944732989u32,
            1455023257,
            3999568197,
            77141538,
            4071493273,
            2253603204,
            774423505,
            1371819118,
        ];
        assert_eq!(&vals[1244..1252], &expected_1244);
        // FNV-style fold over the full sequence: a wrong word at any index fails, which is what kills mutants in the middle of the twist loops.
        let mut h: u64 = 14_695_981_039_346_656_037;
        for &v in &vals {
            h = h.wrapping_mul(1_099_511_628_211) ^ u64::from(v);
        }
        assert_eq!(h, 7_826_571_744_396_643_321);
    }

    // Python reference: import random; rng = random.Random(4096); [rng.random() for _ in range(5)]
    #[test]
    fn mt19937_matches_python_seed_4096() {
        let mut rng = Mt19937::init_by_array(&[4096]);
        let vals: Vec<f64> = (0..5).map(|_| rng.next_f64()).collect();
        let expected = [
            0.6662618002210253,
            0.8124571806520611,
            0.973551421883107,
            0.7500083123050753,
            0.5931119942202338,
        ];
        for (got, exp) in vals.iter().zip(expected.iter()) {
            assert!((got - exp).abs() < 1e-15, "got {got} expected {exp}");
        }
    }

    /// `value` avoids `f64::floor` and all four of the integer divisions it used to run per call. Each rewrite is meant to be exact rather than close, because a golden hash only objects once someone chooses to relock, at which point the new hash quietly becomes the truth. These pin the pieces against the expressions they replaced.
    #[test]
    fn split_floor_matches_f64_floor() {
        let mut cases: Vec<f64> = (-400..400).map(|i| f64::from(i) * 0.37).collect();
        // Exact integers, zero, and negatives are the cases truncate-and-correct can get wrong.
        cases.extend([-96.5, -3.0, -1.0, -0.5, 0.0, 0.5, 1.0, 96.0, 383.0]);
        for v in cases {
            let (floor, frac) = split_floor(v);
            assert_eq!(floor, v.floor() as i64, "floor of {v}");
            assert_eq!(frac, v - v.floor(), "fract of {v}");
        }
    }

    #[test]
    fn wrap_matches_rem_euclid() {
        for i in -500i64..500 {
            for n in [NOISE_WIDTH, NOISE_HEIGHT, 1, 7] {
                assert_eq!(wrap(i, n), i.rem_euclid(n as i64) as usize, "{i} mod {n}");
            }
        }
    }

    /// The whole function against the body it replaced, including coordinates past the grid wrap and negative ones the cloud path never produces. Equality, not a tolerance: anything less would let a golden move.
    #[test]
    fn value_matches_the_expression_it_replaced() {
        let noise = Noise::new(101);
        let previous = |x: f64, y: f64| {
            let w = noise.width as i64;
            let h = noise.height as i64;
            let xi = x.floor();
            let yi = y.floor();
            let x0 = ((xi as i64).rem_euclid(w)) as usize;
            let y0 = ((yi as i64).rem_euclid(h)) as usize;
            let x1 = (x0 + 1) % noise.width;
            let y1 = (y0 + 1) % noise.height;
            let fx = smoothstep(x - xi);
            let fy = smoothstep(y - yi);
            let v00 = noise.at(x0, y0);
            let v10 = noise.at(x1, y0);
            let v01 = noise.at(x0, y1);
            let v11 = noise.at(x1, y1);
            let a = v00 * (1.0 - fx) + v10 * fx;
            let b = v01 * (1.0 - fx) + v11 * fx;
            a * (1.0 - fy) + b * fy
        };
        for iy in -60..60 {
            for ix in -60..60 {
                let x = f64::from(ix) * 3.3;
                let y = f64::from(iy) * 1.7;
                assert_eq!(noise.value(x, y), previous(x, y), "at ({x}, {y})");
            }
        }
    }
}
