//! The busybox-based live initramfs: busybox, the boot modules, and an init
//! script rendered from a template (see `templates/init_tiny.template` for
//! what it does at boot).

use anyhow::{bail, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::busybox::setup_busybox;
use crate::cpio::build_archive;
use crate::live::{BOOT_DEVICE_PROBE_ORDER, LIVE_OVERLAYFS_ISO_PATH, ROOTFS_ISO_PATH};
use crate::modules::{KernelModules, LIVE_MODULES};

#[derive(Debug, Clone)]
pub struct TinyConfig {
    /// Path to kernel modules directory (e.g., /usr/lib/modules/6.12.0)
    pub modules_dir: PathBuf,

    /// Static busybox binary
    pub busybox_path: PathBuf,

    pub output: PathBuf,

    /// ISO volume label (used for finding boot device)
    pub iso_label: String,

    /// The line the init prints first.
    pub boot_banner: String,

    /// Gzip compression level (1-9)
    pub gzip_level: u32,
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
    let resolved = kernel.resolve(&LIVE_MODULES.iter().map(|m| m.to_string()).collect::<Vec<_>>())?;
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

/// Render the init script. Every placeholder must be filled, so template and
/// filler cannot drift apart.
fn create_init_script(
    config: &TinyConfig,
    module_vars: &[(String, String)],
    initramfs_root: &Path,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!("Creating init script from template...");
    }

    let mut vars: Vec<(String, String)> = crate::live::init_template_vars()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    vars.extend([
        ("ISO_LABEL".to_string(), config.iso_label.clone()),
        ("ROOTFS_PATH".to_string(), format!("/{ROOTFS_ISO_PATH}")),
        ("BOOT_DEVICES".to_string(), BOOT_DEVICE_PROBE_ORDER.join(" ")),
        ("LIVE_OVERLAY_IMAGE_PATH".to_string(), format!("/{LIVE_OVERLAYFS_ISO_PATH}")),
        ("BOOT_BANNER".to_string(), config.boot_banner.clone()),
    ]);
    vars.extend(module_vars.iter().cloned());

    let template = DEFAULT_TEMPLATE.to_string();
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
