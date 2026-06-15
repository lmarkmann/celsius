use celsius::weather::forecast::Forecast;
use celsius::weather::location::GeoResult;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GeoResponse {
    #[serde(default)]
    results: Vec<GeoResult>,
}

const GEOCODING_HAMBURG: &str = include_str!("open-meteo-geocoding-hamburg.json");
const FORECAST_HAMBURG: &str = include_str!("open-meteo-forecast-hamburg.json");
const FORECAST_NULLS: &str = include_str!("open-meteo-forecast-with-nulls.json");

#[test]
fn geocoding_response_parses_hamburg() {
    let parsed: GeoResponse =
        serde_json::from_str(GEOCODING_HAMBURG).expect("geocoding fixture must deserialize");
    assert!(
        !parsed.results.is_empty(),
        "fixture should contain at least one match"
    );
    let first = &parsed.results[0];
    assert_eq!(first.name, "Hamburg");
    assert_eq!(first.country.as_deref(), Some("Germany"));
    assert!((first.latitude - 53.55073).abs() < 1e-5);
    assert!((first.longitude - 9.99302).abs() < 1e-5);
    assert_eq!(first.timezone, "Europe/Berlin");
}

#[test]
fn geocoding_label_includes_country() {
    let parsed: GeoResponse = serde_json::from_str(GEOCODING_HAMBURG).unwrap();
    let label = parsed.results[0].label();
    assert!(label.contains("Hamburg"));
    assert!(label.contains("Germany"));
}

#[test]
fn geocoding_empty_results_deserializes_to_empty_vec() {
    let empty = r#"{"generationtime_ms": 0.5}"#;
    let parsed: GeoResponse = serde_json::from_str(empty).unwrap();
    assert!(parsed.results.is_empty());
}

#[test]
fn forecast_response_parses_hamburg() {
    let parsed: Forecast =
        serde_json::from_str(FORECAST_HAMBURG).expect("forecast fixture must deserialize");
    assert!((parsed.latitude - 53.56).abs() < 0.1);
    assert!((parsed.longitude - 10.0).abs() < 0.1);
    assert_eq!(parsed.timezone, "GMT");
    assert_eq!(parsed.hourly.len(), 6);
    assert_eq!(parsed.hourly.time[0], "2026-04-11T00:00");
    assert_eq!(parsed.hourly.temperature_2m[0], Some(4.8));
    assert_eq!(parsed.hourly.weather_code[0], Some(0));
    let daily = parsed.daily.expect("daily block present");
    assert_eq!(daily.temperature_2m_max[0], Some(14.2));
    assert_eq!(daily.temperature_2m_min[0], Some(5.8));
}

#[test]
fn forecast_equal_length_across_variables() {
    let parsed: Forecast = serde_json::from_str(FORECAST_HAMBURG).unwrap();
    let n = parsed.hourly.len();
    let h = &parsed.hourly;
    assert_eq!(h.temperature_2m.len(), n);
    assert_eq!(h.cloud_cover_low.len(), n);
    assert_eq!(h.cloud_cover_mid.len(), n);
    assert_eq!(h.cloud_cover_high.len(), n);
    assert_eq!(h.precipitation.len(), n);
    assert_eq!(h.wind_speed_10m.len(), n);
    assert_eq!(h.wind_direction_10m.len(), n);
    assert_eq!(h.visibility.len(), n);
    assert_eq!(h.weather_code.len(), n);
}

#[test]
fn forecast_nulls_become_option_none() {
    let parsed: Forecast =
        serde_json::from_str(FORECAST_NULLS).expect("nulls fixture must deserialize");
    assert_eq!(parsed.hourly.temperature_2m[0], Some(4.8));
    assert_eq!(parsed.hourly.temperature_2m[1], None);
    assert_eq!(parsed.hourly.temperature_2m[2], Some(4.2));
    assert_eq!(parsed.hourly.cloud_cover_low[2], None);
}

#[test]
fn compose_at_interpolates_between_hours() {
    let forecast: Forecast = serde_json::from_str(FORECAST_HAMBURG).unwrap();
    let geo = GeoResult {
        name: "Hamburg".to_string(),
        latitude: 53.55,
        longitude: 9.99,
        timezone: "UTC".to_string(),
        country: None,
        admin1: None,
        elevation: None,
        population: None,
    };
    // Halfway between 01:00 and 02:00 UTC. wind_speed_10m is 4.0 then 4.1, so the
    // interpolated sky must read 4.05, proving it didn't snap to the top of hour.
    let t01 = 1_775_869_200; // 2026-04-11T01:00Z
    let mid = t01 + 1_800; // 01:30Z
    let opts = celsius::weather::ComposeOpts {
        center_az: 180.0,
        bortle: None,
        analytic: false,
    };
    let sky = celsius::weather::compose_at(&forecast, &geo, mid, t01, opts)
        .expect("compose_at on fixture");
    assert!(
        (sky.wind_speed_kmh - 4.05).abs() < 1e-6,
        "expected interpolated wind 4.05, got {}",
        sky.wind_speed_kmh
    );
    // The fixture's daily high/low (14.2 / 5.8) reach the richest footer tier.
    assert!(
        sky.chrome.footer_tiers[0].contains("H14 L6"),
        "footer should carry the day's H/L, got {:?}",
        sky.chrome.footer_tiers[0]
    );
}

// Live network smoke tests. Opt-in via `cargo test -- --ignored` so they
// only run when a developer wants to verify the real Open-Meteo response
// still matches our types. Never in CI.

#[test]
#[ignore]
fn live_geocoding_returns_hamburg() {
    let results = celsius::weather::location::geocode("Hamburg").expect("live request");
    assert!(
        !results.is_empty(),
        "Hamburg should resolve to at least one match"
    );
    let top = &results[0];
    assert_eq!(top.name, "Hamburg");
    assert_eq!(top.country.as_deref(), Some("Germany"));
}

#[test]
#[ignore]
fn live_forecast_returns_168_hours() {
    let forecast = celsius::weather::forecast::fetch(53.5511, 9.9937).expect("live request");
    assert_eq!(forecast.timezone, "GMT");
    assert_eq!(
        forecast.hourly.len(),
        168,
        "7 days x 24 hours = 168 hourly samples"
    );
    assert_eq!(forecast.hourly.temperature_2m.len(), forecast.hourly.len());
}
