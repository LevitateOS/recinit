//! Rootless initramfs builder for Linux.
//!
//! `recinit` builds initramfs images without requiring root privileges or
//! external tools like dracut. It provides two types of initramfs:
//!
//! - **Tiny/Live initramfs**: Busybox-based, mounts EROFS rootfs for live ISOs
//! - **Install initramfs**: Systemd-based, boots from installed disk
//!
//! # Example
//!
//! ```rust,ignore
//! use recinit::{InitramfsBuilder, TinyConfig, ModulePreset};
//!
//! let builder = InitramfsBuilder::new();
//! builder.build_tiny(TinyConfig {
//!     modules_dir: PathBuf::from("/usr/lib/modules/6.12.0"),
//!     busybox_path: PathBuf::from("/path/to/busybox"),
//!     template_path: PathBuf::from("templates/init_tiny.template"),
//!     output: PathBuf::from("initramfs.cpio.gz"),
//!     iso_label: "MYLINUX".to_string(),
//!     rootfs_path: "live/filesystem.erofs".to_string(),
//!     module_preset: ModulePreset::Live,
//! })?;
//! ```

mod busybox;
mod cpio;
mod elf;
mod install;
mod modules;
mod systemd;
mod tiny;

pub use busybox::{
    download_and_cache_busybox, download_busybox, setup_busybox, verify_sha256, BUSYBOX_COMMANDS,
    BUSYBOX_SHA256, BUSYBOX_URL, BUSYBOX_URL_ENV,
};
pub use cpio::build_cpio;
pub use elf::{
    copy_dir_recursive, copy_library_to, find_binary, find_library, get_all_dependencies,
    get_library_dependencies, make_executable,
};
pub use install::{build_install_initramfs, InstallConfig};
pub use modules::{find_kernel_modules_dir, ModulePreset, INSTALL_MODULES, LIVE_MODULES};
pub use tiny::{build_tiny_initramfs, TinyConfig};

use anyhow::Result;
use std::path::Path;

/// Main builder for creating initramfs images.
///
/// Provides a unified interface for building both tiny (live) and
/// install (full systemd) initramfs images.
pub struct InitramfsBuilder {
    /// Whether to print progress messages
    pub verbose: bool,
}

impl Default for InitramfsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl InitramfsBuilder {
    /// Create a new InitramfsBuilder with default settings.
    pub fn new() -> Self {
        Self { verbose: true }
    }

    /// Set verbosity level.
    pub fn verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Build a tiny initramfs for live ISO boot.
    ///
    /// This creates a small (~5MB) busybox-based initramfs that:
    /// 1. Loads kernel modules for CDROM/storage access
    /// 2. Finds and mounts the EROFS root filesystem
    /// 3. Creates an overlay for writable storage
    /// 4. switch_root to the live system
    pub fn build_tiny(&self, config: TinyConfig) -> Result<()> {
        build_tiny_initramfs(&config, self.verbose)
    }

    /// Build an install initramfs for booting from disk.
    ///
    /// This creates a larger (~30-50MB) systemd-based initramfs that:
    /// 1. Loads all common storage drivers (NVMe, SATA, USB)
    /// 2. Includes systemd for service management
    /// 3. Mounts the root filesystem from disk
    /// 4. Hands off to systemd
    pub fn build_install(&self, config: InstallConfig) -> Result<()> {
        build_install_initramfs(&config, self.verbose)
    }
}

/// Default compression level for CPIO archives (gzip).
pub const DEFAULT_GZIP_LEVEL: u32 = 9;

/// Verify an initramfs file looks valid.
pub fn verify_initramfs(path: &Path) -> Result<bool> {
    use std::fs;

    if !path.exists() {
        return Ok(false);
    }

    let metadata = fs::metadata(path)?;

    // Must be at least 1KB to be a valid initramfs
    if metadata.len() < 1024 {
        return Ok(false);
    }

    // Check for gzip magic bytes
    let mut file = fs::File::open(path)?;
    let mut magic = [0u8; 2];
    use std::io::Read;
    file.read_exact(&mut magic)?;

    // Gzip magic: 0x1f 0x8b
    Ok(magic == [0x1f, 0x8b])
}
