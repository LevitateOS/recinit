//! Install initramfs builder.
//!
//! Builds a full (~30-50MB) systemd-based initramfs for booting from
//! installed disk. This initramfs:
//! 1. Loads all common storage drivers (NVMe, SATA, USB)
//! 2. Includes systemd for service management
//! 3. Mounts the root filesystem from disk
//! 4. Hands off to the real systemd

use anyhow::{bail, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::busybox::setup_busybox;
use crate::cpio::build_archive;
use crate::modules::{KernelModules, ModulePreset};
use crate::systemd::{copy_firmware, copy_initrd_units, copy_systemd};

/// Configuration for building an install initramfs.
#[derive(Debug, Clone)]
pub struct InstallConfig {
    /// Path to rootfs staging directory (contains systemd, firmware)
    pub rootfs: PathBuf,

    /// Path to static busybox binary (emergency shell)
    pub busybox_path: PathBuf,

    /// The kernel's module tree, `/lib/modules/<release>`
    pub modules_dir: PathBuf,

    /// Output path for the initramfs
    pub output: PathBuf,

    /// Module preset or custom list
    pub module_preset: ModulePreset,

    /// Gzip compression level (1-9)
    pub gzip_level: u32,

    /// Whether to include firmware
    pub include_firmware: bool,
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
    // sysroot - systemd mounts root here before switch-root
    "sysroot",
];

/// Build install initramfs for booting from disk.
pub fn build_install_initramfs(config: &InstallConfig, verbose: bool) -> Result<()> {
    if verbose {
        println!("=== Building Install Initramfs ===\n");
    }

    // Verify rootfs exists
    if !config.rootfs.exists() {
        bail!("rootfs directory not found at {}", config.rootfs.display());
    }

    let kernel = KernelModules::open(&config.modules_dir)?;
    let resolved = kernel.resolve(&config.module_preset.names())?;
    if verbose {
        println!("  Kernel release: {}", kernel.release());
    }

    build_archive(&config.output, config.gzip_level, |root| {
        // 1. Create directory structure
        create_install_directory_structure(root, verbose)?;

        // Modules go to usr/lib (merged-usr), so the lib -> usr/lib link
        // made above is not needed for them.
        let installed = kernel.install(&resolved, root, "usr/lib/modules", false)?;
        if verbose {
            println!(
                "  {} modules installed, {} built into the kernel",
                installed.len(),
                resolved.builtin.len()
            );
        }

        // 3. Copy firmware (optional)
        if config.include_firmware {
            copy_firmware(&config.rootfs, root, verbose)?;
        }

        // 4. Copy systemd and dependencies
        copy_systemd(&config.rootfs, root, verbose)?;

        // 4b. Set up busybox for emergency shell
        setup_busybox(&config.busybox_path, root)?;

        // 5. Create init symlink to systemd
        let init_path = root.join("init");
        if !init_path.exists() {
            std::os::unix::fs::symlink("/usr/lib/systemd/systemd", &init_path)?;
        }

        // 5b. Create /etc/initrd-release (required for systemd to recognize initrd)
        // Without this, initrd-root-device.target assertion fails
        let etc_dir = root.join("etc");
        fs::create_dir_all(&etc_dir)?;
        fs::write(
            etc_dir.join("initrd-release"),
            "NAME=\"LevitateOS initramfs\"\nVERSION=\"1.0\"\nID=levitateos\n",
        )?;

        // 5c. Create minimal /etc/passwd and /etc/group (required by systemd/udev)
        // Without these, systemd-udevd and other services may fail to start or
        // run with incorrect permissions, causing /dev/disk/by-uuid to not be created
        fs::write(
            etc_dir.join("passwd"),
            "root:x:0:0:root:/root:/bin/sh\nnobody:x:65534:65534:Nobody:/:/sbin/nologin\n",
        )?;
        fs::write(
            etc_dir.join("group"),
            "root:x:0:\ntty:x:5:\ndisk:x:6:\nwheel:x:10:\nnobody:x:65534:\n",
        )?;
        fs::write(etc_dir.join("shadow"), "root:*:19000:0:99999:7:::\n")?;

        // 5d. Create /etc/nsswitch.conf (required for glibc user/group lookups)
        // Without this, getpwnam/getgrnam may fail, causing udev rules to break
        fs::write(
            etc_dir.join("nsswitch.conf"),
            "passwd:     files\ngroup:      files\nshadow:     files\nhosts:      files dns\n",
        )?;

        // 6. Copy initrd systemd units
        copy_initrd_units(&config.rootfs, root, verbose)?;
        Ok(())
    })?;

    if verbose {
        let size = fs::metadata(&config.output)?.len();
        println!("\n=== Install Initramfs Complete ===");
        println!("  Output: {}", config.output.display());
        println!("  Size: {} MB", size / 1024 / 1024);
    }

    Ok(())
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
