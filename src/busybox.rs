//! Busybox handling for tiny/live initramfs.
//!
//! Busybox provides all the shell commands needed for the init script
//! in a single statically-linked binary.

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
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

/// Default busybox download URL.
pub const BUSYBOX_URL: &str =
    "https://busybox.net/downloads/binaries/1.35.0-x86_64-linux-musl/busybox";

/// SHA256 hash of the default busybox binary.
pub const BUSYBOX_SHA256: &str = "6e123e7f3202a8c1e9b1f94d8941580a25135382b99e8d3e34fb858bba311348";

/// Environment variable to override busybox URL.
pub const BUSYBOX_URL_ENV: &str = "BUSYBOX_URL";

/// Verify SHA256 hash of a file.
pub fn verify_sha256(file: &Path, expected: &str) -> Result<()> {
    let mut f = fs::File::open(file).with_context(|| format!("cannot open {}", file.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];

    loop {
        let n = f.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    let hash = hex::encode(hasher.finalize());
    if hash != expected.to_lowercase() {
        bail!(
            "SHA256 integrity check failed for '{}'\n  expected: {}\n  got:      {}",
            file.display(),
            expected.to_lowercase(),
            hash
        );
    }
    Ok(())
}

/// Download busybox to a cache directory, verifying SHA256 if using default URL.
///
/// Returns the path to the cached busybox binary.
pub fn download_and_cache_busybox(cache_dir: &Path) -> Result<PathBuf> {
    let cache_path = cache_dir.join("busybox-static");

    if cache_path.exists() {
        return Ok(cache_path);
    }

    let url = std::env::var(BUSYBOX_URL_ENV).unwrap_or_else(|_| BUSYBOX_URL.to_string());
    let is_default_url = std::env::var(BUSYBOX_URL_ENV).is_err();

    println!("  Downloading static busybox from {}", url);
    download_busybox(&url, &cache_path)?;

    if is_default_url {
        println!("  Verifying checksum...");
        verify_sha256(&cache_path, BUSYBOX_SHA256)
            .context("Busybox checksum verification failed")?;
    } else {
        println!("  Skipping checksum (custom URL)");
    }

    Ok(cache_path)
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
