//! Install initramfs builder.
//!
//! Builds a full (~30-50MB) systemd-based initramfs for booting from
//! installed disk. This initramfs:
//! 1. Loads all common storage drivers (NVMe, SATA, USB)
//! 2. Includes systemd for service management
//! 3. Mounts the root filesystem from disk
//! 4. Hands off to the real systemd

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::cpio::build_cpio;
use crate::modules::{copy_kernel_modules, ModulePreset};
use crate::systemd::{copy_firmware, copy_initrd_units, copy_systemd};
use crate::DEFAULT_GZIP_LEVEL;

/// Configuration for building an install initramfs.
#[derive(Debug, Clone)]
pub struct InstallConfig {
    /// Path to rootfs staging directory (contains modules, systemd, firmware)
    pub rootfs: PathBuf,

    /// Output path for the initramfs
    pub output: PathBuf,

    /// Module preset or custom list
    pub module_preset: ModulePreset,

    /// Gzip compression level (1-9)
    pub gzip_level: u32,

    /// Whether to include firmware
    pub include_firmware: bool,
}

impl Default for InstallConfig {
    fn default() -> Self {
        Self {
            rootfs: PathBuf::from("rootfs-staging"),
            output: PathBuf::from("initramfs-installed.img"),
            module_preset: ModulePreset::Install,
            gzip_level: DEFAULT_GZIP_LEVEL,
            include_firmware: true,
        }
    }
}

/// Directories for install initramfs.
const INSTALL_DIRS: &[&str] = &[
    // Standard FHS
    "bin",
    "sbin",
    "usr/bin",
    "usr/sbin",
    "usr/lib",
    "usr/lib64",
    "lib",
    "lib64",
    "etc",
    "dev",
    "proc",
    "sys",
    "run",
    "tmp",
    "var",
    "var/run",
    // Systemd
    "usr/lib/systemd",
    "usr/lib/systemd/system",
    "usr/lib/systemd/system/initrd.target.wants",
    "usr/lib/systemd/system/sysinit.target.wants",
    "usr/lib/systemd/system-generators",
    "etc/systemd/system",
    // Modules
    "usr/lib/modules",
    // Firmware
    "usr/lib/firmware",
    // Udev
    "usr/lib/udev",
    "usr/lib/udev/rules.d",
];

/// Build install initramfs for booting from disk.
pub fn build_install_initramfs(config: &InstallConfig, verbose: bool) -> Result<()> {
    if verbose {
        println!("=== Building Install Initramfs ===\n");
    }

    // Verify rootfs exists
    if !config.rootfs.exists() {
        bail!(
            "rootfs directory not found at {}",
            config.rootfs.display()
        );
    }

    // Find kernel version from modules directory
    let modules_dir = config.rootfs.join("usr/lib/modules");
    let kernel_version = find_kernel_version(&modules_dir)?;

    if verbose {
        println!("  Kernel version: {}", kernel_version);
    }

    // Create temporary build directory
    let output_dir = config.output.parent().unwrap_or(Path::new("."));
    let initramfs_root = output_dir.join("initramfs-install-root");

    // Clean previous build
    if initramfs_root.exists() {
        fs::remove_dir_all(&initramfs_root)?;
    }

    // 1. Create directory structure
    create_install_directory_structure(&initramfs_root, verbose)?;

    // 2. Copy kernel modules
    copy_install_modules(&config.rootfs, &initramfs_root, &kernel_version, config, verbose)?;

    // 3. Copy firmware (optional)
    if config.include_firmware {
        copy_firmware(&config.rootfs, &initramfs_root, verbose)?;
    }

    // 4. Copy systemd and dependencies
    copy_systemd(&config.rootfs, &initramfs_root, verbose)?;

    // 5. Create init symlink to systemd
    let init_path = initramfs_root.join("init");
    if !init_path.exists() {
        std::os::unix::fs::symlink("/usr/lib/systemd/systemd", &init_path)?;
    }

    // 6. Copy initrd systemd units
    copy_initrd_units(&config.rootfs, &initramfs_root, verbose)?;

    // 7. Build CPIO archive
    if verbose {
        println!("Building cpio archive...");
    }
    let temp_cpio = output_dir.join(format!(
        "{}.tmp",
        config.output.file_name().unwrap().to_string_lossy()
    ));
    build_cpio(&initramfs_root, &temp_cpio, config.gzip_level)?;

    // 8. Verify the artifact
    if !temp_cpio.exists() || fs::metadata(&temp_cpio)?.len() < 1024 {
        bail!("Install initramfs build produced invalid or empty file");
    }

    // 9. Atomic rename
    fs::rename(&temp_cpio, &config.output)?;

    // 10. Clean up
    fs::remove_dir_all(&initramfs_root)?;

    if verbose {
        let size = fs::metadata(&config.output)?.len();
        println!("\n=== Install Initramfs Complete ===");
        println!("  Output: {}", config.output.display());
        println!("  Size: {} MB", size / 1024 / 1024);

        // Sanity check
        if size < 5 * 1024 * 1024 {
            println!("  [WARN] Initramfs seems small - may be missing modules or systemd");
        }
    }

    Ok(())
}

/// Find kernel version from modules directory.
fn find_kernel_version(modules_dir: &Path) -> Result<String> {
    if !modules_dir.exists() {
        bail!(
            "Kernel modules directory not found: {}",
            modules_dir.display()
        );
    }

    fs::read_dir(modules_dir)?
        .filter_map(|e| e.ok())
        .find(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .context("No kernel version directory found in modules")
}

/// Create install initramfs directory structure.
fn create_install_directory_structure(root: &Path, verbose: bool) -> Result<()> {
    if verbose {
        println!("Creating install initramfs directory structure...");
    }

    for dir in INSTALL_DIRS {
        fs::create_dir_all(root.join(dir))?;
    }

    // Create merged-usr symlinks (standard for modern distros)
    for (link, target) in [
        ("bin", "usr/bin"),
        ("sbin", "usr/sbin"),
        ("lib", "usr/lib"),
        ("lib64", "usr/lib64"),
    ] {
        let link_path = root.join(link);
        // Remove directory we just created and replace with symlink
        if link_path.is_dir() {
            fs::remove_dir(&link_path)?;
        }
        if !link_path.exists() && !link_path.is_symlink() {
            std::os::unix::fs::symlink(target, &link_path)?;
        }
    }

    Ok(())
}

/// Copy kernel modules for installed system boot.
fn copy_install_modules(
    rootfs_staging: &Path,
    initramfs_root: &Path,
    kernel_version: &str,
    config: &InstallConfig,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!("Copying kernel modules for installed boot...");
    }

    let src_dir = rootfs_staging.join("usr/lib/modules").join(kernel_version);
    let modules = config.module_preset.modules();

    let (copied, builtin, missing) = copy_kernel_modules(
        &src_dir,
        initramfs_root,
        &modules,
        true, // Check for built-in modules
    )?;

    // Missing modules are warnings for install initramfs (not fatal)
    if !missing.is_empty() && verbose {
        println!(
            "  [WARN] {} modules not found (may be built-in or unavailable):",
            missing.len()
        );
        for m in missing.iter().take(5) {
            println!("    - {}", m);
        }
        if missing.len() > 5 {
            println!("    ... and {} more", missing.len() - 5);
        }
    }

    if verbose {
        if builtin > 0 {
            println!("  {} boot modules are built-in to kernel", builtin);
        }
        println!("  Copied {} boot modules", copied);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = InstallConfig::default();
        assert_eq!(config.gzip_level, DEFAULT_GZIP_LEVEL);
        assert!(config.include_firmware);
    }
}
