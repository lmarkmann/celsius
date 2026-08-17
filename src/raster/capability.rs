//! What the terminal can be trusted with.
//!
//! Only colour depth is decided here. Whether a font has a glyph for U+2596 is not something a terminal will answer: there is no query for it, and cursor-position probing measures how wide a glyph is, not whether it is a replacement box. So geometry stays the user's choice and only this half is inferred.
//!
//! The policy is to downgrade only on positive evidence. `TERM` is the tempting signal and the wrong one, because `xterm-256color` is the conventional value on a great many terminals that handle 24-bit colour perfectly well, so reading it as a limit would quietly degrade them. Apple Terminal's build number is the one case where the answer is actually known, and everything else keeps what celsius does today.

use super::ColorDepth;

/// Terminal 2.15, shipped with macOS 26 Tahoe, is the first with real 24-bit colour. Earlier builds accept the sequence and approximate it into their own palette, which is why the sky bands there and why celsius cannot see it happen.
const FIRST_TRUECOLOR_APPLE_TERMINAL: u32 = 464;

pub fn detect_depth() -> ColorDepth {
    if declares_truecolor() {
        return ColorDepth::True;
    }
    if let Some(build) = apple_terminal_build() {
        return if build >= FIRST_TRUECOLOR_APPLE_TERMINAL {
            ColorDepth::True
        } else {
            ColorDepth::Indexed256
        };
    }
    ColorDepth::True
}

fn declares_truecolor() -> bool {
    std::env::var("COLORTERM")
        .is_ok_and(|v| v.eq_ignore_ascii_case("truecolor") || v.eq_ignore_ascii_case("24bit"))
}

/// Apple Terminal reports a build number such as `455.1`, not a marketing version, so only the part before the first dot is a number worth comparing.
fn apple_terminal_build() -> Option<u32> {
    if std::env::var("TERM_PROGRAM").ok()? != "Apple_Terminal" {
        return None;
    }
    std::env::var("TERM_PROGRAM_VERSION")
        .ok()?
        .split('.')
        .next()?
        .parse()
        .ok()
}
