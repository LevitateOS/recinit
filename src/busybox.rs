//! Busybox in the initramfs: the binary plus a link for every applet it has.
//!
//! The binary is an input (fetched and verified by the release tooling); its
//! own `--list` decides which applet links exist, so no list is kept here.

use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

/// Install busybox as `/bin/busybox` with a `/bin/<applet>` link for each
/// applet. Existing files win, so tools copied in earlier (util-linux
/// `mount`, a real `sh`) are not shadowed.
pub fn setup_busybox(busybox_src: &Path, initramfs_root: &Path) -> Result<()> {
    let output = Command::new(busybox_src)
        .arg("--list")
        .output()
        .with_context(|| format!("running {} --list", busybox_src.display()))?;
    let applets: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|a| !a.is_empty() && !a.contains('/'))
        .map(str::to_string)
        .collect();
    if !output.status.success() || applets.is_empty() {
        bail!(
            "{} is not a usable busybox (`--list` gave no applets)",
            busybox_src.display()
        );
    }

    let bin_dir = initramfs_root.join("bin");
    fs::create_dir_all(&bin_dir)?;
    let busybox_dst = bin_dir.join("busybox");
    fs::copy(busybox_src, &busybox_dst)
        .with_context(|| format!("Failed to copy busybox from {}", busybox_src.display()))?;
    fs::set_permissions(&busybox_dst, fs::Permissions::from_mode(0o755))?;

    for applet in applets {
        let link = bin_dir.join(&applet);
        if !link.exists() && fs::symlink_metadata(&link).is_err() {
            std::os::unix::fs::symlink("busybox", &link)
                .with_context(|| format!("Failed to create symlink for {applet}"))?;
        }
    }
    Ok(())
}
