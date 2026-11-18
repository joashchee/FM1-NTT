//! The stock FM-1's effect controllers (docs/FM-1-MIDI.md): on the effect channel (default 2), controllers 0 to 23 are six effects' switches and parameters, from M-VAVE's
//! MIDI table. This turns one such message into an action on one of our effects, by name, so reordering the chain changes nothing. Pure data, no state.
//!
//! **What is the table's and what is ours.** The wire numbers, the grouping and the ranges are the table's. Our effects are written fresh and fitted to nothing, so how a
//! value lands on them is our own choice, listed here: the filter's cutoff is 100 Hz to 20 kHz in 100 log steps (the manual); Q 0 to 10 is our 0 to 100; a reverb's
//! type is our damping and its decay our room size; a delay's rate is a shorter time as it rises and its decay is our feedback; a switch is a bypass. **The stock
//! firmware itself did not react to most of these controllers in the emulator** (docs/FM-1-MIDI.md, "What was tried"), so this follows the table, not a measurement.

/// What a controller does to the effect it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// the effect in or out of the chain (0 out, anything else in)
    Switch(bool),
    /// one of its parameters, in that effect's own units
    Param(u16, i32),
}

/// The stock table: the highest controller number it uses is 23.
pub const CC_COUNT: u8 = 24;

/// The effect the controller belongs to and what it does at `value` (0 to 127); `None` for a controller outside 0 to 23.
pub fn act(cc: u8, value: u8) -> Option<(&'static str, Act)> {
    let v = i32::from(value.min(127));
    let pct = v.min(100); // the 0 to 100 parameters: anything above is the top
    let (name, a) = match cc {
        0 => ("filter", Act::Switch(v > 0)),
        1 => ("filter", Act::Param(0, v.min(2))),
        2 => ("filter", Act::Param(1, cutoff_hz(v))),
        3 => ("filter", Act::Param(2, v.min(10) * 10)),
        4 => ("reverb", Act::Switch(v > 0)),
        // room, hall, plate: a smaller room damps more
        5 => ("reverb", Act::Param(1, [60, 40, 20][v.min(2) as usize])),
        6 => ("reverb", Act::Param(0, pct * 99 / 100)),
        7 => ("reverb", Act::Param(2, pct)),
        8 => ("delay", Act::Switch(v > 0)),
        9 => ("delay", Act::Param(1, pct * 90 / 100)),
        // a faster rate is a shorter time: 100 is 50 ms, 0 is a second
        10 => ("delay", Act::Param(0, 1000 - pct * 95 / 10)),
        11 => ("delay", Act::Param(2, pct)),
        12 => ("distortion", Act::Switch(v > 0)),
        13 => ("distortion", Act::Param(0, pct)),
        14 => ("distortion", Act::Param(1, pct)),
        15 => ("distortion", Act::Param(2, pct)),
        16 => ("chorus", Act::Switch(v > 0)),
        17 => ("chorus", Act::Param(0, pct.max(1))),
        18 => ("chorus", Act::Param(1, pct)),
        19 => ("chorus", Act::Param(2, pct)),
        20 => ("phaser", Act::Switch(v > 0)),
        21 => ("phaser", Act::Param(0, pct.max(1))),
        22 => ("phaser", Act::Param(1, pct)),
        23 => ("phaser", Act::Param(2, pct)),
        _ => return None,
    };
    Some((name, a))
}

/// 100 Hz to 20 kHz in 100 logarithmic steps; a value above 100 is the top.
fn cutoff_hz(v: i32) -> i32 {
    let step = f64::from(v.min(100));
    (100.0 * 200f64.powf(step / 100.0)).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stock_controller_is_known_and_the_rest_are_not() {
        for cc in 0..CC_COUNT {
            assert!(act(cc, 0).is_some() && act(cc, 127).is_some(), "cc {cc}");
        }
        assert!(act(24, 1).is_none() && act(74, 1).is_none() && act(127, 1).is_none());
    }

    #[test]
    fn groups_of_four_and_the_names() {
        let names: Vec<&str> = (0..CC_COUNT).map(|c| act(c, 1).unwrap().0).collect();
        assert_eq!(&names[..4], ["filter"; 4]);
        assert_eq!(&names[4..8], ["reverb"; 4]);
        assert_eq!(&names[8..12], ["delay"; 4]);
        assert_eq!(&names[12..16], ["distortion"; 4]);
        assert_eq!(&names[16..20], ["chorus"; 4]);
        assert_eq!(&names[20..], ["phaser"; 4]);
        for g in 0..6u8 {
            assert!(matches!(act(g * 4, 1).unwrap().1, Act::Switch(true)), "the first of each four is the switch");
            assert!(matches!(act(g * 4, 0).unwrap().1, Act::Switch(false)));
        }
    }

    #[test]
    fn every_value_lands_inside_our_effect_ranges() {
        use crate::fx::make;
        for cc in 0..CC_COUNT {
            for v in 0..=127u8 {
                let (name, a) = act(cc, v).unwrap();
                if let Act::Param(id, x) = a {
                    let e = make(name, 44_100.0).unwrap();
                    let d = e.params().into_iter().find(|d| d.id == id).unwrap_or_else(|| panic!("{name} has no parameter {id} (cc {cc})"));
                    assert!((d.min..=d.max).contains(&x), "cc {cc} value {v}: {name}.{id} = {x} is outside {}..{}", d.min, d.max);
                }
            }
        }
    }

    #[test]
    fn the_scalings() {
        assert_eq!(act(2, 0), Some(("filter", Act::Param(1, 100))));
        assert_eq!(act(2, 100), Some(("filter", Act::Param(1, 20_000))));
        assert_eq!(act(2, 107), Some(("filter", Act::Param(1, 20_000))), "the table's 107 is the top");
        assert_eq!(act(2, 50), Some(("filter", Act::Param(1, 1414))), "half way is the geometric mean, 100 Hz x 200^0.5");
        assert_eq!(act(3, 10), Some(("filter", Act::Param(2, 100))));
        assert_eq!(act(1, 99), Some(("filter", Act::Param(0, 2))));
        assert_eq!(act(10, 0), Some(("delay", Act::Param(0, 1000))));
        assert_eq!(act(10, 100), Some(("delay", Act::Param(0, 50))));
        assert_eq!(act(9, 127), Some(("delay", Act::Param(1, 90))));
        assert_eq!(act(5, 1), Some(("reverb", Act::Param(1, 40))));
        assert_eq!(act(17, 0), Some(("chorus", Act::Param(0, 1))), "a rate of 0 is the slowest, not stopped");
    }

    #[test]
    fn the_cutoff_rises_with_the_value() {
        let mut last = 0;
        for v in 0..=100u8 {
            let Some((_, Act::Param(_, hz))) = act(2, v) else { panic!() };
            assert!(hz > last, "step {v}");
            last = hz;
        }
    }
}
