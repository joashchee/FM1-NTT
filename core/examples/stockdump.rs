//! `stockdump FIRMWARE.fwsc`: asks a firmware for a DX7 voice dump over USB MIDI and over the jack (bulk request `F0 43 20 09 F7`, single request `F0 43 20 00 F7`, the 1-byte
//! channel forms too) and prints what comes back.
use fm1ntt::machine::Machine;

fn run(m: &mut Machine, ms: usize) {
    let mut out = vec![0i16; 2 * 441];
    for _ in 0..ms / 10 {
        m.render(&mut out, 4_000_000);
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("firmware");
    let pkg = std::fs::read(path).expect("read");
    let mut m = Machine::from_package(&pkg).expect("load");
    run(&mut m, 3000);
    m.usb_plug();
    run(&mut m, 1000);
    for req in [vec![0xF0u8, 0x43, 0x20, 0x09, 0xF7], vec![0xF0, 0x43, 0x20, 0x00, 0xF7], vec![0xF0, 0x43, 0x20, 0x04, 0xF7]] {
        let sent = m.usb_midi_send(&req);
        run(&mut m, 1500);
        let got = m.usb_midi_take();
        let total: usize = got.iter().map(Vec::len).sum();
        println!("request {:02x?}: sent {sent}, {} packets, {total} bytes back {:02x?}", req, got.len(), got.iter().take(3).collect::<Vec<_>>());
    }
}
