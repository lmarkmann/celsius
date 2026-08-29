// Snow is the first effect drawn in both stills and motion, and the first whose *form* is computed from the weather rather than chosen by an author. Both of those are properties rather than pixels, so this asserts them directly. A hash could not tell a dendrite from a needle, and the thing most worth catching here is the morphology table being read off by one band.

use celsius::colorspace::{PixelBuffer, Rgb};
use celsius::snow::{
    FlakeForm, Snowfall, flake_count, overlay, supersaturation, vapour_pressure_ice,
    vapour_pressure_water,
};

fn buffer(w: usize, h: usize) -> PixelBuffer {
    PixelBuffer::filled(w, h, Rgb::new(40, 44, 52))
}

fn snowfall(form: FlakeForm, count: u32) -> Snowfall {
    Snowfall {
        form,
        count,
        seed: 2749,
        drift: 0.0,
        opacity: 0.75,
    }
}

/// The diagram's water-saturation line, which Libbrecht gives as the humidity inside dense winter cloud. It has to be 100 and not a plausible-looking 98: near freezing the ice and water curves nearly meet, so ice saturation alone needs 99.05 percent at -1 C and anything below that can only grow a faceted plate however much it looks like a snowstorm.
const IN_CLOUD: f64 = 100.0;
/// Cold air that is not producing much: below the branching threshold at every temperature in the table.
const DRY: f64 = 60.0;

/// The morphology diagram as Libbrecht draws it: temperature picks the habit, supersaturation picks how branched it is. Every band is checked on both sides of the humidity split, because reading the table off by one column is exactly as wrong as reading it off by one row and neither shows up in a render as anything but "the snow looks odd".
#[test]
fn the_morphology_table_matches_the_diagram() {
    let cases = [
        (2.0, IN_CLOUD, FlakeForm::Aggregate),
        (2.0, DRY, FlakeForm::Aggregate),
        (-1.0, DRY, FlakeForm::Plate),
        (-1.0, IN_CLOUD, FlakeForm::SectoredPlate),
        (-5.0, DRY, FlakeForm::Column),
        (-5.0, IN_CLOUD, FlakeForm::Needle),
        (-10.0, DRY, FlakeForm::Column),
        (-10.0, IN_CLOUD, FlakeForm::Plate),
        (-15.0, DRY, FlakeForm::Plate),
        (-15.0, IN_CLOUD, FlakeForm::Dendrite),
        (-20.0, DRY, FlakeForm::Plate),
        (-20.0, IN_CLOUD, FlakeForm::SectoredPlate),
        (-30.0, DRY, FlakeForm::Column),
        (-30.0, IN_CLOUD, FlakeForm::Plate),
    ];
    for (t, rh, expected) in cases {
        let got = FlakeForm::select(t, rh);
        assert_eq!(
            got, expected,
            "{t} C at {rh}% relative humidity should grow {expected:?}, not {got:?}"
        );
    }
}

/// The -15 C fernlike peak is the one feature of the diagram a viewer would actually recognise, and it is a narrow band with plates on both sides of it. An off-by-one on the band edges would move the most photogenic flake there is to a temperature it does not occur at.
#[test]
fn the_dendrite_peak_sits_where_the_diagram_puts_it() {
    assert_eq!(FlakeForm::select(-15.0, IN_CLOUD), FlakeForm::Dendrite);
    assert_eq!(FlakeForm::select(-12.5, IN_CLOUD), FlakeForm::Dendrite);
    assert_eq!(FlakeForm::select(-17.5, IN_CLOUD), FlakeForm::Dendrite);
    // Just outside, on both sides, the branched column of the table is not a dendrite.
    assert_eq!(FlakeForm::select(-11.5, IN_CLOUD), FlakeForm::Plate);
    assert_eq!(FlakeForm::select(-18.5, IN_CLOUD), FlakeForm::SectoredPlate);
}

/// Above freezing nothing keeps its shape on the way down, whatever the cloud grew.
#[test]
fn wet_snow_overrides_the_diagram() {
    for rh in [40.0, 70.0, 100.0] {
        assert_eq!(FlakeForm::select(1.0, rh), FlakeForm::Aggregate);
        assert_eq!(FlakeForm::select(5.0, rh), FlakeForm::Aggregate);
    }
}

/// Ice holds vapour less readily than supercooled water at every temperature below freezing, and that gap is the entire reason a crystal in a cloud of droplets grows instead of evaporating. If the two Magnus forms were ever swapped, `select` would report a dry sky in the middle of a snowstorm and every flake would come out faceted.
#[test]
fn the_magnus_pair_is_ordered_and_monotone() {
    let mut previous_w = 0.0;
    let mut previous_i = 0.0;
    for t in [-40.0, -30.0, -20.0, -15.0, -10.0, -5.0, -1.0] {
        let (w, i) = (vapour_pressure_water(t), vapour_pressure_ice(t));
        assert!(w > i, "at {t} C water saturation {w} must exceed ice {i}");
        assert!(
            w > previous_w,
            "water saturation must rise with temperature"
        );
        assert!(i > previous_i, "ice saturation must rise with temperature");
        previous_w = w;
        previous_i = i;
    }
}

/// The conversion the forecast forces on us: Open-Meteo reports humidity over water and the diagram is drawn over ice, so air well below 100 percent is already supersaturated for a growing crystal. Reading the relative humidity straight onto the diagram's axis would call a snowstorm dry.
#[test]
fn air_below_water_saturation_is_still_supersaturated_over_ice() {
    assert!(
        supersaturation(-15.0, 90.0) > 0.0,
        "90 percent humidity at -15 C is supersaturated with respect to ice"
    );
    assert!(
        supersaturation(-15.0, 100.0) > supersaturation(-15.0, 90.0),
        "more vapour must mean more supersaturation"
    );
    assert!(
        supersaturation(-15.0, 60.0) < supersaturation(-15.0, 90.0),
        "less vapour must mean less"
    );
}

/// Snow is drawn by `render()` as well as by the TUI, so unlike lightning it has to be a function of its clock and nothing else. Two calls at one instant must agree exactly, or a still and the first animated frame would disagree.
#[test]
fn one_instant_gives_one_arrangement() {
    let snow = snowfall(FlakeForm::Dendrite, 120);
    let (mut a, mut b) = (buffer(104, 50), buffer(104, 50));
    overlay(&mut a, &snow, 2.5);
    overlay(&mut b, &snow, 2.5);
    assert_eq!(a, b, "the same instant must produce the same frame");

    let mut later = buffer(104, 50);
    overlay(&mut later, &snow, 2.6);
    assert_ne!(a, later, "a tenth of a second later the snow has moved");
}

/// The bug `rules/reference-size.md` exists for, in the shape `tests/meteors.rs` states it: geometry baked at the reference size leaves the effect stranded in one corner of a wide terminal and does not survive a resize.
#[test]
fn flakes_fill_the_frame_at_any_buffer_size() {
    for (w, h) in [(104, 50), (380, 180)] {
        let mut pixels = buffer(w, h);
        overlay(&mut pixels, &snowfall(FlakeForm::Plate, 400), 1.0);

        let untouched = Rgb::new(40, 44, 52);
        let touched = |x: usize, y: usize| pixels.get(x, y) != untouched;
        let band = |xs: std::ops::Range<usize>, ys: std::ops::Range<usize>| {
            ys.flat_map(|y| xs.clone().map(move |x| (x, y)))
                .any(|(x, y)| touched(x, y))
        };
        let (qw, qh) = (w / 8, h / 8);
        assert!(band(0..qw, 0..h), "nothing drawn against the left edge");
        assert!(
            band(w - qw..w, 0..h),
            "nothing drawn against the right edge"
        );
        assert!(band(0..w, 0..qh), "nothing drawn against the top edge");
        assert!(band(0..w, h - qh..h), "nothing drawn against the bottom");
    }
}

/// The correction `rules/reference-size.md` spells out for `Stars.count` and that the old area-scaled drop count had backwards. The frame subtends a fixed field of view however many pixels are in it, so the number of flakes in shot is set by the weather; what a bigger buffer buys is a bigger flake, not more of them.
#[test]
fn count_is_absolute_while_the_drawn_flake_grows() {
    let snow = snowfall(FlakeForm::Plate, 200);

    // A plate is a single pixel per flake at the reference, so touched pixels count flakes directly (bar the odd collision, which is why this compares against a generous bound rather than an exact figure).
    let touched = |w: usize, h: usize| {
        let mut pixels = buffer(w, h);
        overlay(&mut pixels, &snow, 0.0);
        pixels
            .pixels
            .iter()
            .filter(|p| **p != Rgb::new(40, 44, 52))
            .count()
    };

    let small = touched(104, 50);
    let large = touched(416, 200);
    assert!(
        small <= 200,
        "a plate is one pixel at the reference size, so it cannot mark more pixels than there are flakes"
    );
    assert!(
        large > small * 2,
        "at four times the width a flake must cover more pixels, not the same one: {small} then {large}"
    );
    assert!(
        large < 416 * 200 / 4,
        "growing the flake must not blanket the frame: {large} of {} pixels",
        416 * 200
    );
}

/// Density comes from the forecast's snowfall rate, and the compression matters as much as the direction: without it a blizzard fills every pixel and stops reading as snow at all.
#[test]
fn count_follows_the_snowfall_rate_and_saturates() {
    let mut previous = 0;
    for rate in [0.1, 0.25, 0.5, 1.0, 2.0] {
        let n = flake_count(rate);
        assert!(
            n >= previous,
            "more snow must not mean fewer flakes: {rate} cm/h gave {n} after {previous}"
        );
        previous = n;
    }
    assert_eq!(flake_count(0.0), 24, "the floor keeps light snow visible");
    assert_eq!(flake_count(1000.0), 420, "and the ceiling holds");
    assert_eq!(
        flake_count(0.5),
        140,
        "moderate snow is the reference the rest is measured from"
    );
}

/// A sky with no snow in it must be left exactly as it was, which is what lets `render()` call the overlay unconditionally without any golden moving.
#[test]
fn no_flakes_is_a_no_op() {
    let before = buffer(104, 50);
    let mut after = before.clone();
    overlay(&mut after, &snowfall(FlakeForm::Plate, 0), 4.0);
    assert_eq!(before, after, "zero flakes must not touch the buffer");

    let mut transparent = before.clone();
    let mut invisible = snowfall(FlakeForm::Plate, 200);
    invisible.opacity = 0.0;
    overlay(&mut transparent, &invisible, 4.0);
    assert_eq!(before, transparent, "zero opacity must not touch it either");
}

/// The crosswind is the only part of the wind a flat frame can show, and it is what separates snow drifting past from snow falling straight down.
#[test]
fn drift_carries_flakes_sideways() {
    let still = snowfall(FlakeForm::Plate, 200);
    let mut blown = still.clone();
    blown.drift = 0.08;

    let (mut a, mut b) = (buffer(104, 50), buffer(104, 50));
    overlay(&mut a, &still, 3.0);
    overlay(&mut b, &blown, 3.0);
    assert_ne!(a, b, "a crosswind must move the snow across the frame");
}
