//! The busybox-based live initramfs: busybox, the boot modules, and an init
//! script rendered from a template (see `templates/init_tiny.template` for
//! what it does at boot).

use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::busybox::setup_busybox;
use crate::cpio::build_archive;
use crate::modules::{KernelModules, ModulePreset};

#[derive(Debug, Clone)]
pub struct TinyConfig {
    /// Path to kernel modules directory (e.g., /usr/lib/modules/6.12.0)
    pub modules_dir: PathBuf,

    /// Static busybox binary
    pub busybox_path: PathBuf,

    /// Init script template; `None` uses the template built into recinit.
    pub template_path: Option<PathBuf>,

    pub output: PathBuf,

    /// ISO volume label (used for finding boot device)
    pub iso_label: String,

    /// Path to rootfs inside ISO (e.g., "live/filesystem.erofs")
    pub rootfs_path: String,

    /// Path to live overlay payload image inside ISO (e.g., "live/overlayfs.erofs")
    pub live_overlay_image_path: Option<String>,

    /// Devices the init script probes, in order, for the boot medium
    pub boot_devices: Vec<String>,

    pub module_preset: ModulePreset,

    /// Gzip compression level (1-9)
    pub gzip_level: u32,

    /// Template values set by the caller: `BOOT_BANNER` and `PRODUCT_TAG`
    /// override their defaults; any other key must name a placeholder that
    /// only a custom template uses. Keys are given without braces.
    pub extra_template_vars: Vec<(String, String)>,
}

/// The live init's boot device probe order.
pub fn default_boot_devices() -> Vec<String> {
    crate::live::BOOT_DEVICE_PROBE_ORDER
        .iter()
        .map(|s| s.to_string())
        .collect()
}

pub const INITRAMFS_DIRS: &[&str] = &[
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
        let module_vars = install_boot_modules(config, root, verbose)?;
        create_init_script(config, &module_vars, root, verbose)
    })?;

    if verbose {
        let size = fs::metadata(&config.output)?.len();
        println!("\n=== Tiny Initramfs Complete ===");
        println!("  Output: {}", config.output.display());
        println!("  Size: {} KB", size / 1024);
    }

    Ok(())
}

fn create_directory_structure(root: &Path, verbose: bool) -> Result<()> {
    if verbose {
        println!("Creating directory structure...");
    }

    for dir in INITRAMFS_DIRS {
        fs::create_dir_all(root.join(dir))?;
    }

    Ok(())
}

/// Install the boot modules (decompressed, for busybox `insmod`) and return
/// the template values that load them in dependency order.
fn install_boot_modules(
    config: &TinyConfig,
    initramfs_root: &Path,
    verbose: bool,
) -> Result<Vec<(String, String)>> {
    let kernel = KernelModules::open(&config.modules_dir)?;
    let resolved = kernel.resolve(&config.module_preset.names())?;
    let paths = kernel.install(&resolved, initramfs_root, "lib/modules", true)?;
    if verbose {
        println!(
            "  {} boot modules installed, {} built into kernel {}",
            paths.len(),
            resolved.builtin.len(),
            kernel.release()
        );
    }
    Ok(vec![
        ("KERNEL_RELEASE".to_string(), kernel.release().to_string()),
        ("BOOT_MODULE_PATHS".to_string(), paths.join(" ")),
    ])
}

/// The live init template shipped with recinit.
pub const DEFAULT_TEMPLATE: &str = include_str!("../templates/init_tiny.template");

/// Placeholders a caller may override, with their defaults.
const OVERRIDABLE: &[(&str, &str)] = &[
    ("BOOT_BANNER", "initramfs starting..."),
    ("PRODUCT_TAG", "untagged"),
];

/// Render the init script. Every placeholder must be filled, and every value
/// the caller sets must be used, so template and filler cannot drift apart.
fn create_init_script(
    config: &TinyConfig,
    module_vars: &[(String, String)],
    initramfs_root: &Path,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!("Creating init script from template...");
    }

    let template = match &config.template_path {
        Some(path) => fs::read_to_string(path)
            .with_context(|| format!("Failed to read init template at {}", path.display()))?,
        None => DEFAULT_TEMPLATE.to_string(),
    };

    let overlay_image_path = config
        .live_overlay_image_path
        .as_ref()
        .map(|p| format!("/{}", p))
        .unwrap_or_default();

    let mut fixed: Vec<(String, String)> = crate::live::init_template_vars()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    fixed.extend([
        ("ISO_LABEL".to_string(), config.iso_label.clone()),
        ("ROOTFS_PATH".to_string(), format!("/{}", config.rootfs_path)),
        ("BOOT_DEVICES".to_string(), config.boot_devices.join(" ")),
        ("LIVE_OVERLAY_IMAGE_PATH".to_string(), overlay_image_path),
    ]);
    fixed.extend(module_vars.iter().cloned());
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

    let mut perms = fs::metadata(&init_dst)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&init_dst, perms)?;

    Ok(())
}
