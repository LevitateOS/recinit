//! Kernel module presets and handling for initramfs.
//!
//! Provides predefined module lists for different boot scenarios:
//! - **Live**: Minimal set for booting from ISO/CD with squashfs/erofs
//! - **Install**: Full set for booting from installed disk
//! - **Custom**: User-provided module list

use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Kernel module preset for initramfs building.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModulePreset {
    /// Minimal modules for live ISO boot (CDROM, squashfs, overlay, virtio)
    Live,
    /// Full modules for installed systems (NVMe, SATA, USB, ext4, xfs, btrfs)
    Install,
    /// Custom module list
    Custom(Vec<String>),
}

impl Default for ModulePreset {
    fn default() -> Self {
        Self::Live
    }
}

impl ModulePreset {
    /// Get the module paths for this preset.
    ///
    /// Paths are relative to `/lib/modules/<kernel-version>/`.
    /// Order matters - dependencies must be listed before modules that use them.
    pub fn modules(&self) -> Vec<&str> {
        match self {
            Self::Live => LIVE_MODULES.to_vec(),
            Self::Install => INSTALL_MODULES.to_vec(),
            Self::Custom(list) => list.iter().map(|s| s.as_str()).collect(),
        }
    }

    /// Parse a preset from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "live" | "tiny" => Some(Self::Live),
            "install" | "full" => Some(Self::Install),
            _ => None,
        }
    }
}

/// Modules required for live ISO boot.
///
/// ORDER MATTERS: Dependencies must be listed before modules that use them.
/// The initramfs uses `insmod` which requires dependencies to be loaded first.
pub const LIVE_MODULES: &[&str] = &[
    // === Virtio core (must be first for QEMU) ===
    "kernel/drivers/virtio/virtio",
    "kernel/drivers/virtio/virtio_ring",
    "kernel/drivers/virtio/virtio_pci",
    // === SCSI core (needed by sr_mod, sd_mod, virtio_scsi) ===
    "kernel/drivers/scsi/scsi_mod",
    // === CDROM/SCSI for ISO mount ===
    "kernel/drivers/cdrom/cdrom",
    "kernel/drivers/scsi/sr_mod",
    "kernel/drivers/scsi/sd_mod",
    "kernel/drivers/scsi/virtio_scsi",
    "kernel/fs/isofs/isofs",
    // === NVMe (modern SSDs) ===
    "kernel/drivers/nvme/host/nvme-core",
    "kernel/drivers/nvme/host/nvme",
    // === SATA/AHCI ===
    "kernel/drivers/ata/libata",
    "kernel/drivers/ata/libahci",
    "kernel/drivers/ata/ahci",
    // === Virtio block (QEMU virtual disks) ===
    "kernel/drivers/block/virtio_blk",
    // === Loop, squashfs, overlay for live boot ===
    "kernel/drivers/block/loop",
    "kernel/fs/squashfs/squashfs",
    "kernel/fs/overlayfs/overlay",
];

/// Modules required for installed system boot.
///
/// This is a superset of LIVE_MODULES with additional filesystem and
/// storage drivers for booting from any hardware configuration.
pub const INSTALL_MODULES: &[&str] = &[
    // === Virtio core (must be first for QEMU) ===
    "kernel/drivers/virtio/virtio",
    "kernel/drivers/virtio/virtio_ring",
    "kernel/drivers/virtio/virtio_pci",
    // === SCSI core (needed by sd_mod, virtio_scsi, usb-storage) ===
    "kernel/drivers/scsi/scsi_mod",
    "kernel/drivers/scsi/sd_mod",
    "kernel/drivers/scsi/virtio_scsi",
    // === NVMe (modern SSDs) ===
    "kernel/drivers/nvme/host/nvme-core",
    "kernel/drivers/nvme/host/nvme",
    // === SATA/AHCI ===
    "kernel/drivers/ata/libata",
    "kernel/drivers/ata/libahci",
    "kernel/drivers/ata/ahci",
    "kernel/drivers/ata/ata_piix",
    // === Virtio block (QEMU virtual disks) ===
    "kernel/drivers/block/virtio_blk",
    // === USB Storage ===
    "kernel/drivers/usb/common/usb-common",
    "kernel/drivers/usb/core/usbcore",
    "kernel/drivers/usb/host/xhci-hcd",
    "kernel/drivers/usb/host/xhci-pci",
    "kernel/drivers/usb/host/ehci-hcd",
    "kernel/drivers/usb/host/ehci-pci",
    "kernel/drivers/usb/storage/usb-storage",
    // === HID (keyboards for LUKS prompts) ===
    "kernel/drivers/hid/hid",
    "kernel/drivers/hid/hid-generic",
    "kernel/drivers/hid/usbhid/usbhid",
    // === Filesystems ===
    "kernel/fs/ext4/ext4",
    "kernel/fs/xfs/xfs",
    "kernel/fs/btrfs/btrfs",
    "kernel/fs/fat/fat",
    "kernel/fs/vfat/vfat",
    "kernel/fs/nls/nls_cp437",
    "kernel/fs/nls/nls_iso8859-1",
    "kernel/fs/nls/nls_utf8",
    // === Device Mapper (for future LUKS/LVM) ===
    "kernel/drivers/md/dm-mod",
    "kernel/drivers/md/dm-crypt",
];

/// Copy kernel modules from source to initramfs.
///
/// # Arguments
///
/// * `modules_dir` - Path to kernel modules (e.g., /lib/modules/6.12.0)
/// * `initramfs_root` - Root of the initramfs being built
/// * `modules` - List of module paths (relative to kernel version dir)
/// * `builtin_check` - If true, skip modules that are built-in to the kernel
///
/// # Returns
///
/// A tuple of (copied_count, builtin_count, missing_modules)
pub fn copy_kernel_modules(
    modules_dir: &Path,
    initramfs_root: &Path,
    modules: &[&str],
    builtin_check: bool,
) -> Result<(usize, usize, Vec<String>)> {
    // Validate source directory
    if !modules_dir.exists() {
        bail!(
            "Kernel modules directory not found: {}",
            modules_dir.display()
        );
    }

    // Find kernel version from directory name
    let kernel_version = modules_dir
        .file_name()
        .and_then(|n| n.to_str())
        .context("Cannot determine kernel version from modules path")?;

    let dst_dir = initramfs_root
        .join("lib/modules")
        .join(kernel_version);
    fs::create_dir_all(&dst_dir)?;

    // Load modules.builtin if checking for built-in modules
    let builtin_modules: HashSet<String> = if builtin_check {
        let builtin_path = modules_dir.join("modules.builtin");
        if builtin_path.exists() {
            fs::read_to_string(&builtin_path)?
                .lines()
                .map(|s| s.to_string())
                .collect()
        } else {
            HashSet::new()
        }
    } else {
        HashSet::new()
    };

    let mut copied = 0;
    let mut builtin_count = 0;
    let mut missing = Vec::new();

    for module_path in modules {
        // Get base path without extension
        let base_path = module_path
            .trim_end_matches(".ko.xz")
            .trim_end_matches(".ko.gz")
            .trim_end_matches(".ko");

        // Check if module is built-in
        let builtin_key = format!("{}.ko", base_path);
        if builtin_check && builtin_modules.contains(&builtin_key) {
            builtin_count += 1;
            continue;
        }

        // Try to find module with different extensions
        let mut found = false;
        for ext in [".ko", ".ko.xz", ".ko.gz"] {
            let src = modules_dir.join(format!("{}{}", base_path, ext));
            if src.exists() {
                let dst = dst_dir.join(format!("{}{}", base_path, ext));
                fs::create_dir_all(dst.parent().unwrap())?;
                fs::copy(&src, &dst)?;
                copied += 1;
                found = true;
                break;
            }
        }

        if !found {
            missing.push((*module_path).to_string());
        }
    }

    // Copy module metadata files
    for entry in fs::read_dir(modules_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("modules.") && entry.path().is_file() {
            fs::copy(entry.path(), dst_dir.join(&name))?;
        }
    }

    Ok((copied, builtin_count, missing))
}

/// Extract module name from full path.
///
/// Example: "kernel/fs/squashfs/squashfs.ko.xz" -> "squashfs"
pub fn module_name(path: &str) -> &str {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".ko.xz")
        .trim_end_matches(".ko.gz")
        .trim_end_matches(".ko")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_name() {
        assert_eq!(module_name("kernel/fs/squashfs/squashfs.ko.xz"), "squashfs");
        assert_eq!(module_name("kernel/drivers/virtio/virtio.ko"), "virtio");
        assert_eq!(module_name("squashfs"), "squashfs");
    }

    #[test]
    fn test_preset_from_str() {
        assert_eq!(ModulePreset::from_str("live"), Some(ModulePreset::Live));
        assert_eq!(ModulePreset::from_str("LIVE"), Some(ModulePreset::Live));
        assert_eq!(ModulePreset::from_str("tiny"), Some(ModulePreset::Live));
        assert_eq!(ModulePreset::from_str("install"), Some(ModulePreset::Install));
        assert_eq!(ModulePreset::from_str("full"), Some(ModulePreset::Install));
        assert_eq!(ModulePreset::from_str("unknown"), None);
    }

    #[test]
    fn test_live_modules_includes_essentials() {
        let modules = LIVE_MODULES;
        // Check that essential modules are present
        assert!(modules.iter().any(|m| m.contains("squashfs")));
        assert!(modules.iter().any(|m| m.contains("overlay")));
        assert!(modules.iter().any(|m| m.contains("loop")));
        assert!(modules.iter().any(|m| m.contains("virtio")));
    }

    #[test]
    fn test_install_modules_superset_of_storage() {
        let modules = INSTALL_MODULES;
        // Install modules should have filesystem drivers
        assert!(modules.iter().any(|m| m.contains("ext4")));
        assert!(modules.iter().any(|m| m.contains("xfs")));
        assert!(modules.iter().any(|m| m.contains("nvme")));
        assert!(modules.iter().any(|m| m.contains("ahci")));
    }
}
