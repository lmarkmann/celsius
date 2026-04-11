//! Rain and snow particle overlay.
//!
//! Streaks are blended into the pixel buffer after the main compositor
//! runs. Rain uses a vertical color gradient from cool near-white at the
//! top to slate gray at the bottom so the streaks read against both bright
//! upper sky and darker storm clouds below. Snow is a single near-white.

use crate::colorspace::{Oklab, PixelBuffer, lerp_oklab, oklab_to_rgb, rgb_u8_to_oklab};
use crate::noise::Mt19937;
use crate::scene::Precipitation;

fn rain_top() -> Oklab {
    rgb_u8_to_oklab(208, 218, 230)
}
fn rain_bot() -> Oklab {
    rgb_u8_to_oklab(150, 160, 172)
}
fn snow() -> Oklab {
    rgb_u8_to_oklab(242, 244, 248)
}

pub fn overlay(pixels: &mut PixelBuffer, precip: &Precipitation) {
    let width = pixels.width;
    let height = pixels.height;
    let mut rng = Mt19937::init_by_array(&[precip.seed as u32]);
    let n = (((width * height) as f64 * precip.intensity * 0.025) as usize).max(1);
    let is_rain = precip.kind == "rain";
    let angle_rad = precip.angle_deg.to_radians();
    let dx = angle_rad.tan();

    for _ in 0..n {
        let px = rng.randint(0, width as i32 - 1);
        let py = rng.randint(0, height as i32 - 1);
        let jitter = -0.08 + 0.16 * rng.next_f64();
        let effective_dx = dx + jitter;

        for i in 0..precip.streak_len {
            let sy = py + i as i32;
            let sx = (px as f64 + effective_dx * i as f64).round() as i32;
            if sy >= height as i32 || sx < 0 || sx >= width as i32 {
                break;
            }
            let vt = sy as f64 / height as f64;
            let depth_fade = 0.55 + 0.45 * vt;
            let alpha = precip.opacity * depth_fade;
            let color_lab = if is_rain {
                lerp_oklab(rain_top(), rain_bot(), vt)
            } else {
                snow()
            };
            let orig = pixels.get(sx as usize, sy as usize);
            let orig_lab = crate::colorspace::rgb_u8_to_oklab(orig.r, orig.g, orig.b);
            let inv = 1.0 - alpha;
            let new_lab = Oklab::new(
                orig_lab.l * inv + color_lab.l * alpha,
                orig_lab.a * inv + color_lab.a * alpha,
                orig_lab.b * inv + color_lab.b * alpha,
            );
            pixels.set(sx as usize, sy as usize, oklab_to_rgb(new_lab));
        }
    }
}
