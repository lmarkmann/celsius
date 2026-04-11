pub const NOISE_WIDTH: usize = 96;
pub const NOISE_HEIGHT: usize = 32;

pub struct Noise {
    width: usize,
    height: usize,
    grid: Vec<f64>,
}

impl Noise {
    pub fn new(seed: u64) -> Self {
        Self::with_size(seed, NOISE_WIDTH, NOISE_HEIGHT)
    }

    pub fn with_size(seed: u64, width: usize, height: usize) -> Self {
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
        let w = self.width as i64;
        let h = self.height as i64;
        let xi = x.floor();
        let yi = y.floor();
        let x0 = ((xi as i64).rem_euclid(w)) as usize;
        let y0 = ((yi as i64).rem_euclid(h)) as usize;
        let x1 = (x0 + 1) % self.width;
        let y1 = (y0 + 1) % self.height;
        let fx = smoothstep(x - xi);
        let fy = smoothstep(y - yi);
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
        let wx = self.fbm(x + 1.7, y + 3.2, 3);
        let wy = self.fbm(x + 5.8, y + 0.9, 3);
        self.fbm(x + wx * 1.8, y + wy * 1.8, 4)
    }
}

fn smoothstep(x: f64) -> f64 {
    x * x * (3.0 - 2.0 * x)
}

// MT19937 seeded via init_by_array, matching Python's random.Random(seed).
// genrand_res53 matches Python's random.random() output exactly.
const MT_N: usize = 624;
const MT_M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7fff_ffff;

pub struct Mt19937 {
    mt: [u32; MT_N],
    mti: usize,
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

    // Matches Python's random._randbelow for n > 0 (k <= 32 fast path).
    // n.bit_length() bits drawn from genrand_uint32, rejection-sampled.
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
}
