//! Compose a SkyState from a forecast hour, a location, and a wall clock.
//!
//! This is the heart of the live weather pipeline. Everything upstream is
//! Open-Meteo JSON; everything downstream is the lab's renderer. The job
//! here is to make those two halves agree without tuning anything visual --
//! the gradient palettes are lifted verbatim, the cloud thresholds and
//! cover values are derived from the lab scenes, and the per-day cloud
//! seeding is fixed by `(lat, lon, utc_date, layer)` so the same hour
//! re-rendered tomorrow is byte-identical.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use chrono::{Datelike, Local, NaiveDateTime, TimeZone, Utc};

use crate::astro::{self, AltAz};
use crate::scene::{Chrome, CloudLayer, Haze, Moon, Precipitation, SkyState, Stars, Sun};

use super::WeatherError;
use super::forecast::Forecast;
use super::gradients::{gradient_for, select_palette};
use super::location::GeoResult;

const KEYS_HINT: &str = "<- -> scrub   tab day   t now   l location   ? help   q quit";

/// Build a `SkyState` for a single forecast hour.
///
/// `now_unix` is the wall clock at launch and only feeds the chrome label
/// formatter ("today" vs "yesterday" vs absolute date). The render itself
/// uses the parsed UTC timestamp of the selected hour for sun and moon
/// position so the sky is internally consistent regardless of when you
/// scrubbed there.
///
/// `center_az` is the compass bearing the viewer faces (0 = N, 90 = E,
/// 180 = S, 270 = W). Default 180 for northern-hemisphere observers.
pub fn compose(
    forecast: &Forecast,
    location: &GeoResult,
    hour_index: usize,
    now_unix: i64,
    center_az: f64,
) -> Result<SkyState, WeatherError> {
    let h = hour_index.min(forecast.hourly.len().saturating_sub(1));
    let unix_utc = parse_hour_to_unix(&forecast.hourly.time[h])?;

    let lat = location.latitude;
    let lon = location.longitude;
    let sun_altaz = astro::sun_position(lat, lon, unix_utc);
    let moon_state = astro::moon_state(lat, lon, unix_utc);

    let cover_low = forecast.hourly.cloud_cover_low[h].unwrap_or(0.0) / 100.0;
    let cover_mid = forecast.hourly.cloud_cover_mid[h].unwrap_or(0.0) / 100.0;
    let cover_high = forecast.hourly.cloud_cover_high[h].unwrap_or(0.0) / 100.0;
    let total_cover = ((cover_low + cover_mid + cover_high) / 3.0).clamp(0.0, 1.0);

    let palette = select_palette(sun_altaz.altitude, total_cover);
    let gradient = gradient_for(palette);

    let day_ordinal = unix_utc.div_euclid(86_400);
    let sun = build_sun(&sun_altaz, center_az);
    let moon = build_moon(&moon_state, center_az);
    let stars = build_stars(sun_altaz.altitude, lat, lon, day_ordinal);
    let clouds = build_clouds(cover_low, cover_mid, cover_high, lat, lon, day_ordinal);
    let haze = build_haze(forecast.hourly.visibility[h]);
    let precipitation = build_precipitation(
        forecast.hourly.weather_code[h],
        forecast.hourly.precipitation[h],
        forecast.hourly.wind_direction_10m[h],
        lat,
        lon,
        day_ordinal,
        center_az,
    );

    let chrome = build_chrome(
        location,
        unix_utc,
        now_unix,
        forecast.hourly.temperature_2m[h],
        forecast.hourly.weather_code[h],
        forecast.hourly.wind_speed_10m[h],
        forecast.hourly.wind_direction_10m[h],
    );

    Ok(SkyState {
        name: format!("{}-{}", location.name.to_lowercase(), h),
        gradient,
        sun,
        clouds,
        chrome,
        haze,
        stars,
        moon,
        precipitation,
        wind_speed_kmh: forecast.hourly.wind_speed_10m[h].unwrap_or(0.0),
    })
}

fn parse_hour_to_unix(time_str: &str) -> Result<i64, WeatherError> {
    let naive = NaiveDateTime::parse_from_str(time_str, "%Y-%m-%dT%H:%M")
        .map_err(|e| WeatherError::Decode(format!("hour timestamp '{time_str}': {e}")))?;
    Ok(naive.and_utc().timestamp())
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

fn lateral_offset_deg(azimuth: f64, center_az: f64) -> f64 {
    ((azimuth - center_az + 540.0) % 360.0) - 180.0
}

fn build_stars(sun_alt: f64, lat: f64, lon: f64, day_ordinal: i64) -> Option<Stars> {
    if sun_alt >= -3.0 {
        return None;
    }
    let darkness = ((-sun_alt - 3.0) / 15.0).clamp(0.0, 1.0);
    let brightness = 0.55 + 0.45 * darkness;
    let count = (180.0 + 200.0 * darkness) as u32;
    let sky_threshold = 0.30 + 0.08 * darkness;
    Some(Stars {
        count,
        seed: mix_seed(&[hash_lat_lon(lat, lon), day_ordinal as u64, 0x57A4_5EED]),
        brightness,
        sky_threshold,
    })
}

fn build_clouds(
    cover_low: f64,
    cover_mid: f64,
    cover_high: f64,
    lat: f64,
    lon: f64,
    day_ordinal: i64,
) -> Vec<CloudLayer> {
    let pos_hash = hash_lat_lon(lat, lon);
    let mut layers = Vec::new();
    let bands = [
        (cover_high, 0.20, 4.5, 2.4, 0u32),
        (cover_mid, 0.40, 3.6, 2.4, 1),
        (cover_low, 0.60, 3.0, 2.2, 2),
    ];
    for (cover, altitude_t, scale_x, scale_y, idx) in bands {
        let seed = mix_seed(&[pos_hash, day_ordinal as u64, 0xC10D_5EED ^ idx as u64]);
        if let Some(layer) = cloud_layer(cover, altitude_t, scale_x, scale_y, seed) {
            layers.push(layer);
        }
    }
    layers
}

fn cloud_layer(
    cover: f64,
    altitude_t: f64,
    scale_x: f64,
    scale_y: f64,
    seed: u64,
) -> Option<CloudLayer> {
    if cover < 0.05 {
        return None;
    }
    let threshold = 0.55 - 0.40 * cover;
    let cover_strength = 0.90 + 1.00 * (1.0 - cover);
    Some(CloudLayer {
        cover: cover_strength,
        altitude_t,
        altitude_sigma: 0.10,
        scale_x,
        scale_y,
        threshold,
        seed,
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
        "snow"
    } else {
        "rain"
    };
    let intensity = (mm / 5.0).clamp(0.10, 0.85);
    let dir = wind_dir.unwrap_or(180.0);
    let delta = lateral_offset_deg(dir, center_az);
    let angle_deg = (delta * 0.30).clamp(-25.0, 25.0);
    let seed = mix_seed(&[hash_lat_lon(lat, lon), day_ordinal as u64, 0xBA17_DA75]);
    Some(Precipitation {
        kind: kind.to_string(),
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
    temperature_c: Option<f64>,
    weather_code: Option<u32>,
    wind_speed: Option<f64>,
    wind_dir: Option<f64>,
) -> Chrome {
    let header_left = "celsius".to_string();
    let header_right = format!(
        "{}   {}",
        location.label().to_lowercase(),
        format_label(unix_utc, now_unix)
    );

    let temp = temperature_c
        .map(|t| format!("{:.0}°", t))
        .unwrap_or_else(|| "--°".to_string());
    let word = wmo_word(weather_code.unwrap_or(0));
    let speed = wind_speed.unwrap_or(0.0).round() as i64;
    let compass = compass_from_deg(wind_dir.unwrap_or(0.0));
    let footer = format!("{temp}  {word}   wind {compass} {speed}");

    Chrome {
        header_left,
        header_right,
        footer,
        keys: KEYS_HINT.to_string(),
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
    let mut hasher = DefaultHasher::new();
    lat_bits.hash(&mut hasher);
    lon_bits.hash(&mut hasher);
    hasher.finish()
}

fn mix_seed(parts: &[u64]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for p in parts {
        p.hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::super::gradients::Palette;
    use super::*;

    #[test]
    fn parse_hour_round_trip() {
        let unix = parse_hour_to_unix("2026-04-11T00:00").unwrap();
        // 2026-04-11T00:00:00Z, verified against `date -u -d @1775865600`
        assert_eq!(unix, 1_775_865_600);
    }

    #[test]
    fn select_palette_buckets() {
        assert_eq!(select_palette(70.0, 0.1), Palette::Day);
        assert_eq!(select_palette(5.0, 0.1), Palette::GoldenHour);
        assert_eq!(select_palette(-6.0, 0.0), Palette::BlueHour);
        assert_eq!(select_palette(-20.0, 0.0), Palette::Night);
        assert_eq!(select_palette(20.0, 0.95), Palette::Overcast);
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
}
