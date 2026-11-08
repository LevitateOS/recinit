//! Rootless initramfs builder for Linux.
//!
//! Builds a busybox-based live initramfs (`build_tiny_initramfs`) and a
//! systemd-based install initramfs (`build_install_initramfs`) without root
//! privileges or external tools like dracut. The builder is the default
//! `builder` feature; without it the crate is only the live protocol
//! ([`live`]), which the install tools and the tests follow.

#[cfg(feature = "builder")]
mod busybox;
#[cfg(feature = "builder")]
mod cpio;
#[cfg(feature = "builder")]
mod elf;
#[cfg(feature = "builder")]
mod install;
pub mod live;
#[cfg(feature = "builder")]
mod modules;
#[cfg(feature = "builder")]
pub mod payload;
#[cfg(feature = "builder")]
mod tiny;

#[cfg(feature = "builder")]
pub use install::{build_install_initramfs, InstallConfig, MOUNT_DIRS};
#[cfg(feature = "builder")]
pub use modules::{INSTALL_MODULES, LIVE_MODULES};
#[cfg(feature = "builder")]
pub use tiny::{build_tiny_initramfs, TinyConfig, INITRAMFS_DIRS, LIVE_INIT_APPLETS};

/// Default gzip level for initramfs archives.
pub const DEFAULT_GZIP_LEVEL: u32 = 9;
