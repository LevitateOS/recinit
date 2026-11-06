//! The systemd-based initramfs that boots an installed system: kernel
//! modules, firmware and the systemd payload taken from the rootfs, with
//! busybox as the emergency shell.

use anyhow::{bail, Result};
use std::fs;
use std::path::PathBuf;

use crate::busybox::setup_busybox;
use crate::cpio::build_archive;
use crate::modules::{KernelModules, ModulePreset};
use crate::payload;

#[derive(Debug, Clone)]
pub struct InstallConfig {
    /// Path to rootfs staging directory (contains systemd, firmware)
    pub rootfs: PathBuf,

    /// Path to static busybox binary (emergency shell)
    pub busybox_path: PathBuf,

    /// The kernel's module tree, `/lib/modules/<release>`
    pub modules_dir: PathBuf,

    pub output: PathBuf,

    pub module_preset: ModulePreset,

    /// Gzip compression level (1-9)
    pub gzip_level: u32,

    pub include_firmware: bool,
}

/// Mount points and runtime directories the payload does not create itself.
/// `sysroot` is where systemd mounts the root before switch-root.
pub const MOUNT_DIRS: &[&str] = &["dev", "proc", "sys", "run", "tmp", "var/run", "sysroot"];

pub fn build_install_initramfs(config: &InstallConfig, verbose: bool) -> Result<()> {
    if verbose {
        println!("=== Building Install Initramfs ===\n");
    }
    if !config.rootfs.exists() {
        bail!("rootfs directory not found at {}", config.rootfs.display());
    }

    let kernel = KernelModules::open(&config.modules_dir)?;
    let resolved = kernel.resolve(&config.module_preset.names())?;
    if verbose {
        println!("  Kernel release: {}", kernel.release());
    }

    build_archive(&config.output, config.gzip_level, |root| {
        for (link, target) in payload::USR_LINKS {
            fs::create_dir_all(root.join(target))?;
            std::os::unix::fs::symlink(target, root.join(link))?;
        }
        for dir in MOUNT_DIRS {
            fs::create_dir_all(root.join(dir))?;
        }

        let installed = kernel.install(&resolved, root, "usr/lib/modules", false)?;
        if config.include_firmware {
            payload::copy_firmware(&config.rootfs, root)?;
        }
        payload::install(&config.rootfs, root)?;
        // Last, so busybox only fills in what the payload does not provide
        // (its applets never shadow util-linux mount/umount or bash).
        setup_busybox(&config.busybox_path, root)?;
        if verbose {
            println!(
                "  {} modules installed, {} built into the kernel",
                installed.len(),
                resolved.builtin.len()
            );
        }
        Ok(())
    })?;

    if verbose {
        let size = fs::metadata(&config.output)?.len();
        println!("\n=== Install Initramfs Complete ===");
        println!("  Output: {}", config.output.display());
        println!("  Size: {} MB", size / 1024 / 1024);
    }

    Ok(())
}
