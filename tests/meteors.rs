// Meteor overlay: schedule determinism, shower enhancement, and geometry.
// There is no celsius-lab counterpart yet, so these pin the Rust model itself
// (an MT19937-seeded schedule), the way tests/lightning.rs pins its scheduler.

use celsius::meteors::Meteors;

// UTC instants chosen so the radiant geometry is unambiguous at 50N, 0E:
// Geminids peak with the radiant high on the meridian, vs a quiet June night
// with no major shower active.
const GEMINIDS_NIGHT: i64 = 1_797_213_600; // 2026-12-14T02:00Z (day 348, peak)
const QUIET_NIGHT: i64 = 1_781_920_800; // 2026-06-20T02:00Z (no major shower)

fn build(seed: u32, unix: i64) -> Meteors {
    // 50N, 0E, south-facing, one-hour schedule at an unscaled rate, so the counts these tests compare are the model's own and not the frame's share of them.
    Meteors::new(seed, unix, 50.0, 0.0, 180.0, 3_600.0, 1.0)
}

#[test]
fn schedule_is_deterministic_for_a_seed() {
    let a = build(777, GEMINIDS_NIGHT);
    let b = build(777, GEMINIDS_NIGHT);
    assert_eq!(a.meteors.len(), b.meteors.len(), "count must be stable");
    assert_eq!(
        a.meteors[0].t_start, b.meteors[0].t_start,
        "timing bit-stable"
    );
    assert_eq!(a.meteors[0].from, b.meteors[0].from, "geometry bit-stable");
}

#[test]
fn geminids_peak_outnumbers_a_quiet_night() {
    let gem = build(2024, GEMINIDS_NIGHT);
    let quiet = build(2024, QUIET_NIGHT);
    assert!(
        quiet.meteors.len() > 3,
        "sporadic background should still fire on a quiet night, got {}",
        quiet.meteors.len()
    );
    assert!(
        gem.meteors.len() > quiet.meteors.len(),
        "Geminids peak ({}) should outnumber a quiet night ({})",
        gem.meteors.len(),
        quiet.meteors.len()
    );
}

#[test]
fn meteor_geometry_is_well_formed() {
    let m = build(99, GEMINIDS_NIGHT);
    assert!(!m.meteors.is_empty());
    for met in &m.meteors {
        let (dx, dy) = met.dir;
        let len = (dx * dx + dy * dy).sqrt();
        assert!((len - 1.0).abs() < 1e-9, "direction must be a unit vector");
        assert!(met.life > 0.0, "life must be positive");
        assert!(met.travel > 0.0, "travel must be positive");
        assert!(met.streak > 0.0, "streak must be positive");
        assert!(
            (0.0..=1.0).contains(&met.from.0) && (0.0..=1.0).contains(&met.from.1),
            "start position must be a frame fraction, got {:?}",
            met.from
        );
        assert!(
            (0.0..=1.0).contains(&met.peak_l),
            "peak_l in 0..=1, got {}",
            met.peak_l
        );
    }
}

/// The bug this geometry is stored in fractions to prevent: a schedule built once was drawn into whatever buffer the terminal happened to be, so on a wide terminal every meteor landed in the top-left corner. Rendering the same schedule at two sizes must now cover both frames alike.
#[test]
fn meteors_fill_the_frame_at_any_buffer_size() {
    use celsius::PixelBuffer;
    use celsius::colorspace::Rgb;

    let m = build(4242, GEMINIDS_NIGHT);
    let lit_extent = |w: usize, h: usize| {
        let mut pixels = PixelBuffer::filled(w, h, Rgb::new(0, 0, 0));
        for met in &m.meteors {
            celsius::meteors::overlay(&mut pixels, &m, met.t_start + met.life * 0.5);
        }
        let (mut max_x, mut max_y) = (0usize, 0usize);
        for (i, px) in pixels.pixels.iter().enumerate() {
            if px.r > 8 || px.g > 8 || px.b > 8 {
                max_x = max_x.max(i % w);
                max_y = max_y.max(i / w);
            }
        }
        (max_x as f64 / w as f64, max_y as f64 / h as f64)
    };

    let (small_x, small_y) = lit_extent(104, 50);
    let (large_x, large_y) = lit_extent(312, 150);
    assert!(
        small_x > 0.8 && small_y > 0.8,
        "meteors should reach the far edges at the reference size, got ({small_x:.2}, {small_y:.2})"
    );
    assert!(
        large_x > 0.8 && large_y > 0.8,
        "a wider buffer must not strand meteors in the corner, got ({large_x:.2}, {large_y:.2})"
    );
}

/// ZHR counts the whole hemisphere at a dark-sky limiting magnitude, and every meteor is then placed inside the frame. Without scaling the rate down to the sky actually on screen, and down again for light pollution, the visible rate is several times what an observer would see.
#[test]
fn rate_scale_thins_the_schedule() {
    let full = build(31, GEMINIDS_NIGHT);
    let framed = Meteors::new(31, GEMINIDS_NIGHT, 50.0, 0.0, 180.0, 3_600.0, 0.3);
    assert!(
        framed.meteors.len() < full.meteors.len(),
        "the frame holds a fraction of the sky, so it must hold fewer meteors: {} vs {}",
        framed.meteors.len(),
        full.meteors.len()
    );
    let city = Meteors::new(31, GEMINIDS_NIGHT, 50.0, 0.0, 180.0, 3_600.0, 0.3 * 0.084);
    assert!(
        city.meteors.len() < framed.meteors.len(),
        "an inner-city sky must show fewer still"
    );
    assert!(
        !city.meteors.is_empty(),
        "but not none: a city sky still shows the occasional meteor"
    );
}
