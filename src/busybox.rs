//! Busybox handling for tiny/live initramfs.
//!
//! Busybox provides all the shell commands needed for the init script
//! in a single statically-linked binary.

use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

/// Commands to symlink from busybox.
///
/// These are the commands used by the init script to mount filesystems,
/// load modules, and switch root.
pub const BUSYBOX_COMMANDS: &[&str] = &[
    "sh",
    "mount",
    "umount",
    "mkdir",
    "cat",
    "ls",
    "sleep",
    "switch_root",
    "echo",
    "test",
    "[",
    "grep",
    "sed",
    "ln",
    "rm",
    "cp",
    "mv",
    "chmod",
    "chown",
    "mknod",
    "losetup",
    "mount.loop",
    "insmod",
    "modprobe",
    "xz",
    "gunzip",
    "find",
    "head",
];

/// Download busybox from URL to cache location.
///
/// Uses curl with progress bar display.
pub fn download_busybox(url: &str, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let status = Command::new("curl")
        .args(["-L", "-o"])
        .arg(dest)
        .args(["--progress-bar", url])
        .status()
        .context("curl command not found - install curl")?;

    if !status.success() {
        bail!("Failed to download busybox from {}", url);
    }

    // Make executable
    let mut perms = fs::metadata(dest)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(dest, perms)?;

    Ok(())
}

/// Set up busybox in initramfs with command symlinks.
///
/// Copies the busybox binary to /bin/busybox and creates symlinks
/// for all the commands listed in BUSYBOX_COMMANDS.
///
/// # Arguments
///
/// * `busybox_src` - Path to the busybox binary
/// * `initramfs_root` - Root of the initramfs being built
/// * `commands` - Optional list of commands to symlink (uses BUSYBOX_COMMANDS if None)
pub fn setup_busybox(
    busybox_src: &Path,
    initramfs_root: &Path,
    commands: Option<&[&str]>,
) -> Result<()> {
    let bin_dir = initramfs_root.join("bin");
    fs::create_dir_all(&bin_dir)?;

    let busybox_dst = bin_dir.join("busybox");

    // Copy busybox
    fs::copy(busybox_src, &busybox_dst)
        .with_context(|| format!("Failed to copy busybox from {}", busybox_src.display()))?;

    // Make executable
    let mut perms = fs::metadata(&busybox_dst)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&busybox_dst, perms)?;

    // Create symlinks for commands
    let cmds = commands.unwrap_or(BUSYBOX_COMMANDS);
    for cmd in cmds {
        let link = bin_dir.join(cmd);
        if !link.exists() {
            std::os::unix::fs::symlink("busybox", &link)
                .with_context(|| format!("Failed to create symlink for {}", cmd))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_busybox_commands_not_empty() {
        assert!(!BUSYBOX_COMMANDS.is_empty());
        assert!(BUSYBOX_COMMANDS.contains(&"sh"));
        assert!(BUSYBOX_COMMANDS.contains(&"mount"));
        assert!(BUSYBOX_COMMANDS.contains(&"switch_root"));
    }

    #[test]
    fn test_setup_busybox_creates_symlinks() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("initramfs");

        // Create a fake busybox
        let busybox_src = temp.path().join("busybox");
        fs::write(&busybox_src, "#!/bin/sh\necho busybox").unwrap();

        // Set up busybox
        setup_busybox(&busybox_src, &root, Some(&["sh", "mount"])).unwrap();

        // Verify
        assert!(root.join("bin/busybox").exists());
        assert!(root.join("bin/sh").is_symlink());
        assert!(root.join("bin/mount").is_symlink());
    }
}
