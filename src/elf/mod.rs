//! ELF dependency analysis and library copying for initramfs payloads.
//!
//! Uses `readelf -d` instead of `ldd`: readelf reads the ELF headers without
//! running the binary, so it works for a target libc the host doesn't have.

mod analyze;
mod copy;
mod paths;

pub(crate) use analyze::get_all_dependencies;
pub(crate) use copy::{copy_dir_recursive, copy_library_to};
