//! What a user-supplied firmware image may be. Nothing is bundled (docs/FIRMWARE.md); this only bounds what the user hands over.
//! The package format (`.fwsc`) is read by the vendored `fm1_soc::stock_from_package`; a raw XIP image needs its entry address.

use crate::{Error, Result};

/// Larger than any FM-1 flash chip would hold; a bigger file is not firmware.
pub const MAX_IMAGE: usize = 32 * 1024 * 1024;

pub fn check_size(bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() {
        return Err(Error::BadImage("empty".into()));
    }
    if bytes.len() > MAX_IMAGE {
        return Err(Error::BadImage("larger than any FM-1 flash".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds() {
        assert!(check_size(&[]).is_err());
        assert!(check_size(&vec![0; MAX_IMAGE + 1]).is_err());
        assert!(check_size(&[1]).is_ok());
    }
}
