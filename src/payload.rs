//! What the install (systemd) initramfs contains, declared once.
//!
//! The builder copies exactly this from the rootfs, and anything listed that
//! the rootfs lacks fails the build. fsdbg checks built images against these
//! same tables.

use anyhow::{bail, Context, Result};
use leviso_elf::{copy_library_to, get_all_dependencies};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// systemd itself; `/init` links here.
pub const INIT: &str = "usr/lib/systemd/systemd";

/// Programs, copied with every shared library they need.
pub const EXECUTABLES: &[&str] = &[
    INIT,
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
    "usr/lib/systemd/systemd-sysroot-fstab-check",
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
    // Generators: parse root= and fstab before switch-root.
    "usr/lib/systemd/system-generators/systemd-fstab-generator",
    "usr/lib/systemd/system-generators/systemd-gpt-auto-generator",
    "usr/lib/systemd/system-generators/systemd-debug-generator",
    // udev helpers that rules call to identify devices.
    "usr/lib/udev/ata_id",
    "usr/lib/udev/scsi_id",
    "usr/lib/udev/cdrom_id",
    "usr/lib/udev/mtd_probe",
    "usr/lib/udev/v4l_id",
];

/// Where unit files live.
pub const UNIT_DIR: &str = "usr/lib/systemd/system";

/// Units copied into `UNIT_DIR`.
pub const UNITS: &[&str] = &[
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
    "systemd-journald.socket",
    "systemd-journald-dev-log.socket",
    "systemd-udevd-control.socket",
    "systemd-udevd-kernel.socket",
    "dev-hugepages.mount",
    "dev-mqueue.mount",
    "kmod-static-nodes.service",
    "sys-kernel-config.mount",
    "sys-kernel-debug.mount",
    "sys-kernel-tracing.mount",
    "systemd-ask-password-console.path",
    "systemd-tmpfiles-setup-dev-early.service",
    "systemd-tmpfiles-setup-dev.service",
];

/// `default.target` links to this unit.
pub const DEFAULT_TARGET: &str = "initrd.target";

/// Units enabled as `<target>.wants/<unit>` → `../<unit>`.
/// `initrd-udevadm-cleanup-db.service` is deliberately not enabled in
/// `initrd.target.wants`: its `Conflicts=` with the udev sockets breaks boot
/// (TEAM_144).
pub const WANTS: &[(&str, &str)] = &[
    ("sysinit.target", "dev-hugepages.mount"),
    ("sysinit.target", "dev-mqueue.mount"),
    ("sysinit.target", "kmod-static-nodes.service"),
    ("sysinit.target", "sys-kernel-config.mount"),
    ("sysinit.target", "sys-kernel-debug.mount"),
    ("sysinit.target", "sys-kernel-tracing.mount"),
    ("sysinit.target", "systemd-ask-password-console.path"),
    ("sysinit.target", "systemd-modules-load.service"),
    ("sysinit.target", "systemd-sysctl.service"),
    ("sysinit.target", "systemd-tmpfiles-setup-dev-early.service"),
    ("sysinit.target", "systemd-tmpfiles-setup-dev.service"),
    ("sysinit.target", "systemd-udevd.service"),
    ("sysinit.target", "systemd-udev-trigger.service"),
    ("sockets.target", "systemd-journald-dev-log.socket"),
    ("sockets.target", "systemd-journald.socket"),
    ("sockets.target", "systemd-udevd-control.socket"),
    ("sockets.target", "systemd-udevd-kernel.socket"),
    ("initrd.target", "initrd-parse-etc.service"),
    ("initrd-switch-root.target", "initrd-cleanup.service"),
];

/// Lines removed from units: udev's `ConditionPathIsReadWrite=/sys` is false
/// in the initramfs although sysfs is mounted, which keeps udevd from starting.
pub const UNIT_PATCHES: &[(&str, &str)] = &[
    ("systemd-udevd-control.socket", "ConditionPathIsReadWrite=/sys"),
    ("systemd-udevd-kernel.socket", "ConditionPathIsReadWrite=/sys"),
    ("systemd-udevd.service", "ConditionPathIsReadWrite=/sys"),
    ("systemd-udev-trigger.service", "ConditionPathIsReadWrite=/sys"),
];

/// tmpfiles configs needed to create device nodes in the initramfs.
pub const TMPFILES: &[&str] = &[
    "usr/lib/tmpfiles.d/static-nodes-permissions.conf",
    "usr/lib/tmpfiles.d/systemd.conf",
    "usr/lib/tmpfiles.d/tmp.conf",
    "usr/lib/tmpfiles.d/var.conf",
];

/// Directory trees copied whole.
pub const TREES: &[&str] = &["usr/lib/udev/rules.d"];

/// Merged-usr links.
pub const USR_LINKS: &[(&str, &str)] = &[
    ("bin", "usr/bin"),
    ("sbin", "usr/sbin"),
    ("lib", "usr/lib"),
    ("lib64", "usr/lib64"),
];

/// Account and name-service files the initramfs needs: udev rules and
/// systemd services look up these users and groups.
pub const ETC_FILES: &[(&str, &str)] = &[
    (
        "etc/passwd",
        "root:x:0:0:root:/root:/bin/sh\nnobody:x:65534:65534:Nobody:/:/sbin/nologin\n",
    ),
    (
        "etc/group",
        "root:x:0:\ntty:x:5:\ndisk:x:6:\nwheel:x:10:\nnobody:x:65534:\n",
    ),
    ("etc/shadow", "root:*:19000:0:99999:7:::\n"),
    (
        "etc/nsswitch.conf",
        "passwd:     files\ngroup:      files\nshadow:     files\nhosts:      files dns\n",
    ),
];

/// systemd recognises an initrd by this file; it is the rootfs's own os-release.
pub const INITRD_RELEASE: &str = "etc/initrd-release";
const OS_RELEASE: &str = "usr/lib/os-release";

/// Library directories that are private to a package (RUNPATH-only); their
/// libraries are also made visible in `usr/lib64`.
const PRIVATE_LIB_DIRS: &[&str] = &["usr/lib64/systemd"];

/// The dynamic loaders, which no library list names.
const LOADERS: &[&str] = &["usr/lib64/ld-linux-x86-64.so.2"];

fn require(rootfs: &Path, rel: &str) -> Result<std::path::PathBuf> {
    let path = rootfs.join(rel);
    if !path.exists() {
        bail!("rootfs {} has no {rel}, which the install initramfs needs", rootfs.display());
    }
    Ok(path)
}

fn copy_file(src: &Path, dst: &Path, mode: Option<u32>) -> Result<()> {
    fs::create_dir_all(dst.parent().unwrap())?;
    fs::copy(src, dst).with_context(|| format!("copying {}", src.display()))?;
    if let Some(mode) = mode {
        fs::set_permissions(dst, fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

/// Copy the whole declared payload from `rootfs` into `root`.
pub fn install(rootfs: &Path, root: &Path) -> Result<()> {
    for exe in EXECUTABLES {
        let src = require(rootfs, exe)?;
        copy_file(&src, &root.join(exe), Some(0o755))?;
        for lib in get_all_dependencies(rootfs, &src, PRIVATE_LIB_DIRS)? {
            copy_library_to(rootfs, &lib, root, "usr/lib64", "usr/lib", PRIVATE_LIB_DIRS, &["systemd"])?;
        }
    }
    for loader in LOADERS {
        let dst = root.join(loader);
        if !dst.exists() {
            copy_file(&require(rootfs, loader)?, &dst, None)?;
        }
    }
    for dir in PRIVATE_LIB_DIRS {
        let private = root.join(dir);
        if !private.is_dir() {
            continue;
        }
        let relative = Path::new(dir).strip_prefix("usr/lib64").unwrap_or(Path::new(dir));
        for entry in fs::read_dir(&private)? {
            let name = entry?.file_name();
            let link = root.join("usr/lib64").join(&name);
            if !link.exists() {
                std::os::unix::fs::symlink(relative.join(&name), &link)?;
            }
        }
    }

    let unit_dir = root.join(UNIT_DIR);
    for unit in UNITS {
        copy_file(&require(rootfs, &format!("{UNIT_DIR}/{unit}"))?, &unit_dir.join(unit), None)?;
    }
    for (unit, line) in UNIT_PATCHES {
        let path = unit_dir.join(unit);
        let content = fs::read_to_string(&path)?;
        let kept: Vec<&str> = content.lines().filter(|l| l.trim() != *line).collect();
        fs::write(&path, kept.join("\n") + "\n")?;
    }
    std::os::unix::fs::symlink(DEFAULT_TARGET, unit_dir.join("default.target"))?;
    for (target, unit) in WANTS {
        let wants = unit_dir.join(format!("{target}.wants"));
        fs::create_dir_all(&wants)?;
        std::os::unix::fs::symlink(format!("../{unit}"), wants.join(unit))?;
    }

    for file in TMPFILES {
        copy_file(&require(rootfs, file)?, &root.join(file), None)?;
    }
    for tree in TREES {
        leviso_elf::copy_dir_recursive(&require(rootfs, tree)?, &root.join(tree))?;
    }

    let etc = root.join("etc");
    fs::create_dir_all(&etc)?;
    for (path, content) in ETC_FILES {
        fs::write(root.join(path), content)?;
    }
    fs::copy(require(rootfs, OS_RELEASE)?, root.join(INITRD_RELEASE))?;
    std::os::unix::fs::symlink(format!("/{INIT}"), root.join("init"))?;
    Ok(())
}

/// Copy firmware (optional hardware support).
pub fn copy_firmware(rootfs: &Path, root: &Path) -> Result<u64> {
    let src = rootfs.join("usr/lib/firmware");
    if !src.exists() {
        return Ok(0);
    }
    leviso_elf::copy_dir_recursive(&src, &root.join("usr/lib/firmware"))
}
