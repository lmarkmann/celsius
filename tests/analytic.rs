// The analytic sky is the default daytime renderer and had no test at all, because the golden oracle is driven by scene TOMLs and `load_scene` sets `analytic: None`, so no golden can reach it.
//
// These assert on properties rather than on a hash. A hash would only catch a change after someone chose to relock, at which point the new hash becomes the truth; that is how `sample()` was rewritten to share `astro::view_dir` with nothing able to object. What follows states what the radiance field has to satisfy, so a sign flip in the Perez arithmetic fails here and not in someone's terminal.

use celsius::analytic_sky::{self, AnalyticSky};
use celsius::astro::{self, AltAz};
use celsius::atmosphere::Atmosphere;

const CENTER_AZ: f64 = 180.0;

fn sky(sun_alt: f64, sun_az: f64, turbidity: f64) -> analytic_sky::Prepared {
    analytic_sky::prepare(&AnalyticSky {
        sun_alt,
        sun_az,
        center_az: CENTER_AZ,
        atmosphere: Atmosphere::from_turbidity(turbidity),
        blend: 1.0,
    })
}

/// Where the renderer will draw the sun disc, in frame fractions.
fn disc_position(sun_alt: f64, sun_az: f64) -> (f64, f64) {
    astro::to_sky_fracs(
        &AltAz {
            altitude: sun_alt,
            azimuth: sun_az,
        },
        CENTER_AZ,
    )
    .expect("the test suns are all in front of the viewer")
}

/// Scan the frame at the reference resolution and return the brightest sample.
fn brightest(prepared: &analytic_sky::Prepared, w: u32, h: u32) -> (f64, f64, f64) {
    let mut best = (0.0, 0.0, f64::NEG_INFINITY);
    for py in 0..h {
        for px in 0..w {
            let x = f64::from(px) / f64::from(w - 1);
            let y = f64::from(py) / f64::from(h - 1);
            let l = prepared.sample(x, y).l;
            if l > best.2 {
                best = (x, y, l);
            }
        }
    }
    best
}

/// The bug this exists to catch: `sample()` derived its view direction independently of the projection that places the disc, so the brightest point of the sky drifted away from the sun drawn on top of it. The two now share `astro::view_dir`, and this is what holds them together.
#[test]
fn radiance_peaks_where_the_sun_disc_is_drawn() {
    for sun_alt in [10.0, 25.0, 45.0, 70.0] {
        let prepared = sky(sun_alt, CENTER_AZ, 2.5);
        let (peak_x, peak_y, _) = brightest(&prepared, 104, 50);
        let (disc_x, disc_y) = disc_position(sun_alt, CENTER_AZ);

        // One pixel of tolerance in each axis: the scan is discrete, so the peak can only ever land on the sample nearest the true maximum.
        assert!(
            (peak_x - disc_x).abs() <= 1.0 / 104.0 + 1e-9,
            "at {sun_alt} deg the sky peaks at x={peak_x:.4} but the disc is drawn at {disc_x:.4}"
        );
        assert!(
            (peak_y - disc_y).abs() <= 1.0 / 50.0 + 1e-9,
            "at {sun_alt} deg the sky peaks at y={peak_y:.4} but the disc is drawn at {disc_y:.4}"
        );
    }
}

/// The disc and the radiance can only disagree away from frame centre, and a sun parked on the meridian never tests that. This is the same reason `lab-sweep` grew a `--sun-az` axis.
#[test]
fn an_off_centre_sun_still_peaks_at_its_own_position() {
    for az_offset in [-40.0, -15.0, 15.0, 40.0] {
        let sun_az = CENTER_AZ + az_offset;
        let prepared = sky(30.0, sun_az, 2.5);
        let (peak_x, peak_y, _) = brightest(&prepared, 104, 50);
        let (disc_x, disc_y) = disc_position(30.0, sun_az);

        assert!(
            (peak_x - disc_x).abs() <= 1.0 / 104.0 + 1e-9,
            "at {az_offset:+} deg of azimuth the peak sits at x={peak_x:.4}, the disc at {disc_x:.4}"
        );
        assert!(
            (peak_y - disc_y).abs() <= 1.0 / 50.0 + 1e-9,
            "at {az_offset:+} deg of azimuth the peak sits at y={peak_y:.4}, the disc at {disc_y:.4}"
        );
    }
}

/// Sky brightness falls away from the sun. Stated across the frame at a matched height rather than in every direction, because Preetham's gradient term also brightens toward the horizon, so "darker the further from the sun" is only true at constant zenith angle.
#[test]
fn the_sun_side_of_the_sky_is_brighter_than_the_far_side() {
    let prepared = sky(35.0, CENTER_AZ + 45.0, 2.5);
    for y in [0.2, 0.4, 0.6, 0.8] {
        let near_sun = prepared.sample(0.95, y).l;
        let far_side = prepared.sample(0.05, y).l;
        assert!(
            near_sun > far_side,
            "with the sun to the west, x=0.95 ({near_sun:.4}) should beat x=0.05 ({far_side:.4}) at y={y}"
        );
    }
}

/// Preetham's zenith luminance is undefined once the solar zenith angle passes 90 degrees, and `compose` only attaches the model above the horizon for exactly that reason. This pins the region that is meant to work: a sun a tenth of a degree up must still produce a finite, in-gamut sky, so the boundary fails loudly here rather than as NaN pixels.
#[test]
fn every_elevation_and_turbidity_stays_finite_and_in_gamut() {
    for sun_alt in [0.1, 1.0, 5.0, 15.0, 40.0, 89.0] {
        for turbidity in [1.5, 2.0, 4.0, 8.0, 12.0] {
            let prepared = sky(sun_alt, CENTER_AZ, turbidity);
            for (x, y) in [
                (0.0, 0.0),
                (1.0, 0.0),
                (0.0, 1.0),
                (1.0, 1.0),
                (0.5, 0.5),
                (0.5, 0.0),
                (0.5, 1.0),
            ] {
                let lab = prepared.sample(x, y);
                assert!(
                    lab.l.is_finite() && lab.a.is_finite() && lab.b.is_finite(),
                    "sun {sun_alt} deg, turbidity {turbidity}, ({x}, {y}) produced {lab:?}"
                );
                assert!(
                    (0.0..=1.0).contains(&lab.l),
                    "sun {sun_alt} deg, turbidity {turbidity}, ({x}, {y}) has lightness {} outside 0..1",
                    lab.l
                );
            }
        }
    }
}

/// Turbidity flattens the sky: thicker air scatters light out of the aureole and into everywhere else, so the spread between the brightest and dimmest parts of the frame narrows as it rises. Stated as contrast rather than as brightness on purpose, because `adapted_exposure` sets each sky's gain from a survey of that same sky, so two turbidities are never on a common absolute scale and comparing their lightness directly measures the tone curve instead of the atmosphere.
#[test]
fn a_hazier_sky_is_a_flatter_sky() {
    let spread = |turbidity: f64| {
        let prepared = sky(40.0, CENTER_AZ, turbidity);
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for iy in 0..16 {
            for ix in 0..16 {
                let l = prepared
                    .sample(f64::from(ix) / 15.0, f64::from(iy) / 15.0)
                    .l;
                lo = lo.min(l);
                hi = hi.max(l);
            }
        }
        hi - lo
    };

    let mut previous = f64::INFINITY;
    for turbidity in [1.5, 2.5, 4.0, 6.0, 9.0] {
        let contrast = spread(turbidity);
        assert!(
            contrast < previous,
            "turbidity {turbidity} spreads {contrast:.4} of lightness across the frame, no less than the {previous:.4} of the clearer sky before it"
        );
        previous = contrast;
    }
}

/// `blend` is the crossfade weight the renderer lerps by, and `prepare` is the only place it can be sanitised before the pixel loop divides by nothing and multiplies by it thousands of times.
#[test]
fn the_crossfade_weight_is_clamped() {
    for (given, expected) in [(-0.5, 0.0), (0.0, 0.0), (0.5, 0.5), (1.0, 1.0), (2.5, 1.0)] {
        let prepared = analytic_sky::prepare(&AnalyticSky {
            sun_alt: 30.0,
            sun_az: CENTER_AZ,
            center_az: CENTER_AZ,
            atmosphere: Atmosphere::from_turbidity(2.5),
            blend: given,
        });
        assert_eq!(
            prepared.blend, expected,
            "blend {given} should clamp to {expected}"
        );
    }
}
