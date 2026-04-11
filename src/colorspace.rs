//! sRGB <-> linear RGB <-> OKLab conversions.
//!
//! All gradient and compositing math runs in OKLab. Conversion to sRGB
//! happens exactly once, at the final quantization into terminal cell colors
//! (or PNG pixels during oracle tests).

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const BLACK: Self = Self { r: 0, g: 0, b: 0 };
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Oklab {
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

impl Oklab {
    pub const fn new(l: f64, a: f64, b: f64) -> Self {
        Self { l, a, b }
    }
}

pub struct PixelBuffer {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Rgb>,
}

impl PixelBuffer {
    pub fn filled(width: usize, height: usize, color: Rgb) -> Self {
        Self {
            width,
            height,
            pixels: vec![color; width * height],
        }
    }

    #[inline]
    pub fn index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> Rgb {
        self.pixels[self.index(x, y)]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, c: Rgb) {
        let i = self.index(x, y);
        self.pixels[i] = c;
    }
}

fn srgb_to_lin(c: f64) -> f64 {
    let c = c / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn lin_to_srgb(c: f64) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let v = if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (255.0 * v).round() as u8
}

pub fn rgb_to_oklab(r: f64, g: f64, b: f64) -> Oklab {
    let lr = srgb_to_lin(r);
    let lg = srgb_to_lin(g);
    let lb = srgb_to_lin(b);

    let l = 0.412_221_470_8 * lr + 0.536_332_536_3 * lg + 0.051_445_992_9 * lb;
    let m = 0.211_903_498_2 * lr + 0.680_699_545_1 * lg + 0.107_396_956_6 * lb;
    let s = 0.088_302_461_9 * lr + 0.281_718_837_6 * lg + 0.629_978_700_5 * lb;

    let l = l.cbrt();
    let m = m.cbrt();
    let s = s.cbrt();

    Oklab {
        l: 0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        a: 1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        b: 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    }
}

pub fn rgb_u8_to_oklab(r: u8, g: u8, b: u8) -> Oklab {
    rgb_to_oklab(r as f64, g as f64, b as f64)
}

pub fn oklab_to_rgb(lab: Oklab) -> Rgb {
    let l_ = lab.l + 0.396_337_777_4 * lab.a + 0.215_803_757_3 * lab.b;
    let m_ = lab.l - 0.105_561_345_8 * lab.a - 0.063_854_172_8 * lab.b;
    let s_ = lab.l - 0.089_484_177_5 * lab.a - 1.291_485_548_0 * lab.b;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    let lr = 4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s;
    let lg = -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s;
    let lb = -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s;

    Rgb {
        r: lin_to_srgb(lr),
        g: lin_to_srgb(lg),
        b: lin_to_srgb(lb),
    }
}

pub fn lerp_oklab(c1: Oklab, c2: Oklab, t: f64) -> Oklab {
    Oklab {
        l: c1.l + (c2.l - c1.l) * t,
        a: c1.a + (c2.a - c1.a) * t,
        b: c1.b + (c2.b - c1.b) * t,
    }
}
