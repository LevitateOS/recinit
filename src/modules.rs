//! Kernel module handling for initramfs.
//!
//! Uses module definitions from `distro-spec` (single source of truth)
//! and provides copying functionality for building initramfs images.

use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

// Import module definitions from distro-spec (SINGLE SOURCE OF TRUTH)
pub use distro_spec::shared::{module_path, INSTALL_MODULES, LIVE_MODULES};

/// Kernel module preset for initramfs building.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ModulePreset {
    /// Minimal modules for live ISO boot (CDROM, EROFS, overlay, virtio)
    #[default]
    Live,
    /// Full modules for installed systems (NVMe, SATA, USB, ext4, xfs, btrfs)
    Install,
    /// Custom module list
    Custom(Vec<String>),
}

impl ModulePreset {
    /// Get the module paths for this preset.
    ///
    /// Returns full kernel paths relative to `/lib/modules/<kernel-version>/`.
    /// Order matters - dependencies must be listed before modules that use them.
    pub fn module_paths(&self) -> Vec<&'static str> {
        match self {
            Self::Live => LIVE_MODULES
                .iter()
                .filter_map(|name| module_path(name))
                .collect(),
            Self::Install => INSTALL_MODULES
                .iter()
                .filter_map(|name| module_path(name))
                .collect(),
            Self::Custom(list) => list.iter().filter_map(|name| module_path(name)).collect(),
        }
    }

    /// Get module names for this preset.
    pub fn module_names(&self) -> &[&str] {
        match self {
            Self::Live => LIVE_MODULES,
            Self::Install => INSTALL_MODULES,
            Self::Custom(_) => &[], // Custom doesn't have static names
        }
    }

    /// Parse a preset from string.
    pub fn parse_name(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "live" | "tiny" => Some(Self::Live),
            "install" | "full" => Some(Self::Install),
            _ => None,
        }
    }
}

/// Copy kernel modules from source to initramfs.
///
/// # Arguments
///
/// * `modules_dir` - Path to kernel modules (e.g., /lib/modules/6.12.0)
/// * `initramfs_root` - Root of the initramfs being built
/// * `module_paths` - List of module paths (relative to kernel version dir)
/// * `builtin_check` - If true, skip modules that are built-in to the kernel
///
/// # Returns
///
/// A tuple of (copied_count, builtin_count, missing_modules)
pub fn copy_kernel_modules(
    modules_dir: &Path,
    initramfs_root: &Path,
    module_paths: &[&str],
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

    let dst_dir = initramfs_root.join("lib/modules").join(kernel_version);
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

    for module_path in module_paths {
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
        for ext in [".ko", ".ko.xz", ".ko.gz", ".ko.zst"] {
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

/// Find the kernel modules directory for a given base path.
///
/// Scans `base_modules_dir` (e.g., `output/staging/lib/modules/`) for the
/// first directory entry, which is the kernel version directory.
pub fn find_kernel_modules_dir(base_modules_dir: &Path) -> Result<std::path::PathBuf> {
    if !base_modules_dir.exists() {
        bail!(
            "Kernel modules directory not found at {}",
            base_modules_dir.display()
        );
    }

    let kver = fs::read_dir(base_modules_dir)?
        .filter_map(|e| e.ok())
        .find(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string());

    match kver {
        Some(ver) => Ok(base_modules_dir.join(ver)),
        None => bail!(
            "No kernel version directory found in {}",
            base_modules_dir.display()
        ),
    }
}

/// Extract module name from full path.
///
/// Example: "kernel/fs/ext4/ext4.ko.xz" -> "ext4"
pub fn module_name(path: &str) -> &str {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".ko.xz")
        .trim_end_matches(".ko.gz")
        .trim_end_matches(".ko.zst")
        .trim_end_matches(".ko")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_name() {
        assert_eq!(module_name("kernel/fs/ext4/ext4.ko.xz"), "ext4");
        assert_eq!(module_name("kernel/drivers/virtio/virtio.ko"), "virtio");
        assert_eq!(module_name("overlay"), "overlay");
    }

    #[test]
    fn test_preset_from_str() {
        assert_eq!(ModulePreset::parse_name("live"), Some(ModulePreset::Live));
        assert_eq!(ModulePreset::parse_name("LIVE"), Some(ModulePreset::Live));
        assert_eq!(ModulePreset::parse_name("tiny"), Some(ModulePreset::Live));
        assert_eq!(
            ModulePreset::parse_name("install"),
            Some(ModulePreset::Install)
        );
        assert_eq!(
            ModulePreset::parse_name("full"),
            Some(ModulePreset::Install)
        );
        assert_eq!(ModulePreset::parse_name("unknown"), None);
    }

    #[test]
    fn test_live_modules_from_distro_spec() {
        // Verify we're using distro-spec constants
        assert!(LIVE_MODULES.contains(&"virtio"));
        assert!(LIVE_MODULES.contains(&"overlay"));
        assert!(LIVE_MODULES.contains(&"loop"));
    }

    #[test]
    fn test_install_modules_from_distro_spec() {
        // Verify we're using distro-spec constants
        assert!(INSTALL_MODULES.contains(&"ext4"));
        assert!(INSTALL_MODULES.contains(&"xfs"));
        assert!(INSTALL_MODULES.contains(&"nvme"));
        assert!(INSTALL_MODULES.contains(&"ahci"));
    }

    #[test]
    fn test_module_paths_generated() {
        let preset = ModulePreset::Live;
        let paths = preset.module_paths();
        // Should have paths for most modules (some may be built-in)
        assert!(!paths.is_empty());
        assert!(paths.iter().any(|p| p.contains("virtio")));
    }
}
