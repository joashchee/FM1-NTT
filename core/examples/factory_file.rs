//! `factory_file > bundled-firmware/instruments/factory-voices.fm1i`: the built-in voices as the instrument package the app ships (a test keeps the file equal to this).

fn main() {
    print!("{}", fm1ntt::instrument::factory_file_text());
}
