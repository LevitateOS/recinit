//! The busybox initramfs that boots an installed system, for either distro:
//! busybox, the storage modules, root's fsck and blkid taken from the rootfs
//! with their libraries (found through each binary's own ELF interpreter, so
//! any libc works), and an init script rendered from
//! `templates/init_installed.template`. It knows nothing of the installed
//! system's init.

use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::busybox::setup_busybox;
use crate::cpio::build_archive;
use crate::elf::{copy_preserving, find_library, get_all_dependencies, interpreter, resolve_in};
use crate::modules::{install_boot_modules, INSTALL_MODULES};
use crate::tiny::write_init;

#[derive(Debug, Clone)]
pub struct InstallConfig {
    /// The installed system's rootfs tree (fsck and blkid come from it).
    pub rootfs: PathBuf,

    /// Static busybox binary
    pub busybox_path: PathBuf,

    /// The kernel's module tree, `<rootfs>/usr/lib/modules/<release>`
    pub modules_dir: PathBuf,

    pub output: PathBuf,

    /// Gzip compression level (1-9)
    pub gzip_level: u32,
}

/// The installed init template shipped with recinit.
pub const INSTALLED_TEMPLATE: &str = include_str!("../templates/init_installed.template");

/// Where the archive keeps the kernel modules (merged-usr layout).
const MODULES_DIR: &str = "usr/lib/modules";

/// Directories the archive holds (the live protocol's mount points that the
/// installed init uses, under the merged-usr layout).
pub const INSTALL_DIRS: &[&str] = &[
    "dev",
    "proc",
    "sys",
    "tmp",
    "newroot",
    "usr/bin",
    "usr/sbin",
    "usr/lib",
    "usr/lib64",
];

/// Programs the init runs from the rootfs, installed at `usr/sbin/<name>`
/// with their libraries. `blkid` must be util-linux's: busybox's cannot
/// read `PARTUUID`.
pub const ROOTFS_TOOLS: &[&str] = &["blkid", "e2fsck"];

/// Busybox applets the installed init runs.
pub const INSTALL_INIT_APPLETS: &[&str] = &[
    "sh",
    "mount",
    "insmod",
    "cat",
    "echo",
    "head",
    "tr",
    "sleep",
    "sync",
    "reboot",
    "chroot",
    "switch_root",
    "mkdir",
    "cp",
    "mv",
    "rm",
    "chmod",
    "blockdev",
    "readlink",
    "ls",
    "uname",
    "printf",
    "wc",
];

pub fn build_install_initramfs(config: &InstallConfig, verbose: bool) -> Result<()> {
    if verbose {
        println!("=== Building Install Initramfs ===\n");
    }
    if !config.rootfs.is_dir() {
        bail!("rootfs directory not found at {}", config.rootfs.display());
    }

    build_archive(&config.output, config.gzip_level, |root| {
        for dir in INSTALL_DIRS {
            fs::create_dir_all(root.join(dir))?;
        }
        for link in ["bin", "sbin", "lib"] {
            std::os::unix::fs::symlink(format!("usr/{link}"), root.join(link))?;
        }
        // Where an x86_64 glibc loader and libraries live.
        std::os::unix::fs::symlink("usr/lib64", root.join("lib64"))?;

        for tool in ROOTFS_TOOLS {
            install_tool(&config.rootfs, tool, root)?;
        }
        // Last, so busybox only fills in what the rootfs tools do not provide.
        let applets = setup_busybox(&config.busybox_path, root)?;
        let missing: Vec<&str> = INSTALL_INIT_APPLETS
            .iter()
            .copied()
            .filter(|a| !applets.iter().any(|have| have == a))
            .collect();
        if !missing.is_empty() {
            bail!(
                "{} lacks applets the installed init runs: {}",
                config.busybox_path.display(),
                missing.join(", ")
            );
        }

        let mut vars = install_boot_modules(
            &config.modules_dir,
            INSTALL_MODULES,
            root,
            MODULES_DIR,
            verbose,
        )?;
        vars.extend([
            ("MODULES_DIR".to_string(), format!("/{MODULES_DIR}")),
            (
                "BOOT_BANNER".to_string(),
                "installed system initramfs starting...".to_string(),
            ),
            (
                "IMMUTABLE_OPTION".to_string(),
                crate::installed::IMMUTABLE_OPTION.to_string(),
            ),
            (
                "IMMUTABLE_CONFIG".to_string(),
                crate::installed::IMMUTABLE_CONFIG.to_string(),
            ),
            (
                "BOOT_RECORD".to_string(),
                crate::installed::BOOT_RECORD.to_string(),
            ),
            (
                "SLOT_STATE_DIR".to_string(),
                crate::installed::SLOT_STATE_DIR.to_string(),
            ),
            (
                "IMMUTABLE_INIT_MARKER".to_string(),
                crate::installed::IMMUTABLE_INIT_MARKER.to_string(),
            ),
        ]);
        write_init(INSTALLED_TEMPLATE, vars, root)
    })?;

    if verbose {
        let size = fs::metadata(&config.output)?.len();
        println!("\n=== Install Initramfs Complete ===");
        println!("  Output: {}", config.output.display());
        println!("  Size: {} KB", size / 1024);
    }

    Ok(())
}

/// Copy `name` from the rootfs to `usr/sbin/<name>` with its ELF interpreter
/// and every library it needs, each at the path it has in the rootfs.
fn install_tool(rootfs: &Path, name: &str, root: &Path) -> Result<()> {
    let rel = ["usr/sbin", "usr/bin", "sbin", "bin"]
        .iter()
        .map(|dir| Path::new(dir).join(name))
        .find(|rel| rootfs.join(rel).is_symlink() || rootfs.join(rel).exists())
        .with_context(|| {
            format!(
                "rootfs {} has no {name}, which the installed initramfs needs",
                rootfs.display()
            )
        })?;
    let file = resolve_in(rootfs, &rel)?;
    if file.file_name().and_then(|n| n.to_str()) == Some("busybox") {
        bail!("rootfs {name} is a busybox applet; the installed initramfs needs the real {name} (util-linux / e2fsprogs)");
    }
    let src = rootfs.join(&file);
    let dst = root.join("usr/sbin").join(name);
    fs::copy(&src, &dst).with_context(|| format!("copying {}", src.display()))?;
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o755))?;

    if let Some(loader) = interpreter(&src)? {
        copy_preserving(rootfs, Path::new(loader.trim_start_matches('/')), root)?;
    }
    for lib in get_all_dependencies(rootfs, &src, &[])? {
        let found = find_library(rootfs, &lib, &[]).with_context(|| {
            format!(
                "{name} needs {lib}, which rootfs {} does not have",
                rootfs.display()
            )
        })?;
        copy_preserving(rootfs, found.strip_prefix(rootfs)?, root)?;
    }
    Ok(())
}
