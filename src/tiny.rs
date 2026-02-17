//! Tiny/live initramfs builder.
//!
//! Builds a small (~5MB) busybox-based initramfs for live ISO boot.
//! This initramfs:
//! 1. Loads kernel modules for storage access
//! 2. Finds and mounts the EROFS root filesystem
//! 3. Creates an overlay for writable storage
//! 4. switch_root to the live system

use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::busybox::setup_busybox;
use crate::cpio::build_cpio;
use crate::modules::{copy_kernel_modules, module_name, ModulePreset};
use crate::DEFAULT_GZIP_LEVEL;

/// Configuration for building a tiny/live initramfs.
#[derive(Debug, Clone)]
pub struct TinyConfig {
    /// Path to kernel modules directory (e.g., /usr/lib/modules/6.12.0)
    pub modules_dir: PathBuf,

    /// Path to busybox static binary
    pub busybox_path: PathBuf,

    /// Path to init script template
    pub template_path: PathBuf,

    /// Output path for the initramfs (e.g., initramfs.cpio.gz)
    pub output: PathBuf,

    /// ISO volume label (used for finding boot device)
    pub iso_label: String,

    /// Path to rootfs inside ISO (e.g., "live/filesystem.erofs")
    pub rootfs_path: String,

    /// Path to live overlay payload image inside ISO (e.g., "live/overlayfs.erofs")
    pub live_overlay_image_path: Option<String>,

    /// Legacy alias for live overlay payload image path.
    ///
    /// Kept for compatibility with existing callers that still initialize
    /// `TinyConfig` using `live_overlay_path`.
    pub live_overlay_path: Option<String>,

    /// Boot devices to probe in order
    pub boot_devices: Vec<String>,

    /// Module preset to use
    pub module_preset: ModulePreset,

    /// Gzip compression level (1-9)
    pub gzip_level: u32,

    /// Whether to check for built-in modules
    pub check_builtin: bool,

    /// Extra template variables for custom init scripts.
    /// Each pair is (placeholder, value) where placeholder is WITHOUT braces
    /// (e.g., "ROOT_PARTUUID" will replace `{{ROOT_PARTUUID}}`).
    pub extra_template_vars: Vec<(String, String)>,
}

impl Default for TinyConfig {
    fn default() -> Self {
        Self {
            modules_dir: PathBuf::from("/usr/lib/modules"),
            busybox_path: PathBuf::from("/usr/bin/busybox"),
            template_path: PathBuf::from("templates/init_tiny.template"),
            output: PathBuf::from("initramfs.cpio.gz"),
            iso_label: "LINUX".to_string(),
            rootfs_path: "live/filesystem.erofs".to_string(),
            live_overlay_image_path: Some(distro_spec::shared::LIVE_OVERLAYFS_ISO_PATH.to_string()),
            live_overlay_path: Some(distro_spec::shared::LIVE_OVERLAYFS_ISO_PATH.to_string()),
            boot_devices: default_boot_devices(),
            module_preset: ModulePreset::Live,
            gzip_level: DEFAULT_GZIP_LEVEL,
            check_builtin: true,
            extra_template_vars: Vec::new(),
        }
    }
}

/// Default boot device probe order.
///
/// Uses the shared constant from distro-spec for consistency.
pub fn default_boot_devices() -> Vec<String> {
    distro_spec::shared::BOOT_DEVICE_PROBE_ORDER
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Directories to create in initramfs root.
const INITRAMFS_DIRS: &[&str] = &[
    "bin",
    "dev",
    "proc",
    "sys",
    "tmp",
    "mnt",
    "lib/modules",
    "rootfs",
    "overlay",
    "newroot",
    "live-overlay",
];

/// Build a tiny initramfs for live ISO boot.
pub fn build_tiny_initramfs(config: &TinyConfig, verbose: bool) -> Result<()> {
    if verbose {
        println!("=== Building Tiny Initramfs ===\n");
    }

    // Create temporary build directory
    let output_dir = config.output.parent().unwrap_or(Path::new("."));
    let initramfs_root = output_dir.join("initramfs-build-root");

    // Clean previous build
    if initramfs_root.exists() {
        fs::remove_dir_all(&initramfs_root)?;
    }

    // 1. Create directory structure
    create_directory_structure(&initramfs_root, verbose)?;

    // 2. Set up busybox
    if verbose {
        println!("Setting up busybox...");
    }
    setup_busybox(&config.busybox_path, &initramfs_root, None)?;
    if verbose {
        println!("  Busybox ready");
    }

    // 3. Copy kernel modules
    copy_boot_modules(config, &initramfs_root, verbose)?;

    // 4. Create init script from template
    create_init_script(config, &initramfs_root, verbose)?;

    // 5. Build CPIO archive to temporary file
    if verbose {
        println!("Building cpio archive...");
    }
    let temp_cpio = output_dir.join(format!(
        "{}.tmp",
        config.output.file_name().unwrap().to_string_lossy()
    ));
    build_cpio(&initramfs_root, &temp_cpio, config.gzip_level)?;

    // 6. Verify and rename to final destination
    if !temp_cpio.exists() || fs::metadata(&temp_cpio)?.len() < 1024 {
        bail!("Initramfs build produced invalid or empty file");
    }
    fs::rename(&temp_cpio, &config.output)?;

    // 7. Clean up build directory
    fs::remove_dir_all(&initramfs_root)?;

    if verbose {
        let size = fs::metadata(&config.output)?.len();
        println!("\n=== Tiny Initramfs Complete ===");
        println!("  Output: {}", config.output.display());
        println!("  Size: {} KB", size / 1024);
    }

    Ok(())
}

/// Create the initramfs directory structure.
fn create_directory_structure(root: &Path, verbose: bool) -> Result<()> {
    if verbose {
        println!("Creating directory structure...");
    }

    for dir in INITRAMFS_DIRS {
        fs::create_dir_all(root.join(dir))?;
    }

    // Create a note in /dev explaining devtmpfs
    let dev = root.join("dev");
    fs::write(
        dev.join(".note"),
        "# Device nodes are created by devtmpfs mount in /init\n",
    )?;

    Ok(())
}

/// Copy boot kernel modules to initramfs.
fn copy_boot_modules(config: &TinyConfig, initramfs_root: &Path, verbose: bool) -> Result<()> {
    if verbose {
        println!("Copying boot kernel modules...");
    }

    let modules = config.module_preset.module_paths();

    let (copied, builtin, missing) = copy_kernel_modules(
        &config.modules_dir,
        initramfs_root,
        &modules,
        config.check_builtin,
    )?;

    // For live initramfs, missing modules are fatal
    if !missing.is_empty() {
        bail!(
            "Boot modules missing: {:?}\n\
             \n\
             These kernel modules are REQUIRED for the ISO to boot:\n\
             - cdrom, sr_mod, virtio_scsi, isofs (CDROM access)\n\
             - loop, erofs, overlay (EROFS + overlay boot)\n\
             \n\
             Without ALL of these, the initramfs cannot mount the EROFS rootfs.",
            missing
        );
    }

    if verbose {
        if builtin > 0 {
            println!("  {} boot modules are built-in to kernel", builtin);
        }
        println!("  Copied {} boot modules", copied);
    }

    Ok(())
}

/// Create init script from template.
fn create_init_script(config: &TinyConfig, initramfs_root: &Path, verbose: bool) -> Result<()> {
    if verbose {
        println!("Creating init script from template...");
    }

    let template = fs::read_to_string(&config.template_path).with_context(|| {
        format!(
            "Failed to read init template at {}",
            config.template_path.display()
        )
    })?;

    // Get module names from paths
    let module_names: Vec<&str> = config
        .module_preset
        .module_paths()
        .iter()
        .map(|m| module_name(m))
        .collect();

    // Build the init script from template
    let overlay_image_path = config
        .live_overlay_image_path
        .as_ref()
        .or(config.live_overlay_path.as_ref())
        .map(|p| format!("/{}", p))
        .unwrap_or_default();

    let mut init_content = template
        .replace("{{ISO_LABEL}}", &config.iso_label)
        .replace("{{ROOTFS_PATH}}", &format!("/{}", config.rootfs_path))
        .replace("{{BOOT_MODULES}}", &module_names.join(" "))
        .replace("{{BOOT_DEVICES}}", &config.boot_devices.join(" "))
        .replace("{{LIVE_OVERLAY_IMAGE_PATH}}", &overlay_image_path);

    // Apply extra template variables
    for (key, value) in &config.extra_template_vars {
        init_content = init_content.replace(&format!("{{{{{}}}}}", key), value);
    }

    let init_dst = initramfs_root.join("init");
    fs::write(&init_dst, &init_content)?;

    // Make executable
    let mut perms = fs::metadata(&init_dst)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&init_dst, perms)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = TinyConfig::default();
        assert_eq!(config.gzip_level, DEFAULT_GZIP_LEVEL);
        assert!(!config.boot_devices.is_empty());
    }

    #[test]
    fn test_default_boot_devices() {
        let devices = default_boot_devices();
        assert!(devices.contains(&"/dev/sr0".to_string()));
        assert!(devices.contains(&"/dev/vda".to_string()));
    }
}
