//! Systemd copying utilities for install initramfs.
//!
//! Provides functions to copy systemd binaries, units, and library
//! dependencies into the initramfs for systemd-based boot.

use anyhow::Result;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::elf::{copy_library_to, get_all_dependencies};

/// Essential systemd binaries needed for initrd boot.
pub const SYSTEMD_FILES: &[&str] = &[
    "usr/lib/systemd/systemd",
    "usr/lib/systemd/systemd-udevd",
    "usr/lib/systemd/systemd-journald",
    "usr/lib/systemd/systemd-modules-load",
    "usr/lib/systemd/systemd-sysctl",
    "usr/lib/systemd/systemd-fsck",
    "usr/lib/systemd/systemd-remount-fs",
    "usr/lib/systemd/systemd-sulogin-shell",
    "usr/lib/systemd/systemd-shutdown",
    "usr/lib/systemd/systemd-executor",
    "usr/bin/systemctl",
    "usr/bin/udevadm",
    "usr/sbin/modprobe",
    "usr/sbin/insmod",
    "usr/bin/kmod",
    "usr/sbin/fsck",
    "usr/sbin/fsck.ext4",
    "usr/sbin/e2fsck",
    "usr/sbin/blkid",
    "usr/bin/mount",
    "usr/bin/umount",
    "usr/sbin/switch_root",
    "usr/bin/bash",
    "usr/bin/sh",
];

/// Essential initrd systemd units.
pub const INITRD_UNITS: &[&str] = &[
    // Targets
    "initrd.target",
    "initrd-root-fs.target",
    "initrd-root-device.target",
    "initrd-switch-root.target",
    "initrd-fs.target",
    "sysinit.target",
    "basic.target",
    "local-fs.target",
    "local-fs-pre.target",
    "slices.target",
    "sockets.target",
    "paths.target",
    "timers.target",
    "swap.target",
    "emergency.target",
    "rescue.target",
    // Services
    "systemd-journald.service",
    "systemd-udevd.service",
    "systemd-udev-trigger.service",
    "systemd-modules-load.service",
    "systemd-sysctl.service",
    "systemd-fsck@.service",
    "systemd-fsck-root.service",
    "systemd-remount-fs.service",
    "initrd-switch-root.service",
    "initrd-cleanup.service",
    "initrd-udevadm-cleanup-db.service",
    "initrd-parse-etc.service",
    // Sockets
    "systemd-journald.socket",
    "systemd-journald-dev-log.socket",
    "systemd-udevd-control.socket",
    "systemd-udevd-kernel.socket",
    // Slices
    "-.slice",
    "system.slice",
];

/// Copy systemd binaries and their dependencies to initramfs.
pub fn copy_systemd(
    rootfs_staging: &Path,
    initramfs_root: &Path,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!("Copying systemd for init...");
    }

    // Copy essential binaries
    for file in SYSTEMD_FILES {
        let src = rootfs_staging.join(file);
        let dst = initramfs_root.join(file);

        if src.exists() {
            fs::create_dir_all(dst.parent().unwrap())?;
            fs::copy(&src, &dst)?;

            // Make executable
            let mut perms = fs::metadata(&dst)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dst, perms)?;
        }
    }

    // Copy shared libraries for all binaries
    if verbose {
        println!("  Resolving library dependencies...");
    }

    let extra_lib_paths: &[&str] = &["usr/lib64/systemd"];

    for file in SYSTEMD_FILES {
        let src = rootfs_staging.join(file);
        if !src.exists() {
            continue;
        }

        let deps = get_all_dependencies(rootfs_staging, &src, extra_lib_paths)?;
        for lib_name in deps {
            copy_library_to(
                rootfs_staging,
                &lib_name,
                initramfs_root,
                "usr/lib64",
                "usr/lib",
                extra_lib_paths,
                &["systemd"],
            )?;
        }
    }

    // Copy ld-linux (the dynamic linker itself)
    for ld_name in ["ld-linux-x86-64.so.2", "ld-linux.so.2"] {
        let src = rootfs_staging.join("usr/lib64").join(ld_name);
        if src.exists() {
            let dst = initramfs_root.join("usr/lib64").join(ld_name);
            if !dst.exists() {
                fs::copy(&src, &dst)?;
            }
        }
    }

    // Create symlinks for systemd-specific libraries in /usr/lib64
    // libsystemd-shared and libsystemd-core are in /usr/lib64/systemd/ which
    // isn't in the default library search path, so we symlink them
    let systemd_lib_dir = initramfs_root.join("usr/lib64/systemd");
    let lib64_dir = initramfs_root.join("usr/lib64");
    if systemd_lib_dir.exists() {
        for entry in fs::read_dir(&systemd_lib_dir)? {
            let entry = entry?;
            let filename = entry.file_name();
            let name = filename.to_string_lossy();
            if name.starts_with("libsystemd-") && name.ends_with(".so") {
                let target = format!("systemd/{}", name);
                let link = lib64_dir.join(&*name);
                if !link.exists() {
                    std::os::unix::fs::symlink(&target, &link)?;
                }
            }
        }
    }

    if verbose {
        println!("  Copied systemd and dependencies");
    }

    Ok(())
}

/// Copy essential initrd systemd units.
pub fn copy_initrd_units(
    rootfs_staging: &Path,
    initramfs_root: &Path,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!("Copying initrd systemd units...");
    }

    let src_unit_dir = rootfs_staging.join("usr/lib/systemd/system");
    let dst_unit_dir = initramfs_root.join("usr/lib/systemd/system");
    fs::create_dir_all(&dst_unit_dir)?;

    for unit in INITRD_UNITS {
        let src = src_unit_dir.join(unit);
        let dst = dst_unit_dir.join(unit);
        if src.exists() {
            fs::copy(&src, &dst)?;
        }
    }

    // Copy udev rules
    let udev_rules_src = rootfs_staging.join("usr/lib/udev/rules.d");
    let udev_rules_dst = initramfs_root.join("usr/lib/udev/rules.d");
    if udev_rules_src.exists() {
        crate::elf::copy_dir_recursive(&udev_rules_src, &udev_rules_dst)?;
    }

    // Copy systemd generators
    let generators_src = rootfs_staging.join("usr/lib/systemd/system-generators");
    let generators_dst = initramfs_root.join("usr/lib/systemd/system-generators");
    if generators_src.exists() {
        fs::create_dir_all(&generators_dst)?;
        for entry in fs::read_dir(&generators_src)? {
            let entry = entry?;
            let src = entry.path();
            let dst = generators_dst.join(entry.file_name());
            if src.is_file() {
                fs::copy(&src, &dst)?;
                let mut perms = fs::metadata(&dst)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&dst, perms)?;
            }
        }
    }

    // Create default.target symlink
    let default_target = initramfs_root.join("usr/lib/systemd/system/default.target");
    if !default_target.exists() {
        std::os::unix::fs::symlink("initrd.target", &default_target)?;
    }

    if verbose {
        println!("  Copied initrd units and udev rules");
    }

    Ok(())
}

/// Copy firmware directory (for hardware support).
pub fn copy_firmware(
    rootfs_staging: &Path,
    initramfs_root: &Path,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!("Copying firmware...");
    }

    let src = rootfs_staging.join("usr/lib/firmware");
    let dst = initramfs_root.join("usr/lib/firmware");

    if src.exists() {
        let size = crate::elf::copy_dir_recursive(&src, &dst)?;
        if verbose {
            println!("  Copied firmware ({:.1} MB)", size as f64 / 1_000_000.0);
        }
    } else if verbose {
        println!("  No firmware found (skipping)");
    }

    Ok(())
}
