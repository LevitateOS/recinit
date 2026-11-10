//! The installed A/B boot contract, shared by recab and its initramfs.

/// An A/B entry requires the immutable boot setup before starting PID 1.
pub const IMMUTABLE_OPTION: &str = "recab.immutable";
/// The slot and state partition, written into the prepared root by recab.
pub const IMMUTABLE_CONFIG: &str = "usr/lib/recab/immutable";
/// Old per-slot configuration, inspected to refuse unsafe overlay loss.
pub const SLOT_STATE_DIR: &str = "lib/recab/slots";
/// A marker in /init: older initramfs images must not silently boot writable.
pub const IMMUTABLE_INIT_MARKER: &str = "# recab-immutable-v2";

/// Check the actual /init in recinit's gzip/newc archive, without extracting
/// files or trusting a sidecar supplied with a different initramfs.
#[cfg(feature = "inspect")]
pub fn check_immutable_initramfs(path: &std::path::Path) -> std::io::Result<()> {
    use std::io::{self, Read};
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "A/B needs a recinit installed initramfs with immutable boot support");
    let mut archive = flate2::read::GzDecoder::new(std::fs::File::open(path)?);
    loop {
        let mut header = [0; 110];
        archive.read_exact(&mut header)?;
        if &header[..6] != b"070701" { return Err(invalid()); }
        let hex = |start, end| -> io::Result<u64> {
            u64::from_str_radix(std::str::from_utf8(&header[start..end]).map_err(|_| invalid())?, 16).map_err(|_| invalid())
        };
        let mode = hex(14, 22)?;
        let size = hex(54, 62)?;
        let namesize = hex(94, 102)?;
        if !(1..=4096).contains(&namesize) { return Err(invalid()); }
        let mut name = vec![0; namesize as usize];
        archive.read_exact(&mut name)?;
        if name.pop() != Some(0) { return Err(invalid()); }
        let mut padding = [0; 3];
        archive.read_exact(&mut padding[..((4 - (110 + namesize) % 4) % 4) as usize])?;
        if name == b"TRAILER!!!" { return Err(invalid()); }
        if name == b"./init" || name == b"init" {
            if mode & 0o170000 != 0o100000 || mode & 0o111 == 0 || size > 1024 * 1024 { return Err(invalid()); }
            let mut script = vec![0; size as usize];
            archive.read_exact(&mut script)?;
            if !script.split(|b| *b == b'\n').any(|line| line == IMMUTABLE_INIT_MARKER.as_bytes()) { return Err(invalid()); }
            // Finish the gzip stream as well: truncated/corrupt archives are
            // refused before recab destroys the inactive slot.
            io::copy(&mut archive, &mut io::sink())?;
            return Ok(());
        }
        if io::copy(&mut archive.by_ref().take(size), &mut io::sink())? != size { return Err(invalid()); }
        archive.read_exact(&mut padding[..((4 - size % 4) % 4) as usize])?;
    }
}
