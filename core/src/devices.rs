//! The FM-1's face, as the matrix sees it (from the vendored emulator's panel, `fm1-live/src/panel.rs`): a 240x240 RGB565 LCD,
//! 14 buttons (matrix ids 0..13), 27 note keys F3..G5 (ids 14..40), 7 encoders and one pot (ADC channel 4).

pub const SCREEN_W: usize = 240;
pub const SCREEN_H: usize = 240;
pub const BUTTONS: usize = 14;
pub const KEYS: usize = 27;
pub const KEY_ID_BASE: u8 = 14;
pub const MATRIX_IDS: usize = 41;
pub const ENCODERS: usize = 7;
pub const POT_CHANNEL: u8 = 4;
pub const SAMPLE_RATE: u32 = 44_100;

/// Button and encoder labels, in matrix-id order.
pub const BUTTON_NAMES: [&str; BUTTONS] = ["OCT-", "OCT+", "FX", "SCL", "ENV", "LFO", "EDIT", "GLO", "HOME", "SAVE", "ARP", "SEQ", "PLAY", "REC"];
pub const ENCODER_NAMES: [&str; ENCODERS] = ["SELECT", "ALGORITHM", "KNOB1", "KNOB2", "KNOB3", "KNOB4", "PRESETS"];

/// RGB565 to 0x00RRGGBB for every pixel (a host that draws 32-bit textures).
pub fn rgb565_to_xrgb(src: &[u16], out: &mut [u32]) {
    for (o, &p) in out.iter_mut().zip(src) {
        let (r, g, b) = (u32::from(p >> 11) & 31, u32::from(p >> 5) & 63, u32::from(p) & 31);
        *o = ((r << 3 | r >> 2) << 16) | ((g << 2 | g >> 4) << 8) | (b << 3 | b >> 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb565_expands_to_full_range() {
        let mut out = [1u32; 3];
        rgb565_to_xrgb(&[0xffff, 0xf800, 0], &mut out);
        assert_eq!(out, [0xffffff, 0xff0000, 0]);
    }

    #[test]
    fn matrix_layout_adds_up() {
        assert_eq!(BUTTONS + KEYS, MATRIX_IDS);
        assert_eq!(BUTTON_NAMES.len(), BUTTONS);
    }
}
