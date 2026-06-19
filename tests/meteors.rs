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
    // 50N, 0E, south-facing, one-hour schedule at the 104x50 reference size.
    Meteors::new(seed, unix, 50.0, 0.0, 180.0, 3_600.0, (104, 50))
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
        assert!(met.travel_px > 0.0, "travel must be positive");
        assert!(
            (0.0..=1.0).contains(&met.peak_l),
            "peak_l in 0..=1, got {}",
            met.peak_l
        );
    }
}
