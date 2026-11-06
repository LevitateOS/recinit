//! Rootless initramfs builder for Linux.
//!
//! Builds a busybox-based live initramfs (`build_tiny_initramfs`) and a
//! systemd-based install initramfs (`build_install_initramfs`) without root
//! privileges or external tools like dracut.

mod busybox;
mod cpio;
mod install;
pub mod live;
mod modules;
pub mod payload;
mod tiny;

pub use install::{build_install_initramfs, InstallConfig, MOUNT_DIRS};
pub use modules::{ModulePreset, INSTALL_MODULES, LIVE_MODULES};
pub use tiny::{build_tiny_initramfs, default_boot_devices, TinyConfig, INITRAMFS_DIRS};

/// Default gzip level for initramfs archives.
pub const DEFAULT_GZIP_LEVEL: u32 = 9;
