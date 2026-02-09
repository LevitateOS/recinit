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
    "usr/lib/systemd/systemd-makefs",
    "usr/bin/systemctl",
    "usr/bin/systemd-tmpfiles",
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
pub fn copy_systemd(rootfs_staging: &Path, initramfs_root: &Path, verbose: bool) -> Result<()> {
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
            // Match versioned library names like libsystemd-core-257-13.el10.rocky.0.1.so
            // The libraries have version suffixes, so check for .so anywhere in the name
            if name.starts_with("libsystemd-") && name.contains(".so") {
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

    // Copy udev helper programs (needed by udev rules for device identification)
    copy_udev_helpers(rootfs_staging, initramfs_root, verbose)?;

    // Copy tmpfiles.d (needed by systemd-tmpfiles for device node creation)
    let tmpfiles_src = rootfs_staging.join("usr/lib/tmpfiles.d");
    let tmpfiles_dst = initramfs_root.join("usr/lib/tmpfiles.d");
    if tmpfiles_src.exists() {
        fs::create_dir_all(&tmpfiles_dst)?;
        // Copy only essential tmpfiles configs for initrd
        for name in &[
            "static-nodes-permissions.conf",
            "systemd.conf",
            "tmp.conf",
            "var.conf",
        ] {
            let src = tmpfiles_src.join(name);
            let dst = tmpfiles_dst.join(name);
            if src.exists() {
                fs::copy(&src, &dst)?;
            }
        }
        if verbose {
            println!("  Copied tmpfiles.d configurations");
        }
    }

    // Copy systemd generators (essential for parsing root= kernel parameter)
    let generators_src = rootfs_staging.join("usr/lib/systemd/system-generators");
    let generators_dst = initramfs_root.join("usr/lib/systemd/system-generators");
    if generators_src.exists() {
        fs::create_dir_all(&generators_dst)?;

        // Essential generators for initrd boot
        let essential_generators = [
            "systemd-fstab-generator",
            "systemd-gpt-auto-generator",
            "systemd-debug-generator",
        ];

        let extra_lib_paths: &[&str] = &["usr/lib64/systemd"];

        for gen_name in &essential_generators {
            let src = generators_src.join(gen_name);
            let dst = generators_dst.join(gen_name);
            if src.exists() {
                fs::copy(&src, &dst)?;
                let mut perms = fs::metadata(&dst)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&dst, perms)?;

                // Copy generator dependencies
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
        }
    }

    // Create default.target symlink
    let default_target = initramfs_root.join("usr/lib/systemd/system/default.target");
    if !default_target.exists() {
        std::os::unix::fs::symlink("initrd.target", &default_target)?;
    }

    // Copy .wants directory symlinks (enables services during boot)
    copy_wants_symlinks(rootfs_staging, initramfs_root, verbose)?;

    if verbose {
        println!("  Copied initrd units and udev rules");
    }

    Ok(())
}

/// Symlinks to copy from .wants directories to enable services during initrd boot.
const INITRD_WANTS_SYMLINKS: &[(&str, &str)] = &[
    // sysinit.target.wants - services needed during early boot
    (
        "sysinit.target.wants/dev-hugepages.mount",
        "../dev-hugepages.mount",
    ),
    (
        "sysinit.target.wants/dev-mqueue.mount",
        "../dev-mqueue.mount",
    ),
    (
        "sysinit.target.wants/kmod-static-nodes.service",
        "../kmod-static-nodes.service",
    ),
    (
        "sysinit.target.wants/sys-kernel-config.mount",
        "../sys-kernel-config.mount",
    ),
    (
        "sysinit.target.wants/sys-kernel-debug.mount",
        "../sys-kernel-debug.mount",
    ),
    (
        "sysinit.target.wants/sys-kernel-tracing.mount",
        "../sys-kernel-tracing.mount",
    ),
    (
        "sysinit.target.wants/systemd-ask-password-console.path",
        "../systemd-ask-password-console.path",
    ),
    (
        "sysinit.target.wants/systemd-modules-load.service",
        "../systemd-modules-load.service",
    ),
    (
        "sysinit.target.wants/systemd-sysctl.service",
        "../systemd-sysctl.service",
    ),
    (
        "sysinit.target.wants/systemd-tmpfiles-setup-dev-early.service",
        "../systemd-tmpfiles-setup-dev-early.service",
    ),
    (
        "sysinit.target.wants/systemd-tmpfiles-setup-dev.service",
        "../systemd-tmpfiles-setup-dev.service",
    ),
    (
        "sysinit.target.wants/systemd-udevd.service",
        "../systemd-udevd.service",
    ),
    (
        "sysinit.target.wants/systemd-udev-trigger.service",
        "../systemd-udev-trigger.service",
    ),
    // sockets.target.wants - sockets needed during boot
    (
        "sockets.target.wants/systemd-journald-dev-log.socket",
        "../systemd-journald-dev-log.socket",
    ),
    (
        "sockets.target.wants/systemd-journald.socket",
        "../systemd-journald.socket",
    ),
    (
        "sockets.target.wants/systemd-udevd-control.socket",
        "../systemd-udevd-control.socket",
    ),
    (
        "sockets.target.wants/systemd-udevd-kernel.socket",
        "../systemd-udevd-kernel.socket",
    ),
    // initrd.target.wants - initramfs-specific services
    (
        "initrd.target.wants/initrd-parse-etc.service",
        "../initrd-parse-etc.service",
    ),
    (
        "initrd.target.wants/initrd-udevadm-cleanup-db.service",
        "../initrd-udevadm-cleanup-db.service",
    ),
    // initrd-switch-root.target.wants - switch_root services
    (
        "initrd-switch-root.target.wants/initrd-cleanup.service",
        "../initrd-cleanup.service",
    ),
];

/// Copy .wants directory symlinks to enable services.
fn copy_wants_symlinks(rootfs_staging: &Path, initramfs_root: &Path, verbose: bool) -> Result<()> {
    if verbose {
        println!("  Enabling initrd services...");
    }

    let unit_dir = initramfs_root.join("usr/lib/systemd/system");
    let mut created = 0;

    for (link_path, target) in INITRD_WANTS_SYMLINKS {
        let full_link_path = unit_dir.join(link_path);

        // Ensure parent .wants directory exists
        if let Some(parent) = full_link_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Check if the target unit exists in rootfs (skip if not available)
        let target_name = link_path.rsplit('/').next().unwrap_or(link_path);
        let target_unit = rootfs_staging
            .join("usr/lib/systemd/system")
            .join(target_name);

        if !target_unit.exists() {
            continue; // Skip units that don't exist in source
        }

        // Copy the target unit if not already copied
        let dst_unit = unit_dir.join(target_name);
        if !dst_unit.exists() {
            fs::copy(&target_unit, &dst_unit)?;
        }

        // Create the symlink
        if !full_link_path.exists() {
            std::os::unix::fs::symlink(target, &full_link_path)?;
            created += 1;
        }
    }

    // Post-process udev unit files to remove conditions that fail in initramfs
    patch_udev_units(&unit_dir, verbose)?;

    if verbose {
        println!("    Enabled {} services via .wants symlinks", created);
    }

    Ok(())
}

/// Patch udev unit files to remove conditions that fail in initramfs.
///
/// The upstream udev unit files have `ConditionPathIsReadWrite=/sys` which
/// fails during initramfs boot even though sysfs is properly mounted. This
/// prevents udevd from starting, which breaks device detection.
///
/// Files patched:
/// - systemd-udevd-control.socket
/// - systemd-udevd-kernel.socket
/// - systemd-udevd.service
/// - systemd-udev-trigger.service
fn patch_udev_units(unit_dir: &Path, verbose: bool) -> Result<()> {
    let udev_units = [
        "systemd-udevd-control.socket",
        "systemd-udevd-kernel.socket",
        "systemd-udevd.service",
        "systemd-udev-trigger.service",
    ];

    for unit_name in &udev_units {
        let unit_path = unit_dir.join(unit_name);
        if unit_path.exists() {
            let content = fs::read_to_string(&unit_path)?;

            // Remove the ConditionPathIsReadWrite=/sys line specifically
            // We keep other ConditionPathIsReadWrite lines (like ConditionPathIsReadWrite=!/
            // in systemd-fsck-root.service) since they serve valid purposes
            let patched: String = content
                .lines()
                .filter(|line| line.trim() != "ConditionPathIsReadWrite=/sys")
                .collect::<Vec<_>>()
                .join("\n");

            // Only write if we actually changed something
            if patched != content {
                fs::write(&unit_path, patched + "\n")?;
                if verbose {
                    println!(
                        "    Patched {} to remove ConditionPathIsReadWrite=/sys",
                        unit_name
                    );
                }
            }
        }
    }

    Ok(())
}

/// Copy firmware directory (for hardware support).
pub fn copy_firmware(rootfs_staging: &Path, initramfs_root: &Path, verbose: bool) -> Result<()> {
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

/// Udev helper programs needed for device identification.
/// These are invoked by udev rules to probe device attributes.
const UDEV_HELPERS: &[&str] = &[
    "ata_id",    // ATA device identification
    "scsi_id",   // SCSI device identification
    "cdrom_id",  // CD/DVD detection
    "mtd_probe", // MTD probe
    "v4l_id",    // Video4Linux identification
];

/// Copy udev helper programs and their dependencies.
fn copy_udev_helpers(rootfs_staging: &Path, initramfs_root: &Path, verbose: bool) -> Result<()> {
    if verbose {
        println!("  Copying udev helper programs...");
    }

    let src_dir = rootfs_staging.join("usr/lib/udev");
    let dst_dir = initramfs_root.join("usr/lib/udev");
    fs::create_dir_all(&dst_dir)?;

    let extra_lib_paths: &[&str] = &[];
    let mut copied = 0;

    for helper in UDEV_HELPERS {
        let src = src_dir.join(helper);
        let dst = dst_dir.join(helper);

        if src.exists() {
            fs::copy(&src, &dst)?;

            // Make executable
            let mut perms = fs::metadata(&dst)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dst, perms)?;

            // Copy library dependencies
            let deps = get_all_dependencies(rootfs_staging, &src, extra_lib_paths)?;
            for lib_name in deps {
                copy_library_to(
                    rootfs_staging,
                    &lib_name,
                    initramfs_root,
                    "usr/lib64",
                    "usr/lib",
                    extra_lib_paths,
                    &[],
                )?;
            }

            copied += 1;
        }
    }

    if verbose {
        println!("  Copied {} udev helpers", copied);
    }

    Ok(())
}
