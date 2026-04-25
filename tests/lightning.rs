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
        0.68642683069683141,
        0.74322913750176522,
        0.78725061580231304,
        1.08825300133898417,
        1.11827768126692217,
        1.13842521602201807,
        1.18866293573153747,
        1.49265146906865143,
        1.54973202965829282,
        1.58122576372718937,
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
