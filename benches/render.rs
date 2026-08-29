use std::path::PathBuf;
use std::time::Duration;

use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use celsius::analytic_sky::{self, AnalyticSky};
use celsius::astro;
use celsius::atmosphere::Atmosphere;
use celsius::colorspace::{Oklab, oklab_to_rgb, rgb_u8_to_oklab};
use celsius::lightning::{self, Lightning};
use celsius::noise::Noise;
use celsius::precipitation;
use celsius::snow::{self, FlakeForm, Snowfall};
use celsius::tui::{App, Timeline, draw_frame, write_frame};
use celsius::weather::forecast::Forecast;
use celsius::weather::location::GeoResult;
use celsius::weather::{ComposeOpts, compose, compose_at};
use celsius::{SkyState, load_scene, render, render_supersampled};

/// One TUI tick, matching `TICK` in the event loop.
const TICK: Duration = Duration::from_millis(33);

/// Terminal geometry whose sky area comes out at exactly 104x50 pixels: the layout takes one row for the header and one for the footer, and each sky row is two pixels. Keeps the frame numbers comparable with the `104x50_*` renders.
const FRAME_AREA: Rect = Rect {
    x: 0,
    y: 0,
    width: 104,
    height: 27,
};

fn scene_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scenes")
        .join(format!("{name}.toml"))
}

fn bench_render(c: &mut Criterion) {
    let state = load_scene(scene_path("stormy_afternoon_advancing")).unwrap();

    let mut g = c.benchmark_group("render");
    g.sample_size(20);

    // Lab authoring size; what the oracle test uses
    g.bench_function("104x50_stormy", |b| {
        b.iter(|| render(black_box(&state), 104, 50))
    });

    // Typical wide terminal
    g.bench_function("200x50_stormy", |b| {
        b.iter(|| render(black_box(&state), 200, 50))
    });

    // Stress: large viewport
    g.bench_function("320x100_stormy", |b| {
        b.iter(|| render(black_box(&state), 320, 100))
    });

    // The supersampled tiers. `raster::sample_factor` returns 2 for `--aa` on half blocks or for plain `--glyphs quad`, and 4 for both together, so factor 4 draws sixteen times the pixels of the default and is the heaviest frame the crate can be asked for. Until now the only measurement either had was a number typed by hand into a commit message.
    g.bench_function("104x50_stormy_quad", |b| {
        b.iter(|| render_supersampled(black_box(&state), 104, 50, 2))
    });

    g.bench_function("104x50_stormy_aa", |b| {
        b.iter(|| render_supersampled(black_box(&state), 104, 50, 4))
    });

    // Clear sky (no clouds, no precipitation) for comparison
    let clear = load_scene(scene_path("high_noon_clear")).unwrap();
    g.bench_function("104x50_clear", |b| {
        b.iter(|| render(black_box(&clear), 104, 50))
    });

    // Starfield: exercises the star lookup on every pixel
    let night = load_scene(scene_path("moonless_darksky")).unwrap();
    g.bench_function("104x50_night", |b| {
        b.iter(|| render(black_box(&night), 104, 50))
    });

    // Moon disc + glow paths
    let moonlit = load_scene(scene_path("moonlit_clear_winter")).unwrap();
    g.bench_function("104x50_moonlit", |b| {
        b.iter(|| render(black_box(&moonlit), 104, 50))
    });

    // Preetham background. Scenes never set `analytic` (only the live-weather daytime path does), so this is the only way the model that paints every sun-up sky gets measured. Same scene as 104x50_clear, so the delta is the per-pixel Perez cost and nothing else; the disc in the TOML does not sit at `sun_az`, which costs the same either way.
    g.bench_function("104x50_clear_analytic", |b| {
        let sky = with_analytic(&clear, 1.0);
        b.iter(|| render(black_box(&sky), 104, 50))
    });

    // Twilight crossfade: pays for the Perez sample *and* the gradient lerp. This is the sunrise and sunset sky, the one people watch.
    g.bench_function("104x50_clear_crossfade", |b| {
        let sky = with_analytic(&clear, 0.5);
        b.iter(|| render(black_box(&sky), 104, 50))
    });

    // The overcast daytime sky. `build_sky` attaches a Perez sky whenever the sun is up, then weights it by `(1 - total_cover)`, so full cover asks for one and discards it whole. This should now cost what `104x50_clear` costs, and it is the only thing that would notice if the per-pixel sample came back.
    g.bench_function("104x50_clear_overcast", |b| {
        let sky = with_analytic(&clear, 0.0);
        b.iter(|| render(black_box(&sky), 104, 50))
    });

    g.finish();
}

/// `base` with a midday analytic sky attached at crossfade weight `blend`. Turbidity 2.0 is what `Atmosphere::from_visibility` yields at full visibility.
fn with_analytic(base: &SkyState, blend: f64) -> SkyState {
    let mut sky = base.clone();
    sky.analytic = Some(AnalyticSky {
        sun_alt: 55.0,
        sun_az: 180.0,
        center_az: 180.0,
        atmosphere: Atmosphere::from_turbidity(2.0),
        blend,
    });
    sky
}

/// What one frame costs. `render()` only produces a pixel buffer; these cover turning it into terminal cells, which is the rest of the 33ms budget.
fn bench_frame(c: &mut Criterion) {
    let mut g = c.benchmark_group("frame");
    g.sample_size(20);

    // Scene files carry no wind, but the drift that dirties the sky cache does, so take the speed the scene's own chrome advertises.
    let mut stormy = load_scene(scene_path("stormy_afternoon_advancing")).unwrap();
    stormy.wind_speed_kmh = 28.0;

    let mut buf = Buffer::empty(FRAME_AREA);

    // Idle repaint: the cache is warm and nothing invalidated it, so this is chrome layout plus 5200 cell writes. The number draw-gating protects.
    g.bench_function("idle_cache_hit", |b| {
        let timeline = Timeline::single(stormy.clone());
        let mut app = App::new(&timeline);
        draw_frame(&mut buf, FRAME_AREA, &mut app);
        b.iter(|| draw_frame(&mut buf, FRAME_AREA, &mut app));
    });

    // Drifting sky: tick invalidates the cache, so every frame re-renders. Mirrors the event loop, which ticks and then draws.
    g.bench_function("dirty_rerender", |b| {
        let timeline = Timeline::single(stormy.clone());
        let mut app = App::new(&timeline);
        b.iter(|| {
            app.tick(TICK);
            draw_frame(&mut buf, FRAME_AREA, &mut app);
        });
    });

    // A second of a sky nobody is touching: thirty ticks, each followed by a draw. Every one of those ticks re-renders the whole buffer, though at 28 km/h with `scale_x = 3` the clouds advance about a three-hundredth of a pixel per tick, so a few hundred consecutive frames are the same picture. `dirty_rerender` measures a single tick and cannot see that; this measures the cadence, which is where a change to when drift invalidates the cache would show up. `scrub_hour` is what keeps guarding the render itself, since a scrub always dirties.
    const TICKS_PER_SECOND: u32 = 30;
    g.bench_function("drift_cadence", |b| {
        let timeline = Timeline::single(stormy.clone());
        let mut app = App::new(&timeline);
        b.iter(|| {
            for _ in 0..TICKS_PER_SECOND {
                app.tick(TICK);
                draw_frame(&mut buf, FRAME_AREA, &mut app);
            }
        });
    });

    // Thunderstorm: the cached base survives, but every frame clones it and runs the flash overlay on the copy.
    g.bench_function("lightning_frame", |b| {
        let mut lit = stormy.clone();
        lit.lightning = Some(Lightning::new(101, 0.5, 1.0, false));
        let timeline = Timeline::single(lit);
        let mut app = App::new(&timeline);
        b.iter(|| {
            app.tick(TICK);
            draw_frame(&mut buf, FRAME_AREA, &mut app);
        });
    });

    // Scrubbing an hour, which swaps the displayed sky and invalidates the cache. Alternate the direction: scrubbing one way runs off the end of the timeline, after which the index stops moving and this measures cache hits.
    g.bench_function("scrub_hour", |b| {
        let timeline = Timeline::new(vec![stormy.clone(); 24], 0, None, 0);
        let mut app = App::new(&timeline);
        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        let left = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        let mut forward = true;
        b.iter(|| {
            app.handle_key(if forward { right } else { left });
            forward = !forward;
            draw_frame(&mut buf, FRAME_AREA, &mut app);
        });
    });

    // The `--frame` pipe surface, not the TUI: one render plus 5200 formatted ANSI escapes. Clear sky keeps the render share small, so subtracting 104x50_clear leaves the serialization cost.
    let clear = load_scene(scene_path("high_noon_clear")).unwrap();
    let mut sink = Vec::with_capacity(128 * 1024);
    g.bench_function("write_frame_104x50", |b| {
        b.iter(|| {
            sink.clear();
            write_frame(black_box(&clear), &mut sink).unwrap();
        })
    });

    g.finish();
}

/// The two full-buffer passes layered on top of `render()`. Lightning never runs inside `render()` at all; precipitation is only reached via stormy scenes.
fn bench_overlay(c: &mut Criterion) {
    let mut g = c.benchmark_group("overlay");
    g.sample_size(20);

    let clear = load_scene(scene_path("high_noon_clear")).unwrap();
    let base = render(&clear, 104, 50);

    let sheet = Lightning::new(101, 0.5, 1.0, false);
    let bolts = Lightning::new(101, 0.5, 1.0, true);

    // `l_bump_at` returns zero for most of the clock, so a guessed constant would silently benchmark the early return. Pin `t` to a real flash peak and to a point past the last one, and assert both land where intended.
    let t_flash = sheet.strikes[0].sub_flashes[0].t_peak;
    let t_quiet = sheet
        .strikes
        .last()
        .and_then(|s| s.sub_flashes.last())
        .map(|sf| sf.t_peak + 1.0)
        .expect("seed 101 schedules at least one strike");
    assert!(
        lightning::l_bump_at(&sheet.strikes, t_flash, &sheet.params) > 0.0001,
        "t_flash must land inside a flash"
    );
    assert!(
        lightning::l_bump_at(&sheet.strikes, t_quiet, &sheet.params) <= 0.0001,
        "t_quiet must land between flashes"
    );

    // What most thunderstorm ticks cost: scan the strikes, find nothing, return.
    g.bench_function("lightning_quiet", |b| {
        b.iter_batched_ref(
            || base.clone(),
            |px| lightning::overlay(px, &sheet, t_quiet),
            BatchSize::SmallInput,
        )
    });

    // Sheet flash: a full-buffer Oklab round trip through two powf per channel.
    g.bench_function("lightning_flash", |b| {
        b.iter_batched_ref(
            || base.clone(),
            |px| lightning::overlay(px, &sheet, t_flash),
            BatchSize::SmallInput,
        )
    });

    // Adds the pre-bolt buffer snapshot and the recursive bolt draw. The delta from lightning_flash is the per-frame clone.
    g.bench_function("lightning_bolt", |b| {
        b.iter_batched_ref(
            || base.clone(),
            |px| lightning::overlay(px, &bolts, t_flash),
            BatchSize::SmallInput,
        )
    });

    // A dendrite is the most expensive form (centre plus four arms) at a heavy-snow count. Unlike the rain pair below, the count is an absolute, so the two sizes measure the per-flake draw against the buffer rather than against a density that grows with it.
    let heavy = Snowfall {
        form: FlakeForm::Dendrite,
        count: 320,
        seed: 2749,
        drift: 0.02,
        opacity: 0.75,
    };
    g.bench_function("snow_104x50", |b| {
        b.iter_batched_ref(
            || base.clone(),
            |px| snow::overlay(px, &heavy, 3.0),
            BatchSize::SmallInput,
        )
    });

    let stormy = load_scene(scene_path("stormy_afternoon_advancing")).unwrap();
    let precip = stormy
        .precipitation
        .clone()
        .expect("stormy scene carries precipitation");

    // Streak count scales with width * height * intensity, so the pair shows it.
    g.bench_function("precip_104x50", |b| {
        b.iter_batched_ref(
            || base.clone(),
            |px| precipitation::overlay(px, &precip),
            BatchSize::SmallInput,
        )
    });

    let wide = render(&clear, 320, 100);
    g.bench_function("snow_320x100", |b| {
        b.iter_batched_ref(
            || wide.clone(),
            |px| snow::overlay(px, &heavy, 3.0),
            BatchSize::SmallInput,
        )
    });

    g.bench_function("precip_320x100", |b| {
        b.iter_batched_ref(
            || wide.clone(),
            |px| precipitation::overlay(px, &precip),
            BatchSize::SmallInput,
        )
    });

    g.finish();
}

const FORECAST_HAMBURG: &str = include_str!("../tests/open-meteo-forecast-hamburg.json");

/// Hours in the forecast the TUI actually builds a timeline from.
const WEEK_HOURS: usize = 168;

fn hamburg() -> GeoResult {
    GeoResult {
        name: "Hamburg".to_string(),
        latitude: 53.55,
        longitude: 9.99,
        timezone: "UTC".to_string(),
        country: None,
        admin1: None,
        elevation: None,
        population: None,
    }
}

/// The 6-hour fixture stretched to a full week, cycling its values across dates 2026-04-11..17. `bracket_hours` chrono-parses one timestamp per element until it passes the target, so scan cost only shows up at a realistic array length.
fn week_long(base: &Forecast) -> Forecast {
    let mut forecast = base.clone();
    let src = &base.hourly;
    let n = src.time.len();
    let cycle = |v: &[Option<f64>]| -> Vec<Option<f64>> {
        if v.is_empty() {
            return Vec::new();
        }
        (0..WEEK_HOURS).map(|i| v[i % n]).collect()
    };

    let hourly = &mut forecast.hourly;
    hourly.time = (0..WEEK_HOURS)
        .map(|i| format!("2026-04-{:02}T{:02}:00", 11 + i / 24, i % 24))
        .collect();
    hourly.temperature_2m = cycle(&src.temperature_2m);
    hourly.cloud_cover = cycle(&src.cloud_cover);
    hourly.cloud_cover_low = cycle(&src.cloud_cover_low);
    hourly.cloud_cover_mid = cycle(&src.cloud_cover_mid);
    hourly.cloud_cover_high = cycle(&src.cloud_cover_high);
    hourly.precipitation = cycle(&src.precipitation);
    hourly.wind_speed_10m = cycle(&src.wind_speed_10m);
    hourly.wind_direction_10m = cycle(&src.wind_direction_10m);
    hourly.visibility = cycle(&src.visibility);
    hourly.weather_code = (0..WEEK_HOURS).map(|i| src.weather_code[i % n]).collect();

    // Stretch the daily arrays too, so a late target still finds its sunrise entry and the near/far difference stays purely the scan depth.
    if let Some(daily) = &mut forecast.daily {
        let days = WEEK_HOURS / 24;
        let src_daily = base.daily.as_ref().expect("cloned from Some");
        let m = src_daily.time.len();
        daily.time = (0..days)
            .map(|d| format!("2026-04-{:02}", 11 + d))
            .collect();
        daily.sunrise = (0..days)
            .map(|d| format!("2026-04-{:02}T04:38", 11 + d))
            .collect();
        daily.sunset = (0..days)
            .map(|d| format!("2026-04-{:02}T18:14", 11 + d))
            .collect();
        daily.daylight_duration = (0..days)
            .map(|d| src_daily.daylight_duration[d % m])
            .collect();
        daily.temperature_2m_max = (0..days)
            .map(|d| src_daily.temperature_2m_max[d % m])
            .collect();
        daily.temperature_2m_min = (0..days)
            .map(|d| src_daily.temperature_2m_min[d % m])
            .collect();
    }
    forecast
}

/// Forecast to SkyState. Startup runs this once per hour of the timeline, so multiply by 168 for the cost of building a week.
fn bench_compose(c: &mut Criterion) {
    let fixture: Forecast = serde_json::from_str(FORECAST_HAMBURG).unwrap();
    let forecast = week_long(&fixture);
    let geo = hamburg();
    let opts = ComposeOpts::default();

    // 2026-04-11T00:00Z, the fixture's first hour. Sunrise is 04:38, so hour 0 is night (star field, no analytic sky) and hour 5 is just past sunrise.
    let t00 = 1_775_865_600;

    let mut g = c.benchmark_group("compose");

    g.bench_function("night", |b| {
        b.iter(|| compose(black_box(&forecast), &geo, 0, t00, opts).unwrap())
    });

    g.bench_function("day", |b| {
        b.iter(|| compose(black_box(&forecast), &geo, 5, t00, opts).unwrap())
    });

    // Scrubbing near the start of the week: the bracket scan breaks early.
    g.bench_function("at_near", |b| {
        let target = t00 + 3 * 3_600;
        b.iter(|| compose_at(black_box(&forecast), &geo, target, t00, opts).unwrap())
    });

    // Scrubbing to the last day: the scan walks the whole array, chrono-parsing every timestamp on the way. The delta from at_near is that scan.
    g.bench_function("at_far", |b| {
        let target = t00 + (WEEK_HOURS as i64 - 2) * 3_600;
        b.iter(|| compose_at(black_box(&forecast), &geo, target, t00, opts).unwrap())
    });

    // Everything startup computes before the first sky appears: `build_timeline` composes every hour of the week eagerly. Worth stating plainly, because no benchmark can show it: real startup is dominated by two sequential Open-Meteo round trips, so a fast number here does not mean a fast start. What this pins is that the CPU share stays negligible against them.
    g.bench_function("timeline_168h", |b| {
        b.iter(|| {
            for h in 0..WEEK_HOURS {
                black_box(compose(black_box(&forecast), &geo, h, t00, opts).unwrap());
            }
        })
    });

    g.finish();
}

fn bench_noise(c: &mut Criterion) {
    let noise = Noise::new(0xC0FFEE);
    let mut g = c.benchmark_group("noise");

    g.bench_function("new_96x32", |b| {
        b.iter(|| Noise::new(black_box(0xC0FFEE_u64)))
    });

    // A frame's worth, at the coordinates a 104x50 cloud layer actually walks (`nx = fx * scale_x + offset_x`, with the scene defaults). A lone call is far too short to survive the single-iteration harness the CodSpeed runner uses: it reported 2.6 us under simulation against 35 ns locally, a ratio the render benches hold between seven and thirteen, so what it measured was the harness rather than the fbm chain. Renamed rather than fixed in place, because the old series is not comparable to this one.
    let coords: Vec<(f64, f64)> = (0..50)
        .flat_map(|py| {
            (0..104).map(move |px| {
                (
                    f64::from(px) / 104.0 * 3.0 + 0.4,
                    f64::from(py) / 50.0 * 2.2 + 1.3,
                )
            })
        })
        .collect();

    g.bench_function("warped_fbm_5200", |b| {
        b.iter(|| {
            for &(x, y) in &coords {
                black_box(noise.warped_fbm(x, y));
            }
        })
    });

    g.finish();
}

/// Per-pixel and per-sky primitives. The colorspace pair runs a whole frame's worth per iteration: a single conversion is short enough that the number would be criterion's loop overhead rather than the powf chain.
fn bench_micro(c: &mut Criterion) {
    let mut g = c.benchmark_group("micro");

    let prepared = analytic_sky::prepare(&AnalyticSky {
        sun_alt: 55.0,
        sun_az: 180.0,
        center_az: 180.0,
        atmosphere: Atmosphere::from_turbidity(2.0),
        blend: 1.0,
    });
    // A frame's worth of Perez evaluations, which is what a daytime render pays: `sample` runs once per pixel and roughly triples a clear 104x50 frame. Batched for the same reason as `warped_fbm_5200`, a single call being short enough that the reported number was the harness.
    let fracs: Vec<(f64, f64)> = (0..50)
        .flat_map(|py| (0..104).map(move |px| (f64::from(px) / 103.0, f64::from(py) / 49.0)))
        .collect();

    g.bench_function("analytic_sample_5200", |b| {
        b.iter(|| {
            for &(x, y) in &fracs {
                black_box(prepared.sample(x, y));
            }
        })
    });

    // Both run once per compose, and startup composes every hour of the week, so a week is the honest batch rather than an arbitrary multiple. Meeus' lunar series is the heavier of the two.
    let t = 1_775_865_600;
    let week: Vec<i64> = (0..WEEK_HOURS as i64).map(|h| t + h * 3_600).collect();

    g.bench_function("sun_position_168", |b| {
        b.iter(|| {
            for &at in &week {
                black_box(astro::sun_position(black_box(53.55), black_box(9.99), at));
            }
        })
    });

    g.bench_function("moon_state_168", |b| {
        b.iter(|| {
            for &at in &week {
                black_box(astro::moon_state(black_box(53.55), black_box(9.99), at));
            }
        })
    });

    // Real pixels rather than synthetic ones, so the value distribution matches what the write path actually sees.
    let clear = load_scene(scene_path("high_noon_clear")).unwrap();
    let frame = render(&clear, 104, 50);
    let labs: Vec<Oklab> = frame
        .pixels
        .iter()
        .map(|p| rgb_u8_to_oklab(p.r, p.g, p.b))
        .collect();

    g.bench_function("oklab_to_rgb_5200", |b| {
        b.iter(|| {
            for lab in &labs {
                black_box(oklab_to_rgb(*lab));
            }
        })
    });

    g.bench_function("rgb_u8_to_oklab_5200", |b| {
        b.iter(|| {
            for p in &frame.pixels {
                black_box(rgb_u8_to_oklab(p.r, p.g, p.b));
            }
        })
    });

    // `alloc_floor_104x50` used to sit here, asking what share of a 104x50 render is the allocator and therefore whether a `render_into` writing through a reused buffer would buy anything. It answered: 18.6 us against a 5.9 ms stormy render, with the whole render allocating 15.4 KB, so 0.3% and no. Retired rather than kept, because a settled question on the dashboard reads like an open one.

    // The star field is rebuilt inside every render, and placing stars on the sky rather than on the screen made each candidate cost a projection and a rejection draw. This is what says whether that rebuild is worth caching or is already noise against the pixel loop.
    let dark = load_scene(scene_path("moonless_darksky")).unwrap();
    let stars = dark.stars.clone().unwrap();
    g.bench_function("star_field_460", |b| {
        b.iter(|| {
            celsius::stars::build_star_field(
                black_box(&stars),
                black_box(104),
                black_box(50),
                &dark.gradient,
            )
        })
    });

    g.finish();
}

criterion_group!(
    benches,
    bench_render,
    bench_frame,
    bench_overlay,
    bench_compose,
    bench_noise,
    bench_micro
);
criterion_main!(benches);
