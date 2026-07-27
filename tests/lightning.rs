// Fixture captured with seed=8177, intensity=1.0, duration=2.0, and default FlashParams. These values are the lock: they are what the strike schedule must keep reproducing.

use celsius::lightning::{FlashParams, schedule_strikes};

#[test]
fn schedule_strikes_matches_lab_fixture_seed_8177() {
    let strikes = schedule_strikes(8177, 2.0, &FlashParams::default(), 1.0);

    let actual: Vec<f64> = strikes
        .iter()
        .flat_map(|s| s.sub_flashes.iter().map(|sf| sf.t_peak))
        .collect();

    let expected = [
        0.686_426_830_696_831_4,
        0.743_229_137_501_765_2,
        0.787_250_615_802_313,
        1.088_253_001_338_984_2,
        1.118_277_681_266_922_2,
        1.138_425_216_022_018,
        1.188_662_935_731_537_5,
        1.492_651_469_068_651_4,
        1.549_732_029_658_292_8,
        1.581_225_763_727_189_4,
    ];

    assert_eq!(actual.len(), expected.len(), "sub-flash count mismatch");
    assert_eq!(strikes.len(), 4, "strike count mismatch");
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        assert!(
            (a - e).abs() < 1e-12,
            "sub-flash[{i}] t_peak: got {a}, expected {e}"
        );
    }
}

// Everything below covers the drawing rather than the schedule. It had none: lightning composites on a TUI tick, outside `render()`, so no golden reaches it and the schedule fixture above proved only that the flashes were timed correctly, never that anything was painted.

use celsius::PixelBuffer;
use celsius::colorspace::Rgb;
use celsius::lightning::{Lightning, l_bump_at, overlay};

/// The bolt colour `draw_segment` writes, as a `(252, 250, 240)` round trip through Oklab.
const BOLT: Rgb = Rgb {
    r: 252,
    g: 250,
    b: 240,
};

fn storm() -> Lightning {
    Lightning::new(8177, 1.0, 2.0, true)
}

/// The first instant at which a strike carrying a bolt is drawing.
fn bolt_peak(storm: &Lightning) -> f64 {
    storm
        .strikes
        .iter()
        .find(|s| s.bolt.is_some())
        .expect("seed 8177 with bolts enabled must produce at least one")
        .sub_flashes[0]
        .t_peak
}

fn filled(w: usize, h: usize, shade: u8) -> PixelBuffer {
    PixelBuffer::filled(w, h, Rgb::new(shade, shade, shade))
}

fn count(pixels: &PixelBuffer, colour: Rgb) -> usize {
    pixels.pixels.iter().filter(|p| **p == colour).count()
}

/// The frames between strikes are the overwhelming majority, and on them the overlay has to be a genuine no-op. Anything else would make a still storm shimmer and would defeat the draw-gating that stops the TUI repainting an unchanged sky.
#[test]
fn a_frame_between_strikes_is_left_exactly_as_it_was() {
    let storm = storm();
    let before = filled(104, 50, 40);
    let mut after = before.clone();
    overlay(&mut after, &storm, 0.0);
    assert_eq!(
        before.pixels, after.pixels,
        "a quiet frame must be byte-identical to the sky underneath it"
    );
}

/// A sheet flash lights the whole sky, not a shape in it.
#[test]
fn a_flash_lifts_the_whole_frame() {
    let storm = storm();
    let t = storm.strikes[0].sub_flashes[0].t_peak;
    let before = filled(104, 50, 40);
    let mut after = before.clone();
    overlay(&mut after, &storm, t);

    assert!(
        after
            .pixels
            .iter()
            .zip(&before.pixels)
            .all(|(a, b)| a.r >= b.r && a.g >= b.g && a.b >= b.b),
        "a flash may only add light"
    );
    assert!(
        after.pixels.iter().zip(&before.pixels).any(|(a, b)| a != b),
        "a flash at its own peak has to change something"
    );
}

/// The flash envelope: a fast ramp in, then an exponential decay over `tau`. If the sign of that exponent ever flips, a storm gets brighter the longer ago it struck.
///
/// Measured from the storm's final sub-flash, because `l_bump_at` sums every sub-flash still within six tau and a strike fires two or three of them 30 to 60 ms apart. Sampling the decay of the first one therefore walks straight into the second and reads as a brightening, which is the flicker of a real multi-stroke flash and not a fault.
#[test]
fn the_flash_decays_after_its_peak() {
    let storm = storm();
    let t = storm
        .strikes
        .iter()
        .flat_map(|s| &s.sub_flashes)
        .map(|sf| sf.t_peak)
        .fold(f64::NEG_INFINITY, f64::max);
    let params = &storm.params;

    let at_peak = l_bump_at(&storm.strikes, t, params);
    assert!(at_peak > 0.0, "the peak must light something");

    let mut previous = at_peak;
    for step in 1..=5 {
        let later = l_bump_at(&storm.strikes, t + f64::from(step) * params.tau, params);
        assert!(
            later < previous,
            "{step} tau after the peak the flash reads {later}, no dimmer than {previous}"
        );
        previous = later;
    }
    assert!(
        l_bump_at(&storm.strikes, t + params.tau * 100.0, params) == 0.0,
        "long after the last sub-flash the sky must be dark again"
    );
}

#[test]
fn a_bolt_paints_its_channel() {
    let storm = storm();
    let mut pixels = filled(104, 50, 40);
    overlay(&mut pixels, &storm, bolt_peak(&storm));
    assert!(
        count(&pixels, BOLT) > 0,
        "a strike carrying a bolt has to draw one"
    );
}

/// A bolt is behind the weather, not in front of it. `draw_segment` tests the sky it is about to overwrite and leaves anything already brighter than the occlusion threshold alone, which is what puts the channel behind a lit cloud deck instead of over it.
#[test]
fn a_bolt_behind_a_bright_cloud_does_not_draw() {
    let storm = storm();
    let t = bolt_peak(&storm);

    let mut dark = filled(104, 50, 40);
    overlay(&mut dark, &storm, t);
    assert!(count(&dark, BOLT) > 0, "the control frame must draw a bolt");

    let mut bright = filled(104, 50, 250);
    overlay(&mut bright, &storm, t);
    assert_eq!(
        count(&bright, BOLT),
        0,
        "a sky already brighter than the occlusion threshold must hide the channel entirely"
    );
}

/// The same bug meteors had: geometry generated against the 104x50 reference and drawn straight into whatever buffer the terminal gave, which pinned the bolt to one corner of a wide sky. Lightning scales at draw time instead of storing fractions, because converting generation would change the RNG draw order and move every bolt away from the locked fixture.
#[test]
fn a_bolt_spans_the_frame_at_any_buffer_size() {
    let storm = storm();
    let t = bolt_peak(&storm);

    let extent = |w: usize, h: usize| {
        let mut pixels = filled(w, h, 40);
        overlay(&mut pixels, &storm, t);
        let (mut max_x, mut max_y) = (0usize, 0usize);
        for (i, p) in pixels.pixels.iter().enumerate() {
            if *p == BOLT {
                max_x = max_x.max(i % w);
                max_y = max_y.max(i / w);
            }
        }
        (max_x as f64 / w as f64, max_y as f64 / h as f64)
    };

    let (_, small_y) = extent(104, 50);
    let (_, large_y) = extent(312, 150);
    assert!(
        small_y > 0.5,
        "the bolt should reach well down the reference frame, got {small_y:.2}"
    );
    assert!(
        (large_y - small_y).abs() < 0.1,
        "a wider buffer must stretch the bolt with the sky, not strand it: {small_y:.2} against {large_y:.2}"
    );
}
