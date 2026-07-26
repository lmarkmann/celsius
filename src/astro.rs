//! Where the sun and moon are, from Meeus, and how to put them on screen.
//!
//! Solar and lunar position follow Meeus, *Astronomical Algorithms*: the sun to arcminute accuracy, the moon through the principal periodic terms, with apparent sidereal time corrected for nutation. Everything takes `(lat, lon, unix_utc)` and nothing else. That is deliberate: no place names, no timezones, no ambiguity about which "Springfield" was meant. Resolving a location into coordinates happens upstream, so this layer is pure and directly testable against published almanac values.
//!
//! [`to_sky_fracs`] is the bridge to the renderer, projecting an alt/az pair onto the 0..1 screen fractions a scene uses. It takes the compass bearing the viewer faces, because a first-person sky depends on which way you are looking: the same moon sits on the left in Hamburg and the right in Santiago.

use std::f64::consts::PI;

#[derive(Clone, Debug, PartialEq)]
pub struct AltAz {
    pub altitude: f64,
    pub azimuth: f64,
}

#[derive(Clone, Debug)]
pub struct MoonState {
    pub altaz: AltAz,
    pub illumination: f64,
    pub phase: f64,
}

fn rad(deg: f64) -> f64 {
    deg * PI / 180.0
}

fn deg(rad: f64) -> f64 {
    rad * 180.0 / PI
}

fn norm(d: f64) -> f64 {
    d.rem_euclid(360.0)
}

fn jd(unix_utc: i64) -> f64 {
    2_440_587.5 + unix_utc as f64 / 86_400.0
}

fn jc(jd: f64) -> f64 {
    (jd - 2_451_545.0) / 36_525.0
}

fn obliquity(t: f64) -> f64 {
    23.439_291_11 - 0.013_004_2 * t - 1.64e-7 * t * t + 5.04e-7 * t * t * t
}

fn gast(jd_val: f64) -> f64 {
    let t = jc(jd_val);
    // Greenwich Mean Sidereal Time
    let gmst = norm(
        280.460_618_37 + 360.985_647_366_29 * (jd_val - 2_451_545.0) + 0.000_387_933 * t * t
            - t * t * t / 38_710_000.0,
    );
    // Nutation in longitude (arcsec) - simplified one-term approximation
    let omega = rad(125.04452 - 1934.136261 * t);
    let delta_psi_arcsec =
        -17.20 * omega.sin() - 1.32 * rad(2.0 * 280.4665 + 360.9856235 * t).sin();
    let eps = rad(obliquity(t));
    let eq_equinoxes = delta_psi_arcsec / 3600.0 * eps.cos();
    norm(gmst + eq_equinoxes)
}

// Saemundsson's formula: lifts geometric altitude to apparent altitude. At the horizon the sun/moon disc appears ~29 arcmin above its true position; shifts sunrise/sunset timing by a few minutes at mid latitudes, more near the poles.
fn refraction_deg(h_deg: f64) -> f64 {
    if h_deg < -1.0 {
        return 0.0;
    }
    1.02 / rad(h_deg + 10.3 / (h_deg + 5.11)).tan() / 60.0
}

fn to_horizontal(ra: f64, dec: f64, lat: f64, lon: f64, jd_val: f64) -> AltAz {
    let ha = rad(norm(gast(jd_val) + lon - ra));
    let dec_r = rad(dec);
    let lat_r = rad(lat);

    let sin_alt = lat_r.sin() * dec_r.sin() + lat_r.cos() * dec_r.cos() * ha.cos();
    let geometric_alt = deg(sin_alt.clamp(-1.0, 1.0).asin());
    let altitude = geometric_alt + refraction_deg(geometric_alt);

    // atan2 form from Meeus eq. 13.5; add 180 so 0=north
    let az = deg(f64::atan2(
        ha.sin(),
        ha.cos() * lat_r.sin() - dec_r.tan() * lat_r.cos(),
    ));
    let azimuth = norm(az + 180.0);

    AltAz { altitude, azimuth }
}

pub fn sun_position(lat: f64, lon: f64, unix_utc: i64) -> AltAz {
    let jd_val = jd(unix_utc);
    let t = jc(jd_val);

    // Mean longitude and anomaly (Meeus ch. 25)
    let l0 = norm(280.466_46 + 36_000.769_83 * t);
    let m = rad(norm(357.529_11 + 35_999.050_29 * t - 0.000_153_72 * t * t));

    // Equation of center
    let c = (1.914_602 - 0.004_817 * t - 0.000_014 * t * t) * m.sin()
        + (0.019_993 - 0.000_101 * t) * (2.0 * m).sin()
        + 0.000_289 * (3.0 * m).sin();

    let sun_lon = l0 + c;
    let omega = 125.04 - 1934.136 * t;
    // Apparent longitude, corrected for nutation and aberration
    let lambda = rad(sun_lon - 0.00569 - 0.00478 * rad(omega).sin());

    let eps = rad(obliquity(t) + 0.00256 * rad(omega).cos());

    let dec = deg((eps.sin() * lambda.sin()).clamp(-1.0, 1.0).asin());
    let ra = norm(deg(f64::atan2(eps.cos() * lambda.sin(), lambda.cos())));

    to_horizontal(ra, dec, lat, lon, jd_val)
}

/// Horizontal coordinates of a fixed celestial direction (RA, Dec in degrees,
/// J2000) for an observer at `(lat, lon)` at `unix_utc`. Used to place a
/// meteor-shower radiant in the sky.
pub fn equatorial_to_altaz(ra_deg: f64, dec_deg: f64, lat: f64, lon: f64, unix_utc: i64) -> AltAz {
    to_horizontal(ra_deg, dec_deg, lat, lon, jd(unix_utc))
}

pub fn moon_state(lat: f64, lon: f64, unix_utc: i64) -> MoonState {
    let jd_val = jd(unix_utc);
    let t = jc(jd_val);

    // Fundamental arguments (Meeus ch. 47, degrees)
    let lp = norm(218.3164477 + 481_267.881_234_21 * t); // moon mean longitude
    let d = rad(norm(297.8501921 + 445_267.111_403_4 * t)); // mean elongation
    let ms = rad(norm(357.5291092 + 35_999.050_290_9 * t)); // sun mean anomaly
    let mm = rad(norm(134.9633964 + 477_198.867_505_5 * t)); // moon mean anomaly
    let f = rad(norm(93.2720950 + 483_202.017_523_3 * t)); // argument of latitude

    // Longitude perturbations (Meeus Table 47.A, 20 largest terms, units: 0.001 arcsec -> divide by 1e6 for deg) Coefficients are in units of 0.000001 degrees
    #[rustfmt::skip]
    let sigma_l: f64 = [
        ( 6_288_774.0,  0.0,  0.0,  1.0,  0.0),
        ( 1_274_027.0,  2.0,  0.0, -1.0,  0.0),
        (   658_314.0,  2.0,  0.0,  0.0,  0.0),
        (   213_618.0,  0.0,  0.0,  2.0,  0.0),
        (  -185_116.0,  0.0,  1.0,  0.0,  0.0),
        (  -114_332.0,  0.0,  0.0,  0.0,  2.0),
        (    58_793.0,  2.0,  0.0, -2.0,  0.0),
        (    57_066.0,  2.0, -1.0, -1.0,  0.0),
        (    53_322.0,  2.0,  0.0,  1.0,  0.0),
        (    45_758.0,  2.0, -1.0,  0.0,  0.0),
        (   -40_923.0,  0.0,  1.0, -1.0,  0.0),
        (   -34_720.0,  1.0,  0.0,  0.0,  0.0),
        (   -30_383.0,  0.0,  1.0,  1.0,  0.0),
        (    15_327.0,  2.0,  0.0,  0.0, -2.0),
        (   -12_528.0,  0.0,  0.0,  1.0,  2.0),
        (    10_980.0,  0.0,  0.0,  1.0, -2.0),
        (    10_675.0,  4.0,  0.0, -1.0,  0.0),
        (    10_034.0,  0.0,  0.0,  3.0,  0.0),
        (     8_548.0,  4.0,  0.0, -2.0,  0.0),
        (    -7_888.0,  2.0,  1.0, -1.0,  0.0),
    ].iter().map(|&(coef, cd, cms, cmm, cf)| {
        coef * (cd * d + cms * ms + cmm * mm + cf * f).sin()
    }).sum();

    // Latitude perturbations (Meeus Table 47.B, 15 largest terms)
    #[rustfmt::skip]
    let sigma_b: f64 = [
        ( 5_128_122.0,  0.0,  0.0,  0.0,  1.0),
        (   280_602.0,  0.0,  0.0,  1.0,  1.0),
        (   277_693.0,  0.0,  0.0,  1.0, -1.0),
        (   173_237.0,  2.0,  0.0,  0.0, -1.0),
        (    55_413.0,  2.0,  0.0, -1.0,  1.0),
        (    46_271.0,  2.0,  0.0, -1.0, -1.0),
        (    32_573.0,  2.0,  0.0,  0.0,  1.0),
        (    17_198.0,  0.0,  0.0,  2.0,  1.0),
        (     9_266.0,  2.0,  0.0,  1.0, -1.0),
        (     8_822.0,  0.0,  0.0,  2.0, -1.0),
        (     8_216.0,  2.0, -1.0,  0.0, -1.0),
        (     4_324.0,  2.0,  0.0, -2.0, -1.0),
        (     4_200.0,  2.0,  0.0,  1.0,  1.0),
        (    -3_359.0,  2.0,  1.0,  0.0, -1.0),
        (     2_463.0,  2.0, -1.0, -1.0,  1.0),
    ].iter().map(|&(coef, cd, cms, cmm, cf)| {
        coef * (cd * d + cms * ms + cmm * mm + cf * f).sin()
    }).sum();

    let moon_lon = rad(norm(lp + sigma_l / 1_000_000.0));
    let moon_lat = rad(sigma_b / 1_000_000.0);

    // Convert ecliptic to equatorial (Meeus ch. 13)
    let eps = rad(obliquity(t));
    let dec = deg(
        (moon_lat.sin() * eps.cos() + moon_lat.cos() * eps.sin() * moon_lon.sin())
            .clamp(-1.0, 1.0)
            .asin(),
    );
    let ra = norm(deg(f64::atan2(
        moon_lon.sin() * eps.cos() - moon_lat.tan() * eps.sin(),
        moon_lon.cos(),
    )));

    let altaz = to_horizontal(ra, dec, lat, lon, jd_val);

    // Phase: elongation between moon and sun in ecliptic longitude. Reuse t (already computed); skip the full horizontal transform.
    let sun_t = t;
    let sun_l0 = norm(280.466_46 + 36_000.769_83 * sun_t);
    let sun_m = rad(norm(357.529_11 + 35_999.050_29 * sun_t));
    let sun_c = (1.914_602 - 0.004_817 * sun_t) * sun_m.sin() + 0.019_993 * (2.0 * sun_m).sin();
    let sun_lon_deg = norm(sun_l0 + sun_c);

    // Elongation (angle between moon and sun as seen from Earth)
    let elongation = norm(deg(moon_lon) - sun_lon_deg);
    // Phase 0=new, 0.5=full: elongation/360
    let phase = elongation / 360.0;
    // Illuminated fraction from elongation angle (Meeus eq. 48.4, simplified)
    let illumination = (1.0 - rad(elongation).cos()) / 2.0;

    MoonState {
        altaz,
        illumination,
        phase,
    }
}

// Orthographic projection of the sky dome onto the view plane facing `center_az`. The object's unit direction has an eastward and an upward component (the depth component, toward the look direction, is dropped). This foreshortens azimuth as altitude rises, so a star near the zenith barely shifts sideways while one near the horizon swings the full width, and it bows the solar arc the way the real sky does instead of the old anamorphic linear map. Horizon stays at the frame bottom (y=1), zenith at the top (y=0).
/// Horizontal field of view of the frame, in degrees. Wide, because standing outside you take in far more sky than a camera does, but bounded: a rectilinear projection stretches without limit as it approaches 180, and the whole point of bounding it is that the upper sky stops being crushed into the top few rows.
pub const HFOV_DEG: f64 = 110.0;

/// The reference buffer the vertical field of view is derived from. Every scene constant is tuned against this size, so the framing has to come from it too.
const REF_WIDTH: f64 = 104.0;
const REF_HEIGHT: f64 = 50.0;

fn tan_half_h() -> f64 {
    (HFOV_DEG.to_radians() / 2.0).tan()
}

fn tan_half_v() -> f64 {
    tan_half_h() * (REF_HEIGHT / REF_WIDTH)
}

/// How far the view is tilted up from the horizontal, in radians. Chosen so the horizon lands exactly on the bottom edge, which is what makes this a view of the sky rather than of the ground.
fn pitch() -> f64 {
    tan_half_v().atan()
}

/// The direction a screen position looks at, as a unit vector in (east, up, forward) relative to the facing bearing.
///
/// This is the inverse of [`to_sky_fracs`] and the two must stay exact inverses of each other. The analytic sky samples radiance per pixel and needs the direction that pixel points; the renderer places the sun disc from a direction. When those disagree the sun's brightest point drifts away from the drawn disc, which is a bug that only shows at the edges of the frame and is easy to miss.
pub fn view_dir(x_frac: f64, y_frac: f64) -> [f64; 3] {
    let ndc_x = (x_frac - 0.5) * 2.0 * tan_half_h();
    let ndc_y = (0.5 - y_frac) * 2.0 * tan_half_v();
    let (sin_p, cos_p) = pitch().sin_cos();
    // Camera basis: right is due east of the facing bearing, up and forward are tilted by the pitch.
    let east = ndc_x;
    let up = ndc_y * cos_p + sin_p;
    let forward = -ndc_y * sin_p + cos_p;
    let len = (east * east + up * up + forward * forward).sqrt();
    [east / len, up / len, forward / len]
}

/// Project a sky position onto the frame, as fractions where (0, 0) is the top left and (1, 1) the bottom right.
///
/// Rectilinear, like a camera and unlike the eye: straight lines stay straight and the sky is not compressed toward the zenith.
///
/// `None` means the position is behind the viewing plane, where a rectilinear projection is not merely off-frame but undefined. Returning a sentinel instead would put an infinity into whatever arithmetic came next. In front of the plane the result is deliberately **not** clamped, so a caller can distinguish an object at the frame edge from one outside it, and so a meteor radiant off the left of the screen still aims its meteors correctly; ask [`in_view`] whether it actually lands on screen.
pub fn to_sky_fracs(altaz: &AltAz, center_az: f64) -> Option<(f64, f64)> {
    let alt = altaz.altitude.to_radians();
    let az_delta = (norm(altaz.azimuth - center_az + 180.0) - 180.0).to_radians();
    let east = alt.cos() * az_delta.sin();
    let up = alt.sin();
    let forward = alt.cos() * az_delta.cos();

    let (sin_p, cos_p) = pitch().sin_cos();
    let cam_up = up * cos_p - forward * sin_p;
    let cam_forward = up * sin_p + forward * cos_p;

    if cam_forward <= 1e-6 {
        return None;
    }
    let x_frac = 0.5 + 0.5 * (east / cam_forward) / tan_half_h();
    let y_frac = 0.5 - 0.5 * (cam_up / cam_forward) / tan_half_v();
    Some((x_frac, y_frac))
}

/// What fraction of the visible hemisphere the frame holds.
///
/// The frame is a symmetric rectangular pyramid, so its solid angle is `4 asin(sin(h/2) sin(v/2))`, and the pitch is chosen to put the horizon on the bottom edge, so none of it falls below the horizon and no clipping term is needed. Rates quoted for the whole sky have to be multiplied by this before they mean anything on screen: a meteor shower's ZHR counts the entire hemisphere, and you are looking at roughly a third of it.
pub fn frame_solid_angle_fraction() -> f64 {
    let omega = 4.0 * (tan_half_h().atan().sin() * tan_half_v().atan().sin()).asin();
    omega / (2.0 * PI)
}

/// Whether a sky position falls inside the frame. The honest replacement for testing the azimuth alone, which called a body overhead "behind you" purely because of where its azimuth pointed, even though azimuth means almost nothing near the zenith.
pub fn in_view(altaz: &AltAz, center_az: f64) -> bool {
    to_sky_fracs(altaz, center_az)
        .is_some_and(|(x, y)| (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference: USNO solar calculator, Washington DC (38.9N, 77.0W), 2025-06-21 solar noon. Solar noon at -77 lon is ~17:08 UTC (77/15 = 5.13h offset + small equation-of-time term). Expected: altitude ~74 deg, azimuth ~180 deg.
    #[test]
    fn sun_washington_solstice_noon() {
        // 2025-06-21 17:08:00 UTC
        let unix = 1_750_525_680i64;
        let pos = sun_position(38.9, -77.0, unix);
        assert!(
            pos.altitude > 65.0 && pos.altitude < 80.0,
            "altitude {} out of expected range 65..80",
            pos.altitude
        );
        assert!(
            pos.azimuth > 165.0 && pos.azimuth < 210.0,
            "azimuth {} out of expected range 165..210",
            pos.azimuth
        );
    }

    // Reference: USNO, same location, 2025-12-21 UTC noon. Winter solstice: sun altitude much lower, still roughly south.
    #[test]
    fn sun_washington_winter_noon() {
        // 2025-12-21 17:00 UTC ~ solar noon in Washington DC
        let unix = 1_766_340_000i64; // 2025-12-21 17:00:00 UTC
        let pos = sun_position(38.9, -77.0, unix);
        assert!(
            pos.altitude > 25.0 && pos.altitude < 40.0,
            "altitude {} out of expected range 25..40",
            pos.altitude
        );
        assert!(
            pos.azimuth > 160.0 && pos.azimuth < 210.0,
            "azimuth {} out of expected range 160..210",
            pos.azimuth
        );
    }

    // At north pole on summer solstice, sun altitude ~ 23.5 deg (axial tilt) and it circles the horizon, never setting.
    #[test]
    fn sun_north_pole_solstice() {
        // 2025-06-21 12:00 UTC
        let unix = 1_750_507_200i64;
        let pos = sun_position(89.9, 0.0, unix);
        // Should be roughly 23 degrees (earth's axial tilt), definitely above horizon
        assert!(
            pos.altitude > 18.0 && pos.altitude < 28.0,
            "altitude {} out of expected range 18..28",
            pos.altitude
        );
    }

    // Polar night: at north pole on winter solstice, sun should be ~23.5 deg below horizon.
    #[test]
    fn sun_north_pole_winter() {
        // 2025-12-21 12:00 UTC
        let unix = 1_766_318_400i64;
        let pos = sun_position(89.9, 0.0, unix);
        assert!(
            pos.altitude < -18.0,
            "altitude {} should be below -18 (polar night)",
            pos.altitude
        );
    }

    // Moon phase sanity: 2025-01-29 was a full moon. Illumination should be close to 1.0.
    #[test]
    fn moon_full_2025_jan_29() {
        // 2025-01-29 18:36 UTC (new moon is wrong, let me use a full moon) Full moon: 2025-01-13 22:27 UTC
        let unix = 1_736_810_820i64; // 2025-01-13 22:27:00 UTC
        let state = moon_state(51.5, -0.1, unix); // London
        // At full moon illumination >= 0.95
        assert!(
            state.illumination > 0.90,
            "illumination {} should be near 1.0 at full moon",
            state.illumination
        );
        // Phase should be near 0.5
        let phase_dist = (state.phase - 0.5)
            .abs()
            .min((state.phase - 0.5 + 1.0).abs());
        assert!(
            phase_dist < 0.08,
            "phase {} should be near 0.5 at full moon",
            state.phase
        );
    }

    // Moon phase sanity: 2025-01-29 was a new moon. Illumination should be close to 0.
    #[test]
    fn moon_new_2025_jan_29() {
        // New moon: 2025-01-29 12:36 UTC
        let unix = 1_738_150_560i64; // 2025-01-29 12:36:00 UTC
        let state = moon_state(51.5, -0.1, unix);
        assert!(
            state.illumination < 0.08,
            "illumination {} should be near 0 at new moon",
            state.illumination
        );
    }

    // Atmospheric refraction at the horizon: a body at geometric altitude 0 should appear lifted by ~29 arcmin (Saemundsson). At zenith, refraction is zero. Above ~10 deg, it drops below 6 arcmin.
    #[test]
    fn refraction_horizon_and_zenith() {
        assert!(
            (refraction_deg(0.0) - 0.483).abs() < 0.01,
            "refraction at horizon should be ~0.48 deg, got {}",
            refraction_deg(0.0)
        );
        assert!(
            refraction_deg(90.0).abs() < 0.001,
            "refraction at zenith should be ~0, got {}",
            refraction_deg(90.0)
        );
        assert_eq!(
            refraction_deg(-5.0),
            0.0,
            "no refraction well below horizon"
        );
    }

    #[test]
    fn sky_fracs_sun_at_south_horizon() {
        let altaz = AltAz {
            altitude: 0.0,
            azimuth: 180.0,
        };
        let (x, y) = to_sky_fracs(&altaz, 180.0).expect("due south is in front of the viewer");
        assert!((x - 0.5).abs() < 1e-9, "x_frac should be 0.5 for due south");
        assert!((y - 1.0).abs() < 1e-9, "y_frac should be 1.0 at horizon");
    }

    /// The frame is a bounded window, not the whole hemisphere. Something directly overhead is above the top edge: you would have to look up, and the projection says so instead of pinning it to the first row.
    #[test]
    fn sky_fracs_puts_the_zenith_above_the_frame() {
        let altaz = AltAz {
            altitude: 90.0,
            azimuth: 180.0,
        };
        let (x, y) = to_sky_fracs(&altaz, 180.0).expect("the zenith is above, not behind");
        assert!((x - 0.5).abs() < 1e-9, "zenith stays centered, got {x}");
        assert!(y < 0.0, "zenith should sit above the top edge, got {y}");
        assert!(!in_view(&altaz, 180.0));
    }

    /// The bug this projection exists to kill: `sin` is symmetric about 90 degrees, so the old map put something 158 degrees behind the viewer at dead centre of the frame.
    #[test]
    fn objects_behind_the_viewer_are_not_folded_into_frame() {
        let behind = AltAz {
            altitude: 20.0,
            azimuth: 22.0,
        };
        assert!(
            !in_view(&behind, 180.0),
            "az 22 is behind a south-facing view"
        );
        assert!(
            to_sky_fracs(&behind, 180.0).is_none(),
            "behind the viewing plane there is no projection to return"
        );
    }

    /// Equal slabs of sky get roughly equal pixels. Under the old `1 - sin(alt)` map the lowest 30 degrees took half the frame while the top 30 took 13 percent, which is what crushed every high radiant into the top rows.
    #[test]
    fn vertical_mapping_does_not_crush_the_upper_sky() {
        let y_at = |alt: f64| {
            to_sky_fracs(
                &AltAz {
                    altitude: alt,
                    azimuth: 180.0,
                },
                180.0,
            )
            .expect("due south is in front of the viewer")
            .1
        };
        let low_band = y_at(0.0) - y_at(20.0);
        let high_band = y_at(40.0) - y_at(60.0);
        let ratio = low_band / high_band;
        assert!(
            ratio < 2.0,
            "low sky takes {ratio:.2}x the pixels of an equal high band; was ~4x before"
        );
    }

    /// `view_dir` is the inverse of `to_sky_fracs`, and the analytic sky depends on it exactly: it samples radiance per pixel while the renderer places the sun disc by projection, so any drift separates the bright spot from the disc.
    #[test]
    fn view_dir_inverts_the_projection() {
        for (alt, az) in [(10.0, 180.0), (35.0, 210.0), (55.0, 150.0), (5.0, 140.0)] {
            let altaz = AltAz {
                altitude: alt,
                azimuth: az,
            };
            let (x, y) = to_sky_fracs(&altaz, 180.0).expect("test points face the viewer");
            let dir = view_dir(x, y);
            let back_alt = dir[1].asin().to_degrees();
            let back_az = 180.0 + deg(f64::atan2(dir[0], dir[2]));
            assert!(
                (back_alt - alt).abs() < 1e-6,
                "altitude round trip {alt} -> {back_alt}"
            );
            assert!(
                (back_az - az).abs() < 1e-6,
                "azimuth round trip {az} -> {back_az}"
            );
        }
    }
}
