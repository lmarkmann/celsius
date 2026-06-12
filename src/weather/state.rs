use chrono::{Datelike, Local, NaiveDateTime, TimeZone, Utc};

use crate::analytic_sky::AnalyticSky;
use crate::astro::{self, AltAz};
use crate::lightning::Lightning;
use crate::scene::{
    Chrome, CloudKind, CloudLayer, Haze, HorizonGlow, Moon, PrecipKind, Precipitation, SkyState,
    Stars, Sun,
};

use super::WeatherError;
use super::bortle;
use super::forecast::{DailyArrays, Forecast};
use super::gradients::{Palette, gradient_for, sky_gradient};
use super::location::GeoResult;

const KEYS_HINT: &str = "<- -> scrub   tab day   t now   l location   ? help   q quit";

// The weather fields the sky is built from, already resolved (and possibly
// interpolated between two hours) so the builder never indexes the forecast.
struct HourSample {
    temperature_c: Option<f64>,
    reported_cover: Option<f64>,
    cover_low: f64,
    cover_mid: f64,
    cover_high: f64,
    precip_mm: Option<f64>,
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
            reported_cover: hr.cloud_cover.get(h).copied().flatten(),
            cover_low: hr.cloud_cover_low[h].unwrap_or(0.0) / 100.0,
            cover_mid: hr.cloud_cover_mid[h].unwrap_or(0.0) / 100.0,
            cover_high: hr.cloud_cover_high[h].unwrap_or(0.0) / 100.0,
            precip_mm: hr.precipitation[h],
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
            reported_cover: lerp_opt(a.reported_cover, b.reported_cover, frac),
            cover_low: lerp(a.cover_low, b.cover_low, frac),
            cover_mid: lerp(a.cover_mid, b.cover_mid, frac),
            cover_high: lerp(a.cover_high, b.cover_high, frac),
            precip_mm: lerp_opt(a.precip_mm, b.precip_mm, frac),
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

/// Build a sky for the forecast hour at `hour_index`.
pub fn compose(
    forecast: &Forecast,
    location: &GeoResult,
    hour_index: usize,
    now_unix: i64,
    center_az: f64,
    bortle: Option<u8>,
    analytic: bool,
) -> Result<SkyState, WeatherError> {
    let h = hour_index.min(forecast.hourly.len().saturating_sub(1));
    let unix_utc = parse_hour_to_unix(&forecast.hourly.time[h])?;
    let sample = HourSample::at(forecast, h);
    Ok(build_sky(
        &sample,
        location,
        unix_utc,
        now_unix,
        center_az,
        bortle,
        forecast.daily.as_ref(),
        analytic,
    ))
}

/// Build a sky for an exact instant, interpolating the weather fields and the
/// sun/moon position between the bracketing forecast hours instead of snapping
/// to the top of the hour. This is what makes the live "now" view show the sky
/// for 14:23 rather than 14:00.
pub fn compose_at(
    forecast: &Forecast,
    location: &GeoResult,
    target_unix: i64,
    now_unix: i64,
    center_az: f64,
    bortle: Option<u8>,
    analytic: bool,
) -> Result<SkyState, WeatherError> {
    let (h0, h1, frac) = bracket_hours(forecast, target_unix)?;
    let sample = HourSample::interpolated(forecast, h0, h1, frac);
    Ok(build_sky(
        &sample,
        location,
        target_unix,
        now_unix,
        center_az,
        bortle,
        forecast.daily.as_ref(),
        analytic,
    ))
}

// Locate the two forecast hours straddling target_unix and the 0..1 fraction
// between them. Clamps to the ends when the target falls outside the range.
fn bracket_hours(
    forecast: &Forecast,
    target_unix: i64,
) -> Result<(usize, usize, f64), WeatherError> {
    let times = &forecast.hourly.time;
    let last = times.len().saturating_sub(1);
    let mut h0 = 0usize;
    for (i, t) in times.iter().enumerate() {
        if parse_hour_to_unix(t)? <= target_unix {
            h0 = i;
        } else {
            break;
        }
    }
    if h0 >= last {
        return Ok((last, last, 0.0));
    }
    let t0 = parse_hour_to_unix(&times[h0])?;
    let t1 = parse_hour_to_unix(&times[h0 + 1])?;
    let span = (t1 - t0).max(1) as f64;
    let frac = ((target_unix - t0) as f64 / span).clamp(0.0, 1.0);
    Ok((h0, h0 + 1, frac))
}

// Open-Meteo visibility tops out near 24 km on clear days and falls to a few km
// in haze/fog. Map clear -> low turbidity (~2), hazy -> high (~9).
fn turbidity_from_visibility(vis_m: Option<f64>) -> f64 {
    let vis_km = vis_m.unwrap_or(24_000.0) / 1000.0;
    (2.0 + (24.0 - vis_km.clamp(2.0, 24.0)) / 22.0 * 7.0).clamp(2.0, 9.0)
}

// Prototype's `analytic` flag pushes this to 8 args; if the analytic sky
// graduates, bundle (center_az, bortle, analytic) into a render-opts struct.
#[expect(clippy::too_many_arguments)]
fn build_sky(
    sample: &HourSample,
    location: &GeoResult,
    unix_utc: i64,
    now_unix: i64,
    center_az: f64,
    bortle: Option<u8>,
    daily: Option<&DailyArrays>,
    analytic: bool,
) -> SkyState {
    let lat = location.latitude;
    let lon = location.longitude;
    let sun_altaz = astro::sun_position(lat, lon, unix_utc);
    let moon_state = astro::moon_state(lat, lon, unix_utc);

    let total_cover = total_cover(
        sample.reported_cover,
        sample.cover_low,
        sample.cover_mid,
        sample.cover_high,
    );

    let mut gradient = sky_gradient(sun_altaz.altitude, total_cover);
    bortle::apply_glow(&mut gradient, bortle, sun_altaz.altitude);

    let day_ordinal = unix_utc.div_euclid(86_400);
    let sun = build_sun(&sun_altaz, center_az);
    let moon = build_moon(&moon_state, center_az);
    let stars = build_stars(sun_altaz.altitude, lat, lon, day_ordinal, bortle);
    let clouds = build_clouds(
        sample.cover_low,
        sample.cover_mid,
        sample.cover_high,
        sample.weather_code,
        lat,
        lon,
        day_ordinal,
    );
    // A bright-but-clouded daytime sky needs its own horizon haze regardless of
    // reported visibility; this matches the old CloudyDay palette regime.
    let cloudy_day = sun_altaz.altitude > 3.0 && (0.50..0.80).contains(&total_cover);
    let haze = if cloudy_day {
        Some(Haze {
            rgb: [178, 174, 165],
            onset_t: 0.55,
            strength: 0.48,
            exponent: 1.4,
        })
    } else {
        build_haze(sample.visibility_m)
    };
    let precipitation = build_precipitation(
        sample.weather_code,
        sample.precip_mm,
        sample.wind_dir,
        lat,
        lon,
        day_ordinal,
        center_az,
    );
    let lightning = build_lightning(sample.weather_code, sample.precip_mm, lat, lon, unix_utc);

    let sun_day = daily.and_then(|d| sun_day_for(d, &utc_date_iso(unix_utc)));

    let chrome = build_chrome(location, unix_utc, now_unix, sample, sun_day);

    // Prototype: the analytic sky is daytime-only (Preetham's zenith formula
    // breaks once the sun is below the horizon); twilight and night keep the
    // palette gradient.
    let analytic_sky = (analytic && sun_altaz.altitude > 0.0).then(|| AnalyticSky {
        sun_alt: sun_altaz.altitude,
        sun_az: sun_altaz.azimuth,
        center_az,
        turbidity: turbidity_from_visibility(sample.visibility_m),
        // Ramp in over the first 8 degrees of solar elevation so the model
        // crossfades out of the palette through twilight, no seam at sunrise.
        blend: (sun_altaz.altitude / 8.0).clamp(0.0, 1.0),
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
        lightning,
        horizon_glow: build_horizon_glow(&sun_altaz, center_az, total_cover),
        analytic: analytic_sky,
        wind_speed_kmh: sample.wind_speed.unwrap_or(0.0),
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

// Interpolate along the shortest arc so 350 -> 10 crosses through 0, not back
// through 180.
fn lerp_angle_opt(a: Option<f64>, b: Option<f64>, f: f64) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => {
            let diff = ((y - x + 540.0) % 360.0) - 180.0;
            Some((x + diff * f).rem_euclid(360.0))
        }
        (some, None) | (None, some) => some,
    }
}

const SKY_W: u32 = 104;
const SKY_H: u32 = 50;

fn build_lightning(
    weather_code: Option<u32>,
    precip_mm: Option<f64>,
    lat: f64,
    lon: f64,
    unix_utc: i64,
) -> Option<Lightning> {
    let code = weather_code?;
    if !(95..=99).contains(&code) {
        return None;
    }
    let with_bolts = matches!(code, 95 | 96 | 99);
    let mm = precip_mm.unwrap_or(0.4);
    let intensity = (mm / 5.0).clamp(0.20, 0.85);
    let hour = unix_utc.div_euclid(3_600) as u64;
    let day_ordinal = unix_utc.div_euclid(86_400) as u64;
    let seed = mix_seed(&[hash_lat_lon(lat, lon), day_ordinal, hour, 0x1167_8175]) as u32;
    Some(Lightning::new(
        seed, intensity, 3_600.0, with_bolts, SKY_W, SKY_H,
    ))
}

fn parse_hour_to_unix(time_str: &str) -> Result<i64, WeatherError> {
    let naive = NaiveDateTime::parse_from_str(time_str, "%Y-%m-%dT%H:%M")
        .map_err(|e| WeatherError::Decode(format!("hour timestamp '{time_str}': {e}")))?;
    Ok(naive.and_utc().timestamp())
}

#[derive(Debug, Clone, PartialEq)]
enum SunDay {
    Times { rise_unix: i64, set_unix: i64 },
    PolarDay,
    PolarNight,
}

fn utc_date_iso(unix_utc: i64) -> String {
    Utc.timestamp_opt(unix_utc, 0)
        .single()
        .map(|dt| dt.date_naive().to_string())
        .unwrap_or_default()
}

// Open-Meteo encodes polar day/night as sentinel values, not nulls:
// polar day -> daylight_duration == 86400, sunrise YYYY-MM-DDT00:00, sunset next-day T00:00.
// polar night -> daylight_duration == 0, sunrise == sunset == YYYY-MM-DDT00:00.
// Slop guards (>= 86_399, <= 1) are insurance against future float drift, not currently needed.
fn sun_day_for(daily: &DailyArrays, date_iso: &str) -> Option<SunDay> {
    let i = daily.time.iter().position(|d| d == date_iso)?;
    let dur = daily.daylight_duration.get(i).copied()?;
    if dur >= 86_399.0 {
        return Some(SunDay::PolarDay);
    }
    if dur <= 1.0 {
        return Some(SunDay::PolarNight);
    }
    let rise = parse_hour_to_unix(daily.sunrise.get(i)?).ok()?;
    let set = parse_hour_to_unix(daily.sunset.get(i)?).ok()?;
    Some(SunDay::Times {
        rise_unix: rise,
        set_unix: set,
    })
}

fn local_hhmm(unix_utc: i64) -> String {
    Local
        .timestamp_opt(unix_utc, 0)
        .single()
        .map(|dt| dt.format("%H:%M").to_string())
        .unwrap_or_else(|| {
            Utc.timestamp_opt(unix_utc, 0)
                .unwrap()
                .format("%H:%M")
                .to_string()
        })
}

// Arrows ↑ U+2191 / ↓ U+2193 are East-Asian-Width Ambiguous: width 1 in western
// terminals (default), width 2 in CJK locales or when ambiguous-as-wide is set.
// Column math elsewhere assumes width 1; revisit if that changes.
fn format_sun_segment(sun_day: Option<&SunDay>) -> String {
    match sun_day {
        Some(SunDay::Times {
            rise_unix,
            set_unix,
        }) => format!(
            "   ↑ {}  ↓ {}",
            local_hhmm(*rise_unix),
            local_hhmm(*set_unix)
        ),
        Some(SunDay::PolarDay) => "   polar day".to_string(),
        Some(SunDay::PolarNight) => "   polar night".to_string(),
        None => String::new(),
    }
}

fn build_sun(altaz: &AltAz, center_az: f64) -> Sun {
    let (x_frac, y_frac) = astro::to_sky_fracs(altaz, center_az);
    let in_view = lateral_offset_deg(altaz.azimuth, center_az).abs() < 90.0;
    Sun {
        x_frac,
        y_frac,
        radius: 3.5,
        visible: altaz.altitude > -2.0 && in_view,
    }
}

fn build_moon(state: &astro::MoonState, center_az: f64) -> Option<Moon> {
    let (x_frac, y_frac) = astro::to_sky_fracs(&state.altaz, center_az);
    let in_view = lateral_offset_deg(state.altaz.azimuth, center_az).abs() < 90.0;
    if state.altaz.altitude <= 0.0 || !in_view {
        return None;
    }
    Some(Moon {
        x_frac,
        y_frac,
        radius: 6.5,
        phase: state.phase,
        visible: true,
    })
}

// Warm horizon light on the sun's side, strongest at sunrise/sunset and gone by
// full day or deep night. Cover damps it, since an overcast horizon has no glow.
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
    let (x_frac, _) = astro::to_sky_fracs(altaz, center_az);
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

// Total sky cover. Prefer Open-Meteo's own `cloud_cover` (computed across all
// levels); when it's missing, combine the three bands as independent occluders
// (1 - product of clear fractions) rather than averaging them, so a single
// fully-covered layer still reads as a fully-covered sky.
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
    weather_code: Option<u32>,
    lat: f64,
    lon: f64,
    day_ordinal: i64,
) -> Vec<CloudLayer> {
    let pos_hash = hash_lat_lon(lat, lon);
    let code = weather_code.unwrap_or(0);
    let mut layers = Vec::new();
    let bands = [
        (cover_high, 0.20, 4.5, 2.4, 0u32, CloudKind::Cirrus),
        (cover_mid, 0.40, 3.6, 2.4, 1, CloudKind::Altocumulus),
        (
            cover_low,
            0.60,
            3.0,
            2.2,
            2,
            low_cloud_kind(code, cover_low),
        ),
    ];
    for (cover, altitude_t, scale_x, scale_y, idx, kind) in bands {
        let seed = mix_seed(&[pos_hash, day_ordinal as u64, 0xC10D_5EED ^ idx as u64]);
        if let Some(layer) = cloud_layer(cover, altitude_t, scale_x, scale_y, seed, kind) {
            layers.push(layer);
        }
    }
    layers
}

// The low band carries the weather: showers and thunderstorms are convective
// towers (dark cumulonimbus), light/partly-cloudy skies are fair-weather
// cumulus, and anything else is a flat stratus deck.
fn low_cloud_kind(weather_code: u32, cover_low: f64) -> CloudKind {
    if (80..=82).contains(&weather_code) || (95..=99).contains(&weather_code) {
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
    // A stratus deck flattens into a solid lid as it approaches full cover, and
    // widens vertically so the overcast fills the sky rather than a thin band.
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

fn build_haze(visibility_m: Option<f64>) -> Option<Haze> {
    let viz_km = visibility_m? / 1000.0;
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

fn build_precipitation(
    weather_code: Option<u32>,
    precip_mm: Option<f64>,
    wind_dir: Option<f64>,
    lat: f64,
    lon: f64,
    day_ordinal: i64,
    center_az: f64,
) -> Option<Precipitation> {
    let mm = precip_mm.unwrap_or(0.0);
    if mm < 0.10 {
        return None;
    }
    let code = weather_code.unwrap_or(0);
    let kind = if (71..=77).contains(&code) || (85..=86).contains(&code) {
        PrecipKind::Snow
    } else {
        PrecipKind::Rain
    };
    let intensity = (mm / 5.0).clamp(0.10, 0.85);
    let dir = wind_dir.unwrap_or(180.0);
    let delta = lateral_offset_deg(dir, center_az);
    let angle_deg = (delta * 0.30).clamp(-25.0, 25.0);
    let seed = mix_seed(&[hash_lat_lon(lat, lon), day_ordinal as u64, 0xBA17_DA75]);
    Some(Precipitation {
        kind,
        intensity,
        angle_deg,
        seed,
        streak_len: 4,
        opacity: 0.40,
    })
}

fn build_chrome(
    location: &GeoResult,
    unix_utc: i64,
    now_unix: i64,
    sample: &HourSample,
    sun_day: Option<SunDay>,
) -> Chrome {
    let header_left = "celsius".to_string();
    let header_right = format!(
        "{}   {}{}",
        location.label().to_lowercase(),
        format_label(unix_utc, now_unix),
        format_sun_segment(sun_day.as_ref()),
    );

    let temp = sample
        .temperature_c
        .map(|t| format!("{:.0}°", t))
        .unwrap_or_else(|| "--°".to_string());
    let word = wmo_word(sample.weather_code.unwrap_or(0));
    let speed = sample.wind_speed.unwrap_or(0.0).round() as i64;
    let compass = compass_from_deg(sample.wind_dir.unwrap_or(0.0));
    let footer = format!("{temp}  {word}   wind {compass} {speed}");

    // ASCII one-liner for --plain: proper-case place, no degree sign, uppercase
    // compass. Grep- and pipe-friendly, distinct from the decorative footer.
    let temp_ascii = sample
        .temperature_c
        .map(|t| format!("{t:.0}C"))
        .unwrap_or_else(|| "--C".to_string());
    let status = format!(
        "{} {temp_ascii} {word} wind {} {speed}",
        location.name,
        compass.to_uppercase(),
    );

    Chrome {
        header_left,
        header_right,
        footer,
        keys: KEYS_HINT.to_string(),
        status,
    }
}

fn format_label(unix_utc: i64, now_unix: i64) -> String {
    let target = match Local.timestamp_opt(unix_utc, 0).single() {
        Some(dt) => dt,
        None => {
            return Utc
                .timestamp_opt(unix_utc, 0)
                .unwrap()
                .format("%Y-%m-%d %H:%M")
                .to_string();
        }
    };
    let now = Local.timestamp_opt(now_unix, 0).single().unwrap_or(target);
    let day_diff = (target.date_naive() - now.date_naive()).num_days();
    let hhmm = target.format("%H:%M").to_string();
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
    const DIRS: [&str; 8] = ["n", "ne", "e", "se", "s", "sw", "w", "nw"];
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
    let footer = if first_line.len() > 72 {
        format!("{}...", &first_line[..72])
    } else {
        first_line.to_string()
    };
    SkyState {
        name: "error".to_string(),
        gradient,
        sun: Sun {
            x_frac: 0.5,
            y_frac: 1.5,
            radius: 0.0,
            visible: false,
        },
        clouds: vec![],
        chrome: Chrome {
            header_left: "celsius".to_string(),
            header_right: String::new(),
            footer: footer.clone(),
            keys: "r retry   q quit".to_string(),
            status: footer,
        },
        haze: None,
        stars: None,
        moon: None,
        precipitation: None,
        lightning: None,
        horizon_glow: None,
        analytic: None,
        wind_speed_kmh: 0.0,
    }
}

/// FNV-1a over the little-endian bytes of each part. Cloud, star and
/// precipitation seeds must reproduce across toolchains; DefaultHasher's
/// algorithm is explicitly not guaranteed between Rust releases.
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
        // FNV-1a reference vectors, computed independently. If these move,
        // every daily cloud/star/precip seed moves with them.
        assert_eq!(mix_seed(&[]), 0xcbf2_9ce4_8422_2325);
        assert_eq!(mix_seed(&[0]), 0xa8c7_f832_281a_39c5);
        assert_eq!(mix_seed(&[1, 2]), 0x7717_9803_63c8_e066);
    }

    #[test]
    fn parse_hour_round_trip() {
        let unix = parse_hour_to_unix("2026-04-11T00:00").unwrap();
        // 2026-04-11T00:00:00Z, verified against `date -u -d @1775865600`
        assert_eq!(unix, 1_775_865_600);
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
        // 100% low stratus, nothing above: the union must read as fully covered,
        // not the (1.0+0+0)/3 = 0.33 the old averaging produced.
        assert!((total_cover(None, 1.0, 0.0, 0.0) - 1.0).abs() < 1e-9);
        // Two half-covered independent layers: 1 - 0.5*0.5 = 0.75.
        assert!((total_cover(None, 0.5, 0.5, 0.0) - 0.75).abs() < 1e-9);
        // A reported total wins over the bands and is rescaled from percent.
        assert!((total_cover(Some(40.0), 1.0, 1.0, 1.0) - 0.40).abs() < 1e-9);
    }

    #[test]
    fn compass_round() {
        assert_eq!(compass_from_deg(0.0), "n");
        assert_eq!(compass_from_deg(45.0), "ne");
        assert_eq!(compass_from_deg(180.0), "s");
        assert_eq!(compass_from_deg(270.0), "w");
        assert_eq!(compass_from_deg(360.0), "n");
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
        date: &str,
        sunrise: &str,
        sunset: &str,
        daylight_duration: f64,
    ) -> DailyArrays {
        DailyArrays {
            time: vec![date.to_string()],
            sunrise: vec![sunrise.to_string()],
            sunset: vec![sunset.to_string()],
            daylight_duration: vec![daylight_duration],
        }
    }

    #[test]
    fn sun_day_normal_returns_times() {
        let daily = daily_one_day(
            "2026-04-11",
            "2026-04-11T04:38",
            "2026-04-11T18:14",
            48_960.0,
        );
        match sun_day_for(&daily, "2026-04-11") {
            Some(SunDay::Times {
                rise_unix,
                set_unix,
            }) => {
                // 2026-04-11T00:00Z = 1_775_865_600 (per parse_hour_round_trip);
                // +4h38m = +16_680, +18h14m = +65_640.
                assert_eq!(rise_unix, 1_775_882_280);
                assert_eq!(set_unix, 1_775_931_240);
            }
            other => panic!("expected Times, got {other:?}"),
        }
    }

    #[test]
    fn sun_day_polar_day_from_full_daylight() {
        let daily = daily_one_day(
            "2026-05-09",
            "2026-05-09T00:00",
            "2026-05-10T00:00",
            86_400.0,
        );
        assert_eq!(sun_day_for(&daily, "2026-05-09"), Some(SunDay::PolarDay));
    }

    #[test]
    fn sun_day_polar_night_from_zero_daylight() {
        let daily = daily_one_day("2025-12-22", "2025-12-22T00:00", "2025-12-22T00:00", 0.0);
        assert_eq!(sun_day_for(&daily, "2025-12-22"), Some(SunDay::PolarNight));
    }

    #[test]
    fn sun_day_unknown_date_returns_none() {
        let daily = daily_one_day(
            "2026-04-11",
            "2026-04-11T04:38",
            "2026-04-11T18:14",
            48_960.0,
        );
        assert_eq!(sun_day_for(&daily, "2026-04-12"), None);
    }

    #[test]
    fn utc_date_iso_round_trip() {
        // 2026-04-11T00:00Z
        assert_eq!(utc_date_iso(1_775_865_600), "2026-04-11");
        // 2026-04-11T23:59Z still resolves to the same UTC date
        assert_eq!(utc_date_iso(1_775_865_600 + 86_399), "2026-04-11");
    }

    #[test]
    fn format_sun_segment_branches() {
        assert_eq!(format_sun_segment(None), "");
        assert_eq!(format_sun_segment(Some(&SunDay::PolarDay)), "   polar day");
        assert_eq!(
            format_sun_segment(Some(&SunDay::PolarNight)),
            "   polar night"
        );
    }
}
