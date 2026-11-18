//! FM1-NTT core: the machine behind the sandbox, with no UI and no I/O of its own.
//!
//! The CPU, memory map and peripherals are the vendored `fm1-emu` crates (MIT, `vendor/fm1-emu/VENDORED.md`); this crate
//! wraps them as a `Machine` that a host steps and reads, and as a C API (`fm1ntt.h`) for the SDL2 host.
//!
//! Rules: nothing here panics on bad input (a bad image is an `Error`); no firmware is in this repository (docs/FIRMWARE.md).

pub mod analog;
pub mod arp;
pub mod bench;
pub mod capi;
pub mod capi_inst;
pub mod capi_sound;
pub mod devices;
pub mod engine;
pub mod evq;
pub mod extract;
pub mod factory;
pub mod firmware;
pub mod fm4;
pub mod fm6;
pub mod fx;
pub mod instrument;
pub mod machine;
pub mod package;
pub mod port;
pub mod sampler;
pub mod seq;
pub mod verify;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every way the core can refuse, so a host can say why on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The image is empty, too big, or not a package this core reads (why).
    BadImage(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::BadImage(why) => write!(f, "bad firmware image: {why}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
