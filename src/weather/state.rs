//! Forecast plus coordinates in, [`SkyState`] out. The synthesis layer.
//!
//! `compose` is where numbers become a sky. Solar and lunar position come from the `astro` module; a palette is chosen by sun altitude with overcast overrides; cloud layers are built from the low/mid/high cover triple at fixed altitudes; stars fade in below -3 degrees; the rain, showers and snowfall fields decide what falls and the WMO code whether a storm gets lightning; the model's own direct beam decides how much of the sun and of the clear-sky model to draw; and one `Atmosphere` built from optical depth, or visibility when that is missing, feeds both the haze layer and the analytic sky.
//!
//! The judgement calls worth knowing. Cloud seeds mix `(lat, lon, day)` so a sky reshapes once per UTC day rather than every hour, which stops clouds boiling as you scrub the timeline. `compose_at` interpolates between forecast hours for scalars but snaps weather codes to the nearer hour, because a code is categorical and half of "thunderstorm" is not a thing. Wind direction becomes a lateral offset from the facing bearing, so rain leans the way the wind is actually blowing relative to the viewer.
//!
//! Times are unix UTC seconds straight from the forecast. The location's offset is applied only for display and for finding the local day in the daily block.

use chrono::{Datelike, TimeZone, Timelike, Utc};

use crate::analytic_sky::AnalyticSky;
use crate::astro::{self, AltAz};
use crate::atmosphere::Atmosphere;
use crate::lightning::Lightning;
use crate::meteors::Meteors;
use crate::scene::{
    Chrome, CloudKind, CloudLayer, Haze, HorizonGlow, Moon, PrecipKind, Precipitation, SkyState,
    Stars, Sun,
};
use crate::snow::{self, FlakeForm, Snowfall};

use super::WeatherError;
use super::bortle;
use super::forecast::{DailyArrays, Forecast};
use super::gradients::{Palette, fog_gradient, gradient_for, sky_gradient};
use super::location::GeoResult;

const KEYS_HINT: &str = "<- -> scrub   tab day   t now   l location   ? help   q quit";

// Key hints from richest to a single `? help` floor. The TUI drops down this list before sacrificing any footer weather data, then holds `? help` until the too-small gate; `?` opens the overlay that lists every binding.
const KEYS_TIERS: [&str; 4] = [
    KEYS_HINT,
    "tab day   l location   ? help   q quit",
    "? help   q quit",
    "? help",
];

// The weather fields the sky is built from, already resolved (and possibly interpolated between two hours) so the builder never indexes the forecast.
struct HourSample {
    temperature_c: Option<f64>,
    relative_humidity: Option<f64>,
    reported_cover: Option<f64>,
    cover_low: f64,
    cover_mid: f64,
    cover_high: f64,
    precip_mm: Option<f64>,
    rain_mm: Option<f64>,
    showers_mm: Option<f64>,
    snowfall_cm: Option<f64>,
    precip_probability: Option<f64>,
    cape: Option<f64>,
    dni: Option<f64>,
    aod_550: Option<f64>,
    wind_speed: Option<f64>,
    wind_dir: Option<f64>,
    visibility_m: Option<f64>,
    weather_code: Option<u32>,
}

impl HourSample {
    fn at(forecast: &Forecast, h: usize) -> Self {
        let hr = &forecast.hourly;
        HourSample {
            temperature_c: hr.temperature_2m[h],
            relative_humidity: hr.relative_humidity_2m.get(h).copied().flatten(),
            reported_cover: hr.cloud_cover.get(h).copied().flatten(),
            cover_low: hr.cloud_cover_low[h].unwrap_or(0.0) / 100.0,
            cover_mid: hr.cloud_cover_mid[h].unwrap_or(0.0) / 100.0,
            cover_high: hr.cloud_cover_high[h].unwrap_or(0.0) / 100.0,
            precip_mm: hr.precipitation[h],
            rain_mm: hr.rain.get(h).copied().flatten(),
            showers_mm: hr.showers.get(h).copied().flatten(),
            snowfall_cm: hr.snowfall.get(h).copied().flatten(),
            precip_probability: hr.precipitation_probability.get(h).copied().flatten(),
            cape: hr.cape.get(h).copied().flatten(),
            dni: hr
                .direct_normal_irradiance_instant
                .get(h)
                .copied()
                .flatten(),
            aod_550: hr.aerosol_optical_depth.get(h).copied().flatten(),
            wind_speed: hr.wind_speed_10m[h],
            wind_dir: hr.wind_direction_10m[h],
            visibility_m: hr.visibility[h],
            weather_code: hr.weather_code[h],
        }
    }

    fn interpolated(forecast: &Forecast, h0: usize, h1: usize, frac: f64) -> Self {
        let a = Self::at(forecast, h0);
        let b = Self::at(forecast, h1);
        HourSample {
            temperature_c: lerp_opt(a.temperature_c, b.temperature_c, frac),
            relative_humidity: lerp_opt(a.relative_humidity, b.relative_humidity, frac),
            reported_cover: lerp_opt(a.reported_cover, b.reported_cover, frac),
            cover_low: lerp(a.cover_low, b.cover_low, frac),
            cover_mid: lerp(a.cover_mid, b.cover_mid, frac),
            cover_high: lerp(a.cover_high, b.cover_high, frac),
            precip_mm: lerp_opt(a.precip_mm, b.precip_mm, frac),
            rain_mm: lerp_opt(a.rain_mm, b.rain_mm, frac),
            showers_mm: lerp_opt(a.showers_mm, b.showers_mm, frac),
            snowfall_cm: lerp_opt(a.snowfall_cm, b.snowfall_cm, frac),
            precip_probability: lerp_opt(a.precip_probability, b.precip_probability, frac),
            cape: lerp_opt(a.cape, b.cape, frac),
            dni: lerp_opt(a.dni, b.dni, frac),
            aod_550: lerp_opt(a.aod_550, b.aod_550, frac),
            wind_speed: lerp_opt(a.wind_speed, b.wind_speed, frac),
            wind_dir: lerp_angle_opt(a.wind_dir, b.wind_dir, frac),
            visibility_m: lerp_opt(a.visibility_m, b.visibility_m, frac),
            // Weather codes are categorical; snap to the nearer hour.
            weather_code: if frac < 0.5 {
                a.weather_code
            } else {
                b.weather_code
            },
        }
    }
}

/// View options shared by every sky in a timeline: where the camera points, how dark the site is, and which model paints the daytime background.
///
/// Not exhaustively constructible from outside, for the same reason `Config` is not: this is the surface the atmosphere work extends, and every axis added to it (ground albedo, aerosol species) would otherwise break each caller writing a struct literal. Start from [`ComposeOpts::new`], or from `default()` when the facing does not matter.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct ComposeOpts {
    /// Azimuth at the horizontal center of the frame, in degrees (180 = south).
    pub center_az: f64,
    /// Bortle dark-sky class for the light-pollution glow; None = auto.
    pub bortle: Option<u8>,
    /// Preetham analytic daytime background instead of the palette gradient.
    pub analytic: bool,
}

impl ComposeOpts {
    /// Facing is the one option with no universal default, since which way is worth looking depends on the hemisphere. The rest have real defaults and are set through the `with_` methods.
    #[must_use]
    pub fn new(center_az: f64) -> Self {
        Self {
            center_az,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_bortle(mut self, bortle: Option<u8>) -> Self {
        self.bortle = bortle;
        self
    }

    #[must_use]
    pub fn with_analytic(mut self, analytic: bool) -> Self {
        self.analytic = analytic;
        self
    }
}

impl Default for ComposeOpts {
    fn default() -> Self {
        Self {
            center_az: 180.0,
            bortle: None,
            analytic: true,
        }
    }
}

/// Build a sky for the forecast hour at `hour_index`.
///
/// # Errors
///
/// None today. The hourly measurements are all optional, so a forecast with gaps in them still composes; the `Result` stays so a forecast shape that cannot compose has somewhere to go without changing every caller.
pub fn compose(
    forecast: &Forecast,
    location: &GeoResult,
    hour_index: usize,
    now_unix: i64,
    opts: ComposeOpts,
) -> Result<SkyState, WeatherError> {
    let h = hour_index.min(forecast.hourly.len().saturating_sub(1));
    let unix_utc = forecast.hourly.time[h];
    let sample = HourSample::at(forecast, h);
    Ok(build_sky(
        &sample,
        location,
        unix_utc,
        now_unix,
        forecast.utc_offset_seconds,
        forecast.daily.as_ref(),
        opts,
    ))
}

/// Build a sky for an exact instant, interpolating the weather fields and the sun/moon position between the bracketing forecast hours instead of snapping to the top of the hour. This is what makes the live "now" view show the sky for 14:23 rather than 14:00.
///
/// # Errors
///
/// None today, for the same reason as [`compose`].
pub fn compose_at(
    forecast: &Forecast,
    location: &GeoResult,
    target_unix: i64,
    now_unix: i64,
    opts: ComposeOpts,
) -> Result<SkyState, WeatherError> {
    let (h0, h1, frac) = bracket_hours(forecast, target_unix);
    let sample = HourSample::interpolated(forecast, h0, h1, frac);
    Ok(build_sky(
        &sample,
        location,
        target_unix,
        now_unix,
        forecast.utc_offset_seconds,
        forecast.daily.as_ref(),
        opts,
    ))
}

// Locate the two forecast hours straddling target_unix and the 0..1 fraction between them. Clamps to the ends when the target falls outside the range.
fn bracket_hours(forecast: &Forecast, target_unix: i64) -> (usize, usize, f64) {
    let times = &forecast.hourly.time;
    let last = times.len().saturating_sub(1);
    let h0 = times.iter().rposition(|&t| t <= target_unix).unwrap_or(0);
    if h0 >= last {
        return (last, last, 0.0);
    }
    let span = (times[h0 + 1] - times[h0]).max(1) as f64;
    let frac = ((target_unix - times[h0]) as f64 / span).clamp(0.0, 1.0);
    (h0, h0 + 1, frac)
}

fn build_sky(
    sample: &HourSample,
    location: &GeoResult,
    unix_utc: i64,
    now_unix: i64,
    offset: i64,
    daily: Option<&DailyArrays>,
    opts: ComposeOpts,
) -> SkyState {
    let ComposeOpts {
        center_az,
        bortle,
        analytic,
    } = opts;
    let lat = location.latitude;
    let lon = location.longitude;
    let sun_altaz = astro::sun_position(lat, lon, unix_utc);
    let moon_state = astro::moon_state(lat, lon, unix_utc);

    let atmosphere = Atmosphere::from_readings(sample.visibility_m, sample.aod_550);
    let total_cover = total_cover(
        sample.reported_cover,
        sample.cover_low,
        sample.cover_mid,
        sample.cover_high,
    );
    let sun_through = sun_transmission(sun_altaz.altitude, sample.dni, total_cover);
    let fog = fog_density(
        sample.weather_code,
        sample.relative_humidity,
        sample.visibility_m,
    );
    let socked_in = fog.is_some_and(|density| density > 0.5);

    let mut gradient = sky_gradient(sun_altaz.altitude, total_cover);
    if let Some(density) = fog {
        gradient = fog_gradient(&gradient, sun_altaz.altitude, density);
    }
    bortle::apply_glow(&mut gradient, bortle, sun_altaz.altitude);

    let day_ordinal = unix_utc.div_euclid(86_400);
    let sun = build_sun(&sun_altaz, center_az, sun_through);
    let moon = build_moon(&moon_state, center_az);
    let stars = if socked_in {
        None
    } else {
        build_stars(sun_altaz.altitude, lat, lon, day_ordinal, bortle)
    };
    let low_kind = low_cloud_kind(
        sample.weather_code.unwrap_or(0),
        sample.cover_low,
        sample.showers_mm.unwrap_or(0.0),
    );
    let clouds = build_clouds(
        sample.cover_low,
        sample.cover_mid,
        sample.cover_high,
        low_kind,
        lat,
        lon,
        day_ordinal,
    );
    // A bright-but-clouded daytime sky needs its own horizon haze regardless of reported visibility; this matches the old CloudyDay palette regime.
    let cloudy_day = sun_altaz.altitude > 3.0 && (0.50..0.80).contains(&total_cover);
    let haze = if let Some(density) = fog {
        Some(fog_veil(density, sun_altaz.altitude))
    } else if cloudy_day {
        Some(Haze {
            rgb: [178, 174, 165],
            onset_t: 0.55,
            strength: 0.48,
            exponent: 1.4,
        })
    } else {
        build_haze(&atmosphere)
    };
    // Rain and showers are the liquid share of the total; when neither was reported the total stands in and the code has to say whether it fell as snow.
    let (liquid_mm, split_reported) = match (sample.rain_mm, sample.showers_mm) {
        (None, None) => (sample.precip_mm, false),
        (rain, showers) => (Some(rain.unwrap_or(0.0) + showers.unwrap_or(0.0)), true),
    };
    let certainty = precipitation_certainty(sample.precip_probability);
    let precipitation = build_precipitation(
        sample.weather_code,
        liquid_mm,
        split_reported,
        certainty,
        sample.wind_dir,
        lat,
        lon,
        day_ordinal,
        center_az,
    );
    let snowfall = build_snowfall(
        sample.weather_code,
        sample.snowfall_cm,
        certainty,
        sample.temperature_c,
        sample.relative_humidity,
        sample.wind_dir,
        sample.wind_speed,
        lat,
        lon,
        day_ordinal,
        unix_utc.rem_euclid(86_400) as u64 / 3_600,
        center_az,
    );
    let lightning = build_lightning(
        sample.weather_code,
        sample.precip_mm,
        sample.cape,
        lat,
        lon,
        unix_utc,
    );
    let meteors = if socked_in {
        None
    } else {
        build_meteors(
            sun_altaz.altitude,
            total_cover,
            lat,
            lon,
            unix_utc,
            center_az,
            opts.bortle,
        )
    };

    let sun_day = daily.and_then(|d| sun_day_for(d, unix_utc, offset));
    let high_low = daily.and_then(|d| daily_high_low(d, unix_utc, offset));

    let chrome = build_chrome(
        location, unix_utc, now_unix, offset, sample, sun_day, high_low,
    );

    // Prototype: the analytic sky is daytime-only (Preetham's zenith formula breaks once the sun is below the horizon); twilight and night keep the palette gradient.
    let analytic_sky = (analytic && sun_altaz.altitude > 0.0).then(|| AnalyticSky {
        sun_alt: sun_altaz.altitude,
        sun_az: sun_altaz.azimuth,
        center_az,
        atmosphere,
        // Two things hold the model back. It ramps in over the first 8 degrees of solar elevation, so it crossfades out of the palette through twilight with no seam at sunrise; Preetham's zenith formula is not to be trusted that low anyway. And it is weighted by how much of the beam gets through, because Preetham describes a *clear* sky: run at full strength under an overcast deck it paints a clean blue-to-pale gradient and calls it a grey day. Cover used to do that job and did it too well, since a veil of cirrus reports full cover and hides nothing; the beam fraction lets the clear sky show through exactly as much as the sun does.
        blend: (sun_altaz.altitude / 8.0).clamp(0.0, 1.0) * sun_through,
    });

    SkyState {
        name: format!(
            "{}-{}",
            location.name.to_lowercase(),
            unix_utc.div_euclid(3_600)
        ),
        gradient,
        sun,
        clouds,
        chrome,
        haze,
        stars,
        moon,
        precipitation,
        snowfall,
        lightning,
        meteors,
        horizon_glow: build_horizon_glow(&sun_altaz, center_az, total_cover),
        analytic: analytic_sky,
        wind_speed_kmh: sample.wind_speed.unwrap_or(0.0),
        unix_utc,
    }
}

fn lerp(a: f64, b: f64, f: f64) -> f64 {
    a + (b - a) * f
}

fn lerp_opt(a: Option<f64>, b: Option<f64>, f: f64) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(lerp(x, y, f)),
        (some, None) | (None, some) => some,
    }
}

// Interpolate along the shortest arc so 350 -> 10 crosses through 0, not back through 180.
fn lerp_angle_opt(a: Option<f64>, b: Option<f64>, f: f64) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => {
            let diff = ((y - x + 540.0) % 360.0) - 180.0;
            Some((x + diff * f).rem_euclid(360.0))
        }
        (some, None) | (None, some) => some,
    }
}

fn build_lightning(
    weather_code: Option<u32>,
    precip_mm: Option<f64>,
    cape: Option<f64>,
    lat: f64,
    lon: f64,
    unix_utc: i64,
) -> Option<Lightning> {
    let code = weather_code?;
    if !(95..=99).contains(&code) {
        return None;
    }
    let with_bolts = matches!(code, 95 | 96 | 99);
    let intensity = storm_intensity(cape, precip_mm);
    let hour = unix_utc.div_euclid(3_600) as u64;
    let day_ordinal = unix_utc.div_euclid(86_400) as u64;
    let seed = mix_seed(&[hash_lat_lon(lat, lon), day_ordinal, hour, 0x1167_8175]) as u32;
    Some(Lightning::new(seed, intensity, 3_600.0, with_bolts))
}

// The code only says a storm exists; CAPE is its fuel, and 2500 J/kg is the conventional mark for large instability. Without it the rain rate stands in, which makes a wet stratiform storm flash harder than a dry violent one.
fn storm_intensity(cape: Option<f64>, precip_mm: Option<f64>) -> f64 {
    match cape {
        Some(cape) => (cape / 2500.0).clamp(0.20, 0.85),
        None => (precip_mm.unwrap_or(0.4) / 5.0).clamp(0.20, 0.85),
    }
}

// Extraterrestrial normal irradiance in W/m2. The 3.3 percent annual swing from the earth-sun distance is far below the clear-sky model's own error, so it is not tracked.
const SOLAR_CONSTANT: f64 = 1361.0;

/// How much of the sun gets through, 0 under a deck and 1 on a clear day: the model's direct beam against what a clean atmosphere would pass at this elevation.
///
/// This is the quantity the analytic sky's weight and the sun disc both want, and it beats cloud cover for both because cover is opaque by definition: a full sky of thin cirrus reports 100 percent and lets most of the beam through. The clean-atmosphere reference is Meinel's fit, `0.7^(m^0.678)` with Kasten-Young air mass, which lands on measured clear-sky DNI within about ten percent above ten degrees of elevation. Below that both the fit and the model drift, and that is the band the altitude ramp on the analytic blend already fades. Without a beam reading the old cover fade stands in.
fn sun_transmission(sun_alt_deg: f64, dni: Option<f64>, total_cover: f64) -> f64 {
    match dni {
        Some(dni) if sun_alt_deg > 0.0 => {
            (dni / SOLAR_CONSTANT / clear_sky_beam(sun_alt_deg)).clamp(0.0, 1.0)
        }
        _ => (1.0 - total_cover).clamp(0.0, 1.0),
    }
}

fn clear_sky_beam(sun_alt_deg: f64) -> f64 {
    let alt = sun_alt_deg.max(0.0);
    let air_mass = 1.0 / (alt.to_radians().sin() + 0.50572 * (alt + 6.07995).powf(-1.6364));
    0.7f64.powf(air_mass.powf(0.678))
}

// A twenty percent chance on day six must not paint the same downpour as certain rain in an hour. The floor keeps light-but-certain rain from vanishing when a model reports low probabilities for drizzle.
fn precipitation_certainty(probability_pct: Option<f64>) -> f64 {
    probability_pct.map_or(1.0, |p| (p / 100.0).clamp(0.3, 1.0))
}

/// Fog as a density in 0.4..=1, or `None` when the air is merely humid. Either the code says fog (45, 48) or the hour is saturated with under a kilometre of visibility, which is the WMO definition. Mist, one to four kilometres, stays with the ordinary haze curve, which is already at 0.8 by two.
fn fog_density(
    weather_code: Option<u32>,
    relative_humidity: Option<f64>,
    visibility_m: Option<f64>,
) -> Option<f64> {
    let coded = matches!(weather_code, Some(45 | 48));
    let saturated =
        relative_humidity.is_some_and(|rh| rh >= 97.0) && visibility_m.is_some_and(|v| v < 1000.0);
    if !coded && !saturated {
        return None;
    }
    Some(visibility_m.map_or(0.8, |v| (1.0 - v / 1000.0).clamp(0.4, 1.0)))
}

// The fog scene's veil: a full-frame haze, daylight grey, going dark with the sun, since fog at night is lit only by whatever the ground throws up.
fn fog_veil(density: f64, sun_alt_deg: f64) -> Haze {
    let night = bortle::night_factor(sun_alt_deg);
    let day = [208.0, 207.0, 202.0];
    let dark = [60.0, 62.0, 66.0];
    let rgb = [0, 1, 2].map(|i| (day[i] + (dark[i] - day[i]) * night).round() as u8);
    Haze {
        rgb,
        onset_t: 0.0,
        strength: 0.55 + 0.30 * density,
        exponent: 1.15,
    }
}

// Meteors show on a dark, clear-enough sky: sun well below the horizon and not overcast. Seeded per (place, day) like the clouds and lightning.
fn build_meteors(
    sun_alt: f64,
    total_cover: f64,
    lat: f64,
    lon: f64,
    unix_utc: i64,
    center_az: f64,
    bortle: Option<u8>,
) -> Option<Meteors> {
    if sun_alt > -3.0 || total_cover > 0.6 {
        return None;
    }
    let day_ordinal = unix_utc.div_euclid(86_400) as u64;
    let seed = mix_seed(&[hash_lat_lon(lat, lon), day_ordinal, 0x3A9F_C217]) as u32;
    // ZHR counts the whole hemisphere; the frame holds about a third of it, and light pollution takes its share on top.
    let rate_scale = astro::frame_solid_angle_fraction() * bortle::meteor_factor(bortle);
    Some(Meteors::new(
        seed, unix_utc, lat, lon, center_az, 3_600.0, rate_scale,
    ))
}

#[derive(Debug, Clone, PartialEq)]
enum SunDay {
    Times { rise_unix: i64, set_unix: i64 },
    PolarDay,
    PolarNight,
}

fn local_day(unix: i64, offset: i64) -> i64 {
    (unix + offset).div_euclid(86_400)
}

// Daily rows are local days and carry the instant of their local midnight, so the row for an hour is the one whose midnight falls on the same local date.
fn daily_row(daily: &DailyArrays, unix_utc: i64, offset: i64) -> Option<usize> {
    let day = local_day(unix_utc, offset);
    daily
        .time
        .iter()
        .position(|&midnight| local_day(midnight, offset) == day)
}

// Open-Meteo encodes polar day/night as sentinel values, not nulls: polar day -> daylight_duration == 86400, sunrise at local midnight, sunset at the next one. polar night -> daylight_duration == 0, sunrise == sunset == local midnight. Slop guards (>= 86_399, <= 1) are insurance against future float drift, not currently needed.
fn sun_day_for(daily: &DailyArrays, unix_utc: i64, offset: i64) -> Option<SunDay> {
    let i = daily_row(daily, unix_utc, offset)?;
    let dur = daily.daylight_duration.get(i).copied()?;
    if dur >= 86_399.0 {
        return Some(SunDay::PolarDay);
    }
    if dur <= 1.0 {
        return Some(SunDay::PolarNight);
    }
    Some(SunDay::Times {
        rise_unix: *daily.sunrise.get(i)?,
        set_unix: *daily.sunset.get(i)?,
    })
}

// The day's high/low for the date of the displayed hour, so a scrubbed future hour shows that day's envelope, not today's. Both ends must be present or the footer omits the H/L segment entirely (no half pair).
fn daily_high_low(daily: &DailyArrays, unix_utc: i64, offset: i64) -> Option<(f64, f64)> {
    let i = daily_row(daily, unix_utc, offset)?;
    let high = daily.temperature_2m_max.get(i).copied().flatten()?;
    let low = daily.temperature_2m_min.get(i).copied().flatten()?;
    Some((high, low))
}

fn local_hhmm(unix_utc: i64, offset: i64) -> String {
    Utc.timestamp_opt(unix_utc + offset, 0)
        .single()
        .map(|dt| format!("{:02}:{:02}", dt.hour(), dt.minute()))
        .unwrap_or_default()
}

// Arrows ↑ U+2191 / ↓ U+2193 are East-Asian-Width Ambiguous: width 1 in western terminals (default), width 2 in CJK locales or when ambiguous-as-wide is set. Column math elsewhere assumes width 1; revisit if that changes.
fn format_sun_segment(sun_day: Option<&SunDay>, offset: i64) -> String {
    match sun_day {
        Some(SunDay::Times {
            rise_unix,
            set_unix,
        }) => format!(
            "   ↑ {}  ↓ {}",
            local_hhmm(*rise_unix, offset),
            local_hhmm(*set_unix, offset)
        ),
        Some(SunDay::PolarDay) => "   polar day".to_string(),
        Some(SunDay::PolarNight) => "   polar night".to_string(),
        None => String::new(),
    }
}

fn build_sun(altaz: &AltAz, center_az: f64, strength: f64) -> Sun {
    // Behind the viewing plane there is no screen position at all; park the disc off-frame and let `visible` do the hiding.
    let (x_frac, y_frac) = astro::to_sky_fracs(altaz, center_az).unwrap_or((-1.0, -1.0));
    let in_view = astro::in_view(altaz, center_az);
    Sun {
        x_frac,
        y_frac,
        radius: 3.5,
        visible: altaz.altitude > -2.0 && in_view && strength > 0.02,
        strength,
    }
}

fn build_moon(state: &astro::MoonState, center_az: f64) -> Option<Moon> {
    let in_view = astro::in_view(&state.altaz, center_az);
    if state.altaz.altitude <= 0.0 || !in_view {
        return None;
    }
    let (x_frac, y_frac) = astro::to_sky_fracs(&state.altaz, center_az)?;
    Some(Moon {
        x_frac,
        y_frac,
        radius: 6.5,
        phase: state.phase,
        visible: true,
    })
}

// Warm horizon light on the sun's side, strongest at sunrise/sunset and gone by full day or deep night. Cover damps it, since an overcast horizon has no glow.
fn build_horizon_glow(altaz: &AltAz, center_az: f64, total_cover: f64) -> Option<HorizonGlow> {
    if lateral_offset_deg(altaz.azimuth, center_az).abs() >= 90.0 {
        return None;
    }
    let alt = altaz.altitude;
    let altitude_falloff = if !(-8.0..=12.0).contains(&alt) {
        0.0
    } else if alt >= 0.0 {
        1.0 - alt / 12.0
    } else {
        1.0 - (-alt) / 8.0
    };
    let strength = altitude_falloff * (1.0 - 0.6 * total_cover.clamp(0.0, 1.0));
    if strength < 0.02 {
        return None;
    }
    let (x_frac, _) = astro::to_sky_fracs(altaz, center_az)?;
    Some(HorizonGlow {
        x_frac,
        rgb: [255, 138, 72],
        strength,
    })
}

fn lateral_offset_deg(azimuth: f64, center_az: f64) -> f64 {
    ((azimuth - center_az + 540.0) % 360.0) - 180.0
}

fn build_stars(
    sun_alt: f64,
    lat: f64,
    lon: f64,
    day_ordinal: i64,
    bortle_class: Option<u8>,
) -> Option<Stars> {
    if sun_alt >= -3.0 {
        return None;
    }
    let darkness = ((-sun_alt - 3.0) / 15.0).clamp(0.0, 1.0);
    let brightness = 0.55 + 0.45 * darkness;
    let base_count = (180.0 + 200.0 * darkness) as u32;
    let count = bortle::scale_count(base_count, bortle_class);
    let sky_threshold = 0.30 + 0.08 * darkness;
    Some(Stars {
        count,
        seed: mix_seed(&[hash_lat_lon(lat, lon), day_ordinal as u64, 0x57A4_5EED]),
        brightness,
        sky_threshold,
    })
}

// Total sky cover. Prefer Open-Meteo's own `cloud_cover` (computed across all levels); when it's missing, combine the three bands as independent occluders (1 - product of clear fractions) rather than averaging them, so a single fully-covered layer still reads as a fully-covered sky.
fn total_cover(reported: Option<f64>, low: f64, mid: f64, high: f64) -> f64 {
    let cover = match reported {
        Some(pct) => pct / 100.0,
        None => 1.0 - (1.0 - low) * (1.0 - mid) * (1.0 - high),
    };
    cover.clamp(0.0, 1.0)
}

fn build_clouds(
    cover_low: f64,
    cover_mid: f64,
    cover_high: f64,
    low_kind: CloudKind,
    lat: f64,
    lon: f64,
    day_ordinal: i64,
) -> Vec<CloudLayer> {
    let pos_hash = hash_lat_lon(lat, lon);
    let mut layers = Vec::new();
    let bands = [
        (cover_high, 0.20, 4.5, 2.4, 0u32, CloudKind::Cirrus),
        (cover_mid, 0.40, 3.6, 2.4, 1, CloudKind::Altocumulus),
        (cover_low, 0.60, 3.0, 2.2, 2, low_kind),
    ];
    for (cover, altitude_t, scale_x, scale_y, idx, kind) in bands {
        let seed = mix_seed(&[pos_hash, day_ordinal as u64, 0xC10D_5EED ^ idx as u64]);
        if let Some(layer) = cloud_layer(cover, altitude_t, scale_x, scale_y, seed, kind) {
            layers.push(layer);
        }
    }
    layers
}

// The low band carries the weather: showers and thunderstorms are convective towers (dark cumulonimbus), light/partly-cloudy skies are fair-weather cumulus, and anything else is a flat stratus deck. Any convective rain at all is a tower whatever the code says, since the code reports the dominant weather and an hour of rain with showers in it is still built of cumulonimbus.
fn low_cloud_kind(weather_code: u32, cover_low: f64, showers_mm: f64) -> CloudKind {
    if showers_mm >= 0.1 || (80..=82).contains(&weather_code) || (95..=99).contains(&weather_code) {
        CloudKind::Cumulonimbus
    } else if matches!(weather_code, 1 | 2) && cover_low < 0.6 {
        CloudKind::Cumulus
    } else {
        CloudKind::Stratus
    }
}

fn cloud_layer(
    cover: f64,
    altitude_t: f64,
    scale_x: f64,
    scale_y: f64,
    seed: u64,
    kind: CloudKind,
) -> Option<CloudLayer> {
    if cover < 0.05 {
        return None;
    }
    let threshold = 0.55 - 0.40 * cover;
    let cover_strength = 0.90 + 1.00 * (1.0 - cover);
    // A stratus deck flattens into a solid lid as it approaches full cover, and widens vertically so the overcast fills the sky rather than a thin band.
    let flatten = if kind == CloudKind::Stratus {
        let f = ((cover - 0.70) / 0.25).clamp(0.0, 1.0);
        f * f * (3.0 - 2.0 * f)
    } else {
        0.0
    };
    let altitude_sigma = 0.10 + 0.15 * flatten;
    Some(CloudLayer {
        cover: cover_strength,
        altitude_t,
        altitude_sigma,
        scale_x,
        scale_y,
        threshold,
        seed,
        kind,
        flatten,
        offset_x: ((seed >> 16) as f64 / u32::MAX as f64) * 4.0,
        offset_y: ((seed >> 32) as f64 / u32::MAX as f64) * 4.0,
    })
}

fn build_haze(atmosphere: &Atmosphere) -> Option<Haze> {
    let viz_km = atmosphere.visibility_m? / 1000.0;
    if viz_km >= 12.0 {
        return None;
    }
    let strength = ((12.0 - viz_km) / 12.0).clamp(0.0, 0.85);
    Some(Haze {
        rgb: [188, 180, 168],
        onset_t: 0.10,
        strength,
        exponent: 1.6,
    })
}

/// WMO codes for snow and snow showers. The single place the split between the two precipitation renderers is decided, so neither can claim an hour the other is also drawing.
fn is_snow_code(weather_code: Option<u32>) -> bool {
    let code = weather_code.unwrap_or(0);
    (71..=77).contains(&code) || (85..=86).contains(&code)
}

/// Falling snow for this hour, or `None` when it is not snowing.
///
/// The hour is in the seed, which precipitation's is not. A rain seed carries only the place and the UTC day, so scrubbing a whole forecast day past shows one frozen arrangement of drops for twenty-four hours; clouds get away with that because they are meant to reshape once a day, and falling precipitation is not.
#[allow(clippy::too_many_arguments)]
fn build_snowfall(
    weather_code: Option<u32>,
    snowfall_cm: Option<f64>,
    certainty: f64,
    temperature_c: Option<f64>,
    relative_humidity: Option<f64>,
    wind_dir: Option<f64>,
    wind_speed: Option<f64>,
    lat: f64,
    lon: f64,
    day_ordinal: i64,
    hour_of_day: u64,
    center_az: f64,
) -> Option<Snowfall> {
    if !is_snow_code(weather_code) {
        return None;
    }
    // A snow code with no reported accumulation is still snow; fall back to something light rather than drawing an empty sky.
    let rate = snowfall_cm.unwrap_or(0.0).max(0.05);
    // The diagram is drawn against the humidity where the crystal grew, and the forecast reports it at 2 m, so this is an approximation and not a measurement: what is below the cloud is drier than what is inside it. The fallback is 95 rather than a round 90 because it only fires when the field is missing during an hour already coded as snow, and the split between faceted and branched sits at 93 percent at -15 C, so 90 would quietly report every sky as faceted.
    let form = FlakeForm::select(
        temperature_c.unwrap_or(-3.0),
        relative_humidity.unwrap_or(95.0),
    );
    Some(Snowfall {
        form,
        count: (f64::from(snow::flake_count(rate)) * certainty)
            .round()
            .max(1.0) as u32,
        seed: mix_seed(&[
            hash_lat_lon(lat, lon),
            day_ordinal as u64,
            hour_of_day,
            0x0F1E_ECE5,
        ]),
        drift: snow_drift(wind_dir, wind_speed, center_az),
        opacity: 0.75,
    })
}

/// Sideways travel in frame widths per second.
///
/// Only the across-view component moves a flake on screen: wind blowing away from the viewer changes nothing a flat frame can show. Scaled so a 20 km/h crosswind carries a flake across the frame in about twenty seconds, and capped, because a gale should lean the snow rather than fire it sideways faster than the eye can follow.
fn snow_drift(wind_dir: Option<f64>, wind_speed: Option<f64>, center_az: f64) -> f64 {
    let speed = wind_speed.unwrap_or(0.0);
    let lateral = lateral_offset_deg(wind_dir.unwrap_or(180.0), center_az)
        .to_radians()
        .sin();
    (lateral * speed * 0.0025).clamp(-0.12, 0.12)
}

/// Rain streaks for this hour, from the liquid share of what fell. `split_reported` says whether that share came from the rain and showers fields or is the whole total, in which case the code has to keep snow hours away from the rain renderer; with the split, a sleet hour draws both.
#[allow(clippy::too_many_arguments)]
fn build_precipitation(
    weather_code: Option<u32>,
    liquid_mm: Option<f64>,
    split_reported: bool,
    certainty: f64,
    wind_dir: Option<f64>,
    lat: f64,
    lon: f64,
    day_ordinal: i64,
    center_az: f64,
) -> Option<Precipitation> {
    let mm = liquid_mm.unwrap_or(0.0);
    if mm < 0.10 {
        return None;
    }
    if !split_reported && is_snow_code(weather_code) {
        return None;
    }
    let intensity = (mm / 5.0).clamp(0.10, 0.85) * certainty;
    let dir = wind_dir.unwrap_or(180.0);
    let delta = lateral_offset_deg(dir, center_az);
    let angle_deg = (delta * 0.30).clamp(-25.0, 25.0);
    let seed = mix_seed(&[hash_lat_lon(lat, lon), day_ordinal as u64, 0xBA17_DA75]);
    Some(Precipitation {
        kind: PrecipKind::Rain,
        intensity,
        angle_deg,
        seed,
        streak_len: 4,
        opacity: 0.40,
    })
}

// Footer payload tiers, richest to poorest, dropping lowest-value first: abbreviate wind (lose the word "wind"), then the wind value, then H/L, with the condition word and temperature held longest. `temp` alone is the floor.
fn footer_tiers(
    temp: &str,
    high_low: Option<&str>,
    word: &str,
    compass: &str,
    speed: i64,
) -> Vec<String> {
    let wind_full = format!("wind {compass} {speed}");
    let wind_short = format!("{compass} {speed}");
    match high_low {
        Some(hl) => vec![
            format!("{temp}  {hl}   {word}   {wind_full}"),
            format!("{temp}  {hl}   {word}   {wind_short}"),
            format!("{temp}  {hl}   {word}"),
            format!("{temp}  {word}"),
            temp.to_string(),
        ],
        None => vec![
            format!("{temp}  {word}   {wind_full}"),
            format!("{temp}  {word}   {wind_short}"),
            format!("{temp}  {word}"),
            temp.to_string(),
        ],
    }
}

fn build_chrome(
    location: &GeoResult,
    unix_utc: i64,
    now_unix: i64,
    offset: i64,
    sample: &HourSample,
    sun_day: Option<SunDay>,
    high_low: Option<(f64, f64)>,
) -> Chrome {
    let header_left = "celsius".to_string();
    let header_right = format!(
        "{}   {}{}",
        location.label(),
        format_label(unix_utc, now_unix, offset),
        format_sun_segment(sun_day.as_ref(), offset),
    );

    let temp = sample
        .temperature_c
        .map(|t| format!("{:.0}°", t))
        .unwrap_or_else(|| "--°".to_string());
    let word = wmo_word(sample.weather_code.unwrap_or(0));
    let speed = sample.wind_speed.unwrap_or(0.0).round() as i64;
    let compass = compass_from_deg(sample.wind_dir.unwrap_or(0.0));
    let high_low = high_low.map(|(hi, lo)| format!("H{hi:.0} L{lo:.0}"));
    let footer = format!("{temp}  {word}   wind {compass} {speed}");

    // ASCII one-liner for --plain: carries the place name, no degree sign. Grep- and pipe-friendly, distinct from the decorative footer.
    let temp_ascii = sample
        .temperature_c
        .map(|t| format!("{t:.0}C"))
        .unwrap_or_else(|| "--C".to_string());
    let status = format!(
        "{} {temp_ascii} {word} wind {compass} {speed}",
        location.name
    );

    Chrome {
        header_left,
        header_right,
        footer,
        keys: KEYS_HINT.to_string(),
        status,
        footer_tiers: footer_tiers(&temp, high_low.as_deref(), word, compass, speed),
        keys_tiers: KEYS_TIERS.iter().map(|s| s.to_string()).collect(),
    }
}

fn format_label(unix_utc: i64, now_unix: i64, offset: i64) -> String {
    let target = match Utc.timestamp_opt(unix_utc + offset, 0).single() {
        Some(dt) => dt,
        None => {
            // The offset pushed the timestamp out of range, so fall back to bare UTC. If that is out of range too there is no date to format, and the raw epoch beats a panic in the branch whose whole job is not failing.
            return match Utc.timestamp_opt(unix_utc, 0).single() {
                Some(dt) => format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}",
                    dt.year(),
                    dt.month(),
                    dt.day(),
                    dt.hour(),
                    dt.minute()
                ),
                None => format!("t={unix_utc}"),
            };
        }
    };
    let now = Utc
        .timestamp_opt(now_unix + offset, 0)
        .single()
        .unwrap_or(target);
    let day_diff = (target.date_naive() - now.date_naive()).num_days();
    let hhmm = format!("{:02}:{:02}", target.hour(), target.minute());
    match day_diff {
        0 => format!("today {hhmm}"),
        1 => format!("tomorrow {hhmm}"),
        -1 => format!("yesterday {hhmm}"),
        _ => {
            let weekday = WEEKDAYS[target.weekday().num_days_from_monday() as usize];
            let month = MONTHS[(target.month() - 1) as usize];
            format!("{} {} {} {}", weekday, target.day(), month, hhmm)
        }
    }
}

const WEEKDAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

fn compass_from_deg(deg: f64) -> &'static str {
    const DIRS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    let idx = (((deg / 45.0).round() as i64).rem_euclid(8)) as usize;
    DIRS[idx]
}

fn wmo_word(code: u32) -> &'static str {
    match code {
        0 => "clear",
        1 => "mostly clear",
        2 => "partly cloudy",
        3 => "overcast",
        45 | 48 => "fog",
        51..=57 => "drizzle",
        61..=63 => "rain",
        65..=67 => "heavy rain",
        71..=73 => "snow",
        75..=77 => "heavy snow",
        80..=82 => "showers",
        85..=86 => "snow showers",
        95 => "thunderstorms",
        96..=99 => "thunder + hail",
        _ => "unknown",
    }
}

fn hash_lat_lon(lat: f64, lon: f64) -> u64 {
    let lat_bits = (lat * 1000.0).round() as i64;
    let lon_bits = (lon * 1000.0).round() as i64;
    mix_seed(&[lat_bits as u64, lon_bits as u64])
}

pub fn error_sky(msg: &str) -> SkyState {
    let gradient = gradient_for(Palette::Night);
    let first_line = msg.lines().next().unwrap_or(msg);
    // Truncate on a char boundary: error text carries user-supplied location labels, and a byte slice through a multibyte char would panic the one path that must never panic.
    let footer = match first_line.char_indices().nth(72) {
        Some((cut, _)) => format!("{}...", &first_line[..cut]),
        None => first_line.to_string(),
    };
    SkyState {
        name: "error".to_string(),
        gradient,
        sun: Sun {
            x_frac: 0.5,
            y_frac: 1.5,
            radius: 0.0,
            visible: false,
            strength: 0.0,
        },
        clouds: vec![],
        chrome: Chrome {
            header_left: "celsius".to_string(),
            header_right: String::new(),
            footer: footer.clone(),
            keys: "r retry   q quit".to_string(),
            status: footer,
            footer_tiers: Vec::new(),
            keys_tiers: Vec::new(),
        },
        haze: None,
        stars: None,
        moon: None,
        precipitation: None,
        snowfall: None,
        lightning: None,
        meteors: None,
        horizon_glow: None,
        analytic: None,
        wind_speed_kmh: 0.0,
        unix_utc: 0,
    }
}

/// FNV-1a over the little-endian bytes of each part. Cloud, star and precipitation seeds must reproduce across toolchains; DefaultHasher's algorithm is explicitly not guaranteed between Rust releases.
fn mix_seed(parts: &[u64]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in part.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_seed_is_stable_across_toolchains() {
        // FNV-1a reference vectors, computed independently. If these move, every daily cloud/star/precip seed moves with them.
        assert_eq!(mix_seed(&[]), 0xcbf2_9ce4_8422_2325);
        assert_eq!(mix_seed(&[0]), 0xa8c7_f832_281a_39c5);
        assert_eq!(mix_seed(&[1, 2]), 0x7717_9803_63c8_e066);
    }

    // 2026-06-15T18:00Z, verified against `date -u -r 1781546400`.
    const JUNE_15_18Z: i64 = 1_781_546_400;
    // 2026-04-11T00:00Z, the forecast fixture's first hour.
    const APRIL_11: i64 = 1_775_865_600;

    #[test]
    fn local_hhmm_applies_offset() {
        let unix = JUNE_15_18Z;
        assert_eq!(local_hhmm(unix, 8 * 3600), "02:00");
        assert_eq!(local_hhmm(unix, 0), "18:00");
    }

    #[test]
    fn format_label_uses_location_offset_not_machine() {
        // 18:00Z is 02:00 the next local day at UTC+8; with "now" at the same instant the header reads the location's wall clock, not the machine's.
        let unix = JUNE_15_18Z;
        assert_eq!(format_label(unix, unix, 8 * 3600), "today 02:00");
        let next = unix + 86_400;
        assert_eq!(format_label(next, unix, 8 * 3600), "tomorrow 02:00");
    }

    #[test]
    fn error_sky_truncates_multibyte_text_without_panicking() {
        // The two-byte ü starts at byte 71, so the old `&first_line[..72]` sliced through it and panicked the error path itself.
        let msg = format!("{}ürich weather fetch failed", "Z".repeat(71));
        let sky = error_sky(&msg);
        assert!(sky.chrome.footer.ends_with("..."));
        assert_eq!(sky.chrome.footer.chars().count(), 75);

        let short = error_sky("kaputt");
        assert_eq!(short.chrome.footer, "kaputt");
    }

    #[test]
    fn scalar_lerp_opt_handles_missing_sides() {
        assert_eq!(lerp_opt(Some(0.0), Some(10.0), 0.25), Some(2.5));
        assert_eq!(lerp_opt(Some(4.0), None, 0.9), Some(4.0));
        assert_eq!(lerp_opt(None, Some(7.0), 0.1), Some(7.0));
        assert_eq!(lerp_opt(None, None, 0.5), None);
    }

    #[test]
    fn angle_lerp_takes_short_arc() {
        // 350 -> 10 must cross through 0, so the midpoint is ~0/360, not ~180.
        let mid = lerp_angle_opt(Some(350.0), Some(10.0), 0.5).unwrap();
        assert!(
            mid < 1e-6 || (360.0 - mid) < 1e-6,
            "midpoint {mid} took the long arc"
        );
        assert_eq!(lerp_angle_opt(None, Some(37.0), 0.5), Some(37.0));
    }

    #[test]
    fn total_cover_overcast_stratus_is_total() {
        // 100% low stratus, nothing above: the union must read as fully covered, not the (1.0+0+0)/3 = 0.33 the old averaging produced.
        assert!((total_cover(None, 1.0, 0.0, 0.0) - 1.0).abs() < 1e-9);
        // Two half-covered independent layers: 1 - 0.5*0.5 = 0.75.
        assert!((total_cover(None, 0.5, 0.5, 0.0) - 0.75).abs() < 1e-9);
        // A reported total wins over the bands and is rescaled from percent.
        assert!((total_cover(Some(40.0), 1.0, 1.0, 1.0) - 0.40).abs() < 1e-9);
    }

    #[test]
    fn compass_round() {
        assert_eq!(compass_from_deg(0.0), "N");
        assert_eq!(compass_from_deg(45.0), "NE");
        assert_eq!(compass_from_deg(180.0), "S");
        assert_eq!(compass_from_deg(270.0), "W");
        assert_eq!(compass_from_deg(360.0), "N");
    }

    #[test]
    fn wmo_words_cover_common_codes() {
        assert_eq!(wmo_word(0), "clear");
        assert_eq!(wmo_word(3), "overcast");
        assert_eq!(wmo_word(63), "rain");
        assert_eq!(wmo_word(75), "heavy snow");
        assert_eq!(wmo_word(95), "thunderstorms");
    }

    fn daily_one_day(
        midnight: i64,
        sunrise: i64,
        sunset: i64,
        daylight_duration: f64,
    ) -> DailyArrays {
        DailyArrays {
            time: vec![midnight],
            sunrise: vec![sunrise],
            sunset: vec![sunset],
            daylight_duration: vec![daylight_duration],
            temperature_2m_max: vec![],
            temperature_2m_min: vec![],
        }
    }

    #[test]
    fn sun_day_normal_returns_times() {
        // +4h38m = +16_680, +18h14m = +65_640.
        let daily = daily_one_day(APRIL_11, APRIL_11 + 16_680, APRIL_11 + 65_640, 48_960.0);
        match sun_day_for(&daily, APRIL_11 + 12 * 3_600, 0) {
            Some(SunDay::Times {
                rise_unix,
                set_unix,
            }) => {
                assert_eq!(rise_unix, 1_775_882_280);
                assert_eq!(set_unix, 1_775_931_240);
            }
            other => panic!("expected Times, got {other:?}"),
        }
    }

    #[test]
    fn sun_day_polar_day_from_full_daylight() {
        let may_9 = APRIL_11 + 28 * 86_400;
        let daily = daily_one_day(may_9, may_9, may_9 + 86_400, 86_400.0);
        assert_eq!(
            sun_day_for(&daily, may_9 + 3_600, 0),
            Some(SunDay::PolarDay)
        );
    }

    #[test]
    fn sun_day_polar_night_from_zero_daylight() {
        let dec_22 = APRIL_11 - 110 * 86_400;
        let daily = daily_one_day(dec_22, dec_22, dec_22, 0.0);
        assert_eq!(
            sun_day_for(&daily, dec_22 + 3_600, 0),
            Some(SunDay::PolarNight)
        );
    }

    #[test]
    fn sun_day_unknown_date_returns_none() {
        let daily = daily_one_day(APRIL_11, APRIL_11 + 16_680, APRIL_11 + 65_640, 48_960.0);
        assert_eq!(sun_day_for(&daily, APRIL_11 + 86_400, 0), None);
    }

    #[test]
    fn daily_row_follows_the_local_date_not_the_utc_one() {
        // Local midnight at UTC+2 is 22:00Z the evening before. 23:00 local is still that row; 00:30 local the next day is not, though it is the same UTC date.
        let offset = 2 * 3_600;
        let midnight = APRIL_11 - offset;
        let daily = daily_one_day(midnight, midnight + 16_680, midnight + 65_640, 48_960.0);
        assert_eq!(daily_row(&daily, midnight + 23 * 3_600, offset), Some(0));
        assert_eq!(
            daily_row(&daily, midnight + 24 * 3_600 + 1_800, offset),
            None
        );
    }

    fn daily_with_high_low(midnight: i64, high: Option<f64>, low: Option<f64>) -> DailyArrays {
        DailyArrays {
            time: vec![midnight],
            sunrise: vec![midnight + 6 * 3_600],
            sunset: vec![midnight + 20 * 3_600],
            daylight_duration: vec![48_960.0],
            temperature_2m_max: vec![high],
            temperature_2m_min: vec![low],
        }
    }

    #[test]
    fn daily_high_low_reads_matching_day() {
        let daily = daily_with_high_low(APRIL_11, Some(22.4), Some(14.6));
        assert_eq!(
            daily_high_low(&daily, APRIL_11 + 15 * 3_600, 0),
            Some((22.4, 14.6))
        );
    }

    #[test]
    fn daily_high_low_missing_end_returns_none() {
        let daily = daily_with_high_low(APRIL_11, Some(22.0), None);
        assert_eq!(daily_high_low(&daily, APRIL_11, 0), None);
        assert_eq!(daily_high_low(&daily, APRIL_11 + 86_400, 0), None);
    }

    #[test]
    fn sun_transmission_reads_the_beam_when_there_is_one() {
        // A clean noon: Meinel passes about two thirds of the constant at 60 degrees, and the model reports exactly that much.
        let clear_noon = clear_sky_beam(60.0);
        assert!(clear_noon > 0.62 && clear_noon < 0.72, "got {clear_noon}");
        assert!(
            (sun_transmission(60.0, Some(SOLAR_CONSTANT * clear_noon), 0.0) - 1.0).abs() < 1e-9
        );
        assert_eq!(
            sun_transmission(60.0, Some(0.0), 0.0),
            0.0,
            "no beam under a deck"
        );
        let cirrus = sun_transmission(45.0, Some(400.0), 1.0);
        assert!(
            cirrus > 0.3 && cirrus < 0.8,
            "a veil reports full cover and still passes some sun, got {cirrus}"
        );
        assert_eq!(
            sun_transmission(45.0, None, 0.25),
            0.75,
            "no beam reading falls back to cover"
        );
        assert_eq!(
            sun_transmission(-1.0, Some(0.0), 0.25),
            0.75,
            "below the horizon the beam says nothing"
        );
    }

    #[test]
    fn storm_intensity_prefers_cape() {
        assert_eq!(storm_intensity(Some(3_000.0), Some(0.2)), 0.85);
        assert_eq!(storm_intensity(Some(400.0), Some(4.0)), 0.20);
        assert_eq!(storm_intensity(None, Some(2.5)), 0.5);
    }

    #[test]
    fn fog_needs_saturation_and_a_short_view_or_the_code() {
        assert_eq!(fog_density(Some(0), Some(99.0), Some(300.0)), Some(0.7));
        assert_eq!(fog_density(Some(0), Some(99.0), Some(8_000.0)), None);
        assert_eq!(fog_density(Some(0), Some(80.0), Some(300.0)), None);
        assert_eq!(fog_density(Some(45), None, None), Some(0.8));
        assert_eq!(fog_density(Some(48), Some(90.0), Some(5_000.0)), Some(0.4));
    }

    #[test]
    fn showers_make_the_low_deck_convective() {
        assert_eq!(low_cloud_kind(61, 0.8, 0.5), CloudKind::Cumulonimbus);
        assert_eq!(low_cloud_kind(61, 0.8, 0.0), CloudKind::Stratus);
        assert_eq!(low_cloud_kind(1, 0.3, 0.0), CloudKind::Cumulus);
    }

    #[test]
    fn precipitation_scales_with_its_probability() {
        let certain =
            build_precipitation(Some(61), Some(2.0), true, 1.0, None, 53.5, 10.0, 0, 180.0)
                .expect("rain");
        let unlikely = build_precipitation(
            Some(61),
            Some(2.0),
            true,
            precipitation_certainty(Some(20.0)),
            None,
            53.5,
            10.0,
            0,
            180.0,
        )
        .expect("still drawn, fainter");
        assert!((unlikely.intensity - certain.intensity * 0.3).abs() < 1e-9);
        assert_eq!(precipitation_certainty(None), 1.0);
    }

    #[test]
    fn a_split_total_lets_sleet_rain_while_a_bare_total_defers_to_the_code() {
        assert!(
            build_precipitation(Some(73), Some(1.0), true, 1.0, None, 53.5, 10.0, 0, 180.0)
                .is_some()
        );
        assert!(
            build_precipitation(Some(73), Some(1.0), false, 1.0, None, 53.5, 10.0, 0, 180.0)
                .is_none()
        );
    }

    #[test]
    fn format_sun_segment_branches() {
        assert_eq!(format_sun_segment(None, 0), "");
        assert_eq!(
            format_sun_segment(Some(&SunDay::PolarDay), 0),
            "   polar day"
        );
        assert_eq!(
            format_sun_segment(Some(&SunDay::PolarNight), 0),
            "   polar night"
        );
    }
}
