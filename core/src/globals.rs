//! The instrument's global settings, what the FM-1 keeps outside a voice: the MIDI channel, the effect channel, the pitch-bend range, a fixed velocity and mono mode
//! (docs/HARDWARE.md, docs/FM-1-MIDI.md). Described like an engine's parameters (`ParamDesc`) so the editor and a package reach them the same way. Pure data: the
//! instrument host (`instrument.rs`) applies them. Glide slides only in `analog` (the other engines ignore it).

use crate::engine::ParamDesc;

/// 0 omni (every channel plays), 1 to 16 only that channel.
pub const G_CHANNEL: u16 = 0;
/// 0 none, 1 to 16: with a MIDI channel set, controllers on this channel reach the map (and the effects) and nothing else does. The FM-1's default is 2.
pub const G_FX_CHANNEL: u16 = 1;
/// Semitones either way; 0 turns the wheel off.
pub const G_BEND: u16 = 2;
/// 0 the played velocity, 1 to 127 every note at that velocity.
pub const G_VELOCITY: u16 = 3;
/// 0 polyphonic, 1 one note at a time, the last played, going back to the one before when it is released.
pub const G_MONO: u16 = 4;
/// 0 off, 1 on: controllers 0 to 23 on the effect channel are the stock FM-1's six effects (`stockmap.rs`) and no longer go through the package's map. Notes and other
/// controllers are unchanged.
pub const G_STOCK_CC: u16 = 5;
/// Portamento in milliseconds, 0 off: a new note slides from the last one's pitch. Only engines that can slide do (`analog`); the others ignore it.
pub const G_GLIDE: u16 = 6;
pub const G_COUNT: usize = 7;

/// name, min, max, default, unit
const DESC: [(&str, i32, i32, i32, &str); G_COUNT] = [
    ("MIDI CHANNEL (0 OMNI)", 0, 16, 0, ""),
    ("EFFECT CHANNEL (0 NONE)", 0, 16, 2, ""),
    ("BEND RANGE", 0, 24, 2, "st"),
    ("FIXED VELOCITY (0 OFF)", 0, 127, 0, ""),
    ("MONO", 0, 1, 0, ""),
    ("STOCK EFFECT CCS", 0, 1, 0, ""),
    ("GLIDE (0 OFF)", 0, 2000, 0, "ms"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Globals {
    p: [i32; G_COUNT],
}

impl Default for Globals {
    fn default() -> Self {
        Globals::new()
    }
}

impl Globals {
    pub fn new() -> Globals {
        let mut p = [0; G_COUNT];
        for (i, d) in DESC.iter().enumerate() {
            p[i] = d.3;
        }
        Globals { p }
    }

    pub fn params(&self) -> Vec<ParamDesc> {
        DESC.iter().enumerate().map(|(i, d)| ParamDesc { id: i as u16, name: d.0.to_string(), min: d.1, max: d.2, default: d.3, unit: d.4 }).collect()
    }

    pub fn get(&self, id: u16) -> Option<i32> {
        self.p.get(id as usize).copied()
    }

    /// Sets a parameter, clamped; false when the id is unknown.
    pub fn set(&mut self, id: u16, value: i32) -> bool {
        let Some(d) = DESC.get(id as usize) else { return false };
        self.p[id as usize] = value.clamp(d.1, d.2);
        true
    }

    /// What a message on channel `ch` (0 to 15) gets: everything, controllers only (the effect channel), or nothing.
    pub fn route(&self, ch: u8) -> Route {
        let main = self.p[G_CHANNEL as usize];
        if main == 0 || main == i32::from(ch) + 1 {
            return Route::All;
        }
        let fx = self.p[G_FX_CHANNEL as usize];
        if fx != 0 && fx == i32::from(ch) + 1 {
            Route::Controllers
        } else {
            Route::None
        }
    }

    /// The velocity a note plays at.
    pub fn velocity(&self, played: u8) -> u8 {
        match self.p[G_VELOCITY as usize] {
            0 => played,
            v => v as u8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    All,
    Controllers,
    None,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_clamp_and_unknown() {
        let mut g = Globals::new();
        assert_eq!(g.params().len(), G_COUNT);
        assert_eq!((g.get(G_CHANNEL), g.get(G_FX_CHANNEL), g.get(G_BEND), g.get(G_VELOCITY), g.get(G_MONO)), (Some(0), Some(2), Some(2), Some(0), Some(0)));
        assert_eq!(g.get(G_STOCK_CC), Some(0));
        assert!(g.set(G_BEND, 99) && g.get(G_BEND) == Some(24));
        assert!(!g.set(77, 1) && g.get(77).is_none());
    }

    #[test]
    fn routing_by_channel() {
        let mut g = Globals::new();
        for ch in 0..16 {
            assert_eq!(g.route(ch), Route::All, "omni takes every channel");
        }
        g.set(G_CHANNEL, 1);
        assert_eq!((g.route(0), g.route(1), g.route(2)), (Route::All, Route::Controllers, Route::None), "channel 1 plays, channel 2 (the default effect channel) controls");
        g.set(G_FX_CHANNEL, 0);
        assert_eq!(g.route(1), Route::None);
        g.set(G_CHANNEL, 2);
        g.set(G_FX_CHANNEL, 2);
        assert_eq!(g.route(1), Route::All, "the main channel wins when both are the same");
    }

    #[test]
    fn fixed_velocity() {
        let mut g = Globals::new();
        assert_eq!(g.velocity(33), 33);
        g.set(G_VELOCITY, 100);
        assert_eq!(g.velocity(33), 100);
    }
}
