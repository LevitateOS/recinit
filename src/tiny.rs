//! The busybox-based live initramfs: busybox, the boot modules, and an init
//! script rendered from a template (see `templates/init_tiny.template` for
//! what it does at boot).

use anyhow::{bail, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::busybox::setup_busybox;
use crate::cpio::build_archive;
use crate::modules::{install_boot_modules, LIVE_MODULES};

#[derive(Debug, Clone)]
pub struct TinyConfig {
    /// Path to kernel modules directory (e.g., /usr/lib/modules/6.12.0)
    pub modules_dir: PathBuf,

    /// Static busybox binary
    pub busybox_path: PathBuf,

    pub output: PathBuf,

    /// The boot medium's volume label, which the init finds it by (a
    /// `root=LABEL=` on the command line overrides it)
    pub iso_label: String,

    /// The line the init prints first.
    pub boot_banner: String,

    /// Gzip compression level (1-9)
    pub gzip_level: u32,
}

/// The live initramfs's directories (the live protocol's).
pub const INITRAMFS_DIRS: &[&str] = crate::live::INIT_DIRS;

/// Busybox applets the init template runs; a busybox without one would
/// build an initramfs that cannot boot.
pub const LIVE_INIT_APPLETS: &[&str] = &[
    "sh", "mount", "umount", "insmod", "findfs", "blkid", "mkdir", "touch", "ln", "cat", "chmod",
    "echo", "sed", "tr", "sleep", "chroot", "switch_root",
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
        let applets = setup_busybox(&config.busybox_path, root)?;
        let missing: Vec<&str> =
            LIVE_INIT_APPLETS.iter().copied().filter(|a| !applets.iter().any(|have| have == a)).collect();
        if !missing.is_empty() {
            bail!(
                "{} lacks applets the live init runs: {}",
                config.busybox_path.display(),
                missing.join(", ")
            );
        }
        let module_vars = install_boot_modules(&config.modules_dir, LIVE_MODULES, root, "lib/modules", verbose)?;
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

/// The live init template shipped with recinit.
pub const DEFAULT_TEMPLATE: &str = include_str!("../templates/init_tiny.template");

/// Render the live init script.
fn create_init_script(
    config: &TinyConfig,
    module_vars: &[(String, String)],
    initramfs_root: &Path,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!("Creating init script from template...");
    }
    let mut vars = vec![
        ("ISO_LABEL".to_string(), config.iso_label.clone()),
        ("BOOT_BANNER".to_string(), config.boot_banner.clone()),
    ];
    vars.extend(module_vars.iter().cloned());
    write_init(DEFAULT_TEMPLATE, vars, initramfs_root)
}

/// Fill a template from the live protocol's values plus `extra`, and write it
/// as the executable `/init`. Every placeholder must be filled, so template
/// and filler cannot drift apart.
pub(crate) fn write_init(template: &str, extra: Vec<(String, String)>, initramfs_root: &Path) -> Result<()> {
    let mut vars: Vec<(String, String)> = crate::live::init_template_vars()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    vars.extend(extra);

    let mut init_content = template.to_string();
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
    fs::set_permissions(&init_dst, fs::Permissions::from_mode(0o755))?;
    Ok(())
}
