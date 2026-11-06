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
use crate::cpio::build_archive;
use crate::modules::{copy_kernel_modules, module_name, ModulePreset};

/// Configuration for building a tiny/live initramfs.
#[derive(Debug, Clone)]
pub struct TinyConfig {
    /// Path to kernel modules directory (e.g., /usr/lib/modules/6.12.0)
    pub modules_dir: PathBuf,

    /// Path to busybox static binary
    pub busybox_path: PathBuf,

    /// Init script template; `None` uses the template built into recinit.
    pub template_path: Option<PathBuf>,

    /// Output path for the initramfs (e.g., initramfs.cpio.gz)
    pub output: PathBuf,

    /// ISO volume label (used for finding boot device)
    pub iso_label: String,

    /// Path to rootfs inside ISO (e.g., "live/filesystem.erofs")
    pub rootfs_path: String,

    /// Path to live overlay payload image inside ISO (e.g., "live/overlayfs.erofs")
    pub live_overlay_image_path: Option<String>,

    /// Boot devices to probe in order
    pub boot_devices: Vec<String>,

    /// Module preset to use
    pub module_preset: ModulePreset,

    /// Gzip compression level (1-9)
    pub gzip_level: u32,

    /// Whether to check for built-in modules
    pub check_builtin: bool,

    /// Template values set by the caller: `BOOT_BANNER` and `PRODUCT_TAG`
    /// override their defaults; any other key must name a placeholder that
    /// only a custom template uses. Keys are given without braces.
    pub extra_template_vars: Vec<(String, String)>,
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

    build_archive(&config.output, config.gzip_level, |root| {
        create_directory_structure(root, verbose)?;
        if verbose {
            println!("Setting up busybox...");
        }
        setup_busybox(&config.busybox_path, root)?;
        copy_boot_modules(config, root, verbose)?;
        create_init_script(config, root, verbose)
    })?;

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

/// The live init template shipped with recinit.
pub const DEFAULT_TEMPLATE: &str = include_str!("../templates/init_tiny.template");

/// Placeholders a caller may override, with their defaults.
const OVERRIDABLE: &[(&str, &str)] = &[
    ("BOOT_BANNER", "initramfs starting..."),
    ("PRODUCT_TAG", "untagged"),
];

/// Create init script from template. Every placeholder must be filled, and
/// every value set must be used, so template and filler cannot drift apart.
fn create_init_script(config: &TinyConfig, initramfs_root: &Path, verbose: bool) -> Result<()> {
    if verbose {
        println!("Creating init script from template...");
    }

    let template = match &config.template_path {
        Some(path) => fs::read_to_string(path)
            .with_context(|| format!("Failed to read init template at {}", path.display()))?,
        None => DEFAULT_TEMPLATE.to_string(),
    };

    let module_names: Vec<&str> = config
        .module_preset
        .module_paths()
        .iter()
        .map(|m| module_name(m))
        .collect();
    let overlay_image_path = config
        .live_overlay_image_path
        .as_ref()
        .map(|p| format!("/{}", p))
        .unwrap_or_default();

    let mut fixed: Vec<(String, String)> = distro_spec::shared::live_boot::init_template_vars()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    fixed.extend([
        ("ISO_LABEL".to_string(), config.iso_label.clone()),
        ("ROOTFS_PATH".to_string(), format!("/{}", config.rootfs_path)),
        ("BOOT_MODULES".to_string(), module_names.join(" ")),
        ("BOOT_DEVICES".to_string(), config.boot_devices.join(" ")),
        ("LIVE_OVERLAY_IMAGE_PATH".to_string(), overlay_image_path),
    ]);
    let mut vars = fixed.clone();
    vars.extend(OVERRIDABLE.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    for (key, value) in &config.extra_template_vars {
        if fixed.iter().any(|(k, _)| k == key) {
            bail!("template value {key} is set by recinit and cannot be overridden");
        }
        if !template.contains(&format!("{{{{{key}}}}}")) {
            bail!("template value {key} is not used by the init template");
        }
        vars.retain(|(k, _)| k != key);
        vars.push((key.clone(), value.clone()));
    }

    let mut init_content = template;
    for (key, value) in &vars {
        init_content = init_content.replace(&format!("{{{{{key}}}}}"), value);
    }
    let unfilled: Vec<&str> = init_content
        .match_indices("{{")
        .filter_map(|(i, _)| {
            let rest = &init_content[i + 2..];
            rest.find("}}").map(|end| &rest[..end])
        })
        .collect();
    if !unfilled.is_empty() {
        bail!("init template has unfilled placeholders: {}", unfilled.join(", "));
    }

    let init_dst = initramfs_root.join("init");
    fs::write(&init_dst, &init_content)?;

    // Make executable
    let mut perms = fs::metadata(&init_dst)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&init_dst, perms)?;

    Ok(())
}
