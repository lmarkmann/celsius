// Bit-parity test against the celsius-lab lightning prototype.
// Fixture captured from celsius-lab/src/skyterm_lab/lightning.py with
// seed=8177, intensity=1.0, duration=2.0, default FlashParams.

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
