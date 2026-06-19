//! Throwaway: render the prototype analytic sky across sun elevations and
//! turbidities to PNGs in out/, so the model can be eyeballed without a live
//! forecast. Sun position is chosen directly (due south), no network.
//!   cargo run --example analytic_demo --features png

use std::path::Path;

use celsius::analytic_sky::AnalyticSky;
use celsius::gradient::Gradient;
use celsius::render::render;
use celsius::scene::{Chrome, CloudKind, CloudLayer, SkyState, Sun};
use celsius::terminal;

fn cumulus_layer() -> CloudLayer {
    CloudLayer {
        cover: 1.4,
        altitude_t: 0.45,
        altitude_sigma: 0.16,
        scale_x: 3.2,
        scale_y: 2.4,
        threshold: 0.48,
        seed: 1337,
        kind: CloudKind::Cumulus,
        flatten: 0.0,
        offset_x: 0.4,
        offset_y: 1.3,
    }
}

fn demo_sky(sun_alt: f64, turbidity: f64) -> SkyState {
    let center_az = 180.0;
    let sun_az = 180.0; // due south, dead center
    let y_frac = (1.0 - sun_alt.to_radians().sin()).clamp(0.0, 1.0);
    SkyState {
        name: format!("analytic_alt{}_t{}", sun_alt as i32, turbidity as i32),
        gradient: Gradient::from_rgb_stops(&[(0.0, [0, 0, 0]), (1.0, [0, 0, 0])]),
        sun: Sun {
            x_frac: 0.5,
            y_frac,
            radius: 4.0,
            visible: sun_alt > 0.0,
        },
        clouds: vec![],
        chrome: Chrome {
            header_left: String::new(),
            header_right: String::new(),
            footer: String::new(),
            keys: String::new(),
            status: String::new(),
            footer_tiers: Vec::new(),
            keys_tiers: Vec::new(),
        },
        haze: None,
        stars: None,
        moon: None,
        precipitation: None,
        lightning: None,
        horizon_glow: None,
        analytic: Some(AnalyticSky {
            sun_alt,
            sun_az,
            center_az,
            turbidity,
            blend: 1.0,
        }),
        wind_speed_kmh: 0.0,
        unix_utc: 0,
    }
}

fn main() {
    std::fs::create_dir_all("out").unwrap();
    for &alt in &[3.0, 10.0, 25.0, 55.0] {
        for &t in &[2.0, 4.0, 8.0] {
            let sky = demo_sky(alt, t);
            let pixels = render(&sky, 240, 120);
            let path = format!("out/analytic_alt{:02}_t{}.png", alt as i32, t as i32);
            terminal::write_png(&pixels, Path::new(&path)).unwrap();
            println!("{path}");

            // Same scene at the real half-block terminal resolution (104x100),
            // so the on-screen coarseness is visible, not just the smooth PNG.
            let term = render(&sky, 104, 100);
            let tpath = format!("out/analytic_alt{:02}_t{}_term.png", alt as i32, t as i32);
            terminal::write_png(&term, Path::new(&tpath)).unwrap();
        }
    }

    // Analytic sky under a cumulus deck: clouds composite on top, analytic blue
    // shows through the gaps. The one case to confirm before defaulting on.
    let mut cloudy = demo_sky(30.0, 2.5);
    cloudy.clouds = vec![cumulus_layer()];
    let px = render(&cloudy, 240, 120);
    terminal::write_png(&px, Path::new("out/analytic_cloudy.png")).unwrap();
    println!("out/analytic_cloudy.png");
}
