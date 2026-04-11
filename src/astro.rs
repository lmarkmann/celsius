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

fn to_horizontal(ra: f64, dec: f64, lat: f64, lon: f64, jd_val: f64) -> AltAz {
    let ha = rad(norm(gast(jd_val) + lon - ra));
    let dec_r = rad(dec);
    let lat_r = rad(lat);

    let sin_alt = lat_r.sin() * dec_r.sin() + lat_r.cos() * dec_r.cos() * ha.cos();
    let altitude = deg(sin_alt.clamp(-1.0, 1.0).asin());

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

pub fn moon_state(lat: f64, lon: f64, unix_utc: i64) -> MoonState {
    let jd_val = jd(unix_utc);
    let t = jc(jd_val);

    // Fundamental arguments (Meeus ch. 47, degrees)
    let lp = norm(218.3164477 + 481_267.881_234_21 * t); // moon mean longitude
    let d = rad(norm(297.8501921 + 445_267.111_403_4 * t)); // mean elongation
    let ms = rad(norm(357.5291092 + 35_999.050_290_9 * t)); // sun mean anomaly
    let mm = rad(norm(134.9633964 + 477_198.867_505_5 * t)); // moon mean anomaly
    let f = rad(norm(93.2720950 + 483_202.017_523_3 * t)); // argument of latitude

    // Longitude perturbations (Meeus Table 47.A, 20 largest terms, units: 0.001 arcsec -> divide by 1e6 for deg)
    // Coefficients are in units of 0.000001 degrees
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

    // Phase: elongation between moon and sun in ecliptic longitude.
    // Reuse t (already computed); skip the full horizontal transform.
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

pub fn to_sky_fracs(altaz: &AltAz, center_az: f64) -> (f64, f64) {
    // y_frac: altitude 90 (zenith) -> 0, altitude 0 (horizon) -> 1, clamp below
    let y_frac = 1.0 - altaz.altitude / 90.0;

    // x_frac: azimuth delta from center mapped to 0..1 over a 180-deg window
    let delta_az = norm(altaz.azimuth - center_az + 180.0) - 180.0; // -180..180
    let x_frac = 0.5 + delta_az / 180.0;

    (x_frac.clamp(0.0, 1.0), y_frac.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference: USNO solar calculator, Washington DC (38.9N, 77.0W), 2025-06-21 solar noon.
    // Solar noon at -77 lon is ~17:08 UTC (77/15 = 5.13h offset + small equation-of-time term).
    // Expected: altitude ~74 deg, azimuth ~180 deg.
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

    // Reference: USNO, same location, 2025-12-21 UTC noon.
    // Winter solstice: sun altitude much lower, still roughly south.
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

    // At north pole on summer solstice, sun altitude ~ 23.5 deg (axial tilt)
    // and it circles the horizon, never setting.
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

    // Moon phase sanity: 2025-01-29 was a full moon.
    // Illumination should be close to 1.0.
    #[test]
    fn moon_full_2025_jan_29() {
        // 2025-01-29 18:36 UTC (new moon is wrong, let me use a full moon)
        // Full moon: 2025-01-13 22:27 UTC
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

    // Moon phase sanity: 2025-01-29 was a new moon.
    // Illumination should be close to 0.
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

    #[test]
    fn sky_fracs_sun_at_south_horizon() {
        let altaz = AltAz {
            altitude: 0.0,
            azimuth: 180.0,
        };
        let (x, y) = to_sky_fracs(&altaz, 180.0);
        assert!((x - 0.5).abs() < 1e-9, "x_frac should be 0.5 for due south");
        assert!((y - 1.0).abs() < 1e-9, "y_frac should be 1.0 at horizon");
    }

    #[test]
    fn sky_fracs_sun_at_zenith() {
        let altaz = AltAz {
            altitude: 90.0,
            azimuth: 180.0,
        };
        let (x, y) = to_sky_fracs(&altaz, 180.0);
        assert!((x - 0.5).abs() < 1e-9);
        assert!((y - 0.0).abs() < 1e-9, "y_frac should be 0.0 at zenith");
    }
}
