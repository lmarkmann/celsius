use std::path::PathBuf;

use criterion::{Criterion, black_box, criterion_group, criterion_main};

use celsius::noise::Noise;
use celsius::{load_scene, render};

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

    g.finish();
}

fn bench_noise(c: &mut Criterion) {
    let noise = Noise::new(0xC0FFEE);
    let mut g = c.benchmark_group("noise");

    g.bench_function("new_96x32", |b| {
        b.iter(|| Noise::new(black_box(0xC0FFEE_u64)))
    });

    g.bench_function("warped_fbm", |b| {
        b.iter(|| noise.warped_fbm(black_box(1.23), black_box(4.56)))
    });

    g.finish();
}

criterion_group!(benches, bench_render, bench_noise);
criterion_main!(benches);
