//! ELF binary analysis and library copying utilities.
//!
//! Uses `readelf -d` to extract library dependencies. This works for
//! cross-compilation since readelf reads ELF headers directly without
//! executing the binary (which ldd does via the host dynamic linker).

use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Extract library dependencies from an ELF binary using readelf.
///
/// This is architecture-independent - readelf reads the ELF headers directly
/// without executing the binary, unlike ldd which uses the host dynamic linker.
///
/// # Errors
///
/// Returns an error if:
/// - The file does not exist
/// - `readelf` is not installed (install binutils)
/// - `readelf` fails for reasons other than "not an ELF file"
///
/// Returns `Ok(Vec::new())` if the file is not an ELF binary (e.g., a text file).
pub fn get_library_dependencies(binary_path: &Path) -> Result<Vec<String>> {
    if !binary_path.exists() {
        bail!("File does not exist: {}", binary_path.display());
    }

    let output = Command::new("readelf")
        .args(["-d"])
        .arg(binary_path)
        .output()
        .context("readelf command not found - install binutils")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // These are legitimate "not an ELF" cases, not errors
        if stderr.contains("Not an ELF file")
            || stderr.contains("not a dynamic executable")
            || stderr.contains("File format not recognized")
        {
            return Ok(Vec::new());
        }
        bail!(
            "readelf failed on {}: {}",
            binary_path.display(),
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_readelf_output(&stdout)
}

/// Parse readelf -d output to extract NEEDED library names.
///
/// Example readelf output:
/// ```text
/// Dynamic section at offset 0x2d0e0 contains 28 entries:
///   Tag        Type                         Name/Value
///  0x0000000000000001 (NEEDED)             Shared library: [libtinfo.so.6]
///  0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
/// ```
pub fn parse_readelf_output(output: &str) -> Result<Vec<String>> {
    let mut libs = Vec::new();

    for line in output.lines() {
        if line.contains("(NEEDED)") && line.contains("Shared library:") {
            if let Some(start) = line.find('[') {
                if let Some(end) = line.find(']') {
                    let lib_name = &line[start + 1..end];
                    libs.push(lib_name.to_string());
                }
            }
        }
    }

    Ok(libs)
}

/// Recursively get all library dependencies (including transitive).
///
/// Some libraries depend on other libraries. We need to copy all of them.
/// The `extra_lib_paths` parameter specifies additional paths to search for libraries.
pub fn get_all_dependencies(
    source_root: &Path,
    binary_path: &Path,
    extra_lib_paths: &[&str],
) -> Result<HashSet<String>> {
    let mut all_libs = HashSet::new();
    let mut to_process = vec![binary_path.to_path_buf()];
    let mut processed = HashSet::new();

    while let Some(path) = to_process.pop() {
        if processed.contains(&path) {
            continue;
        }
        processed.insert(path.clone());

        let deps = get_library_dependencies(&path)?;
        for lib_name in deps {
            if all_libs.insert(lib_name.clone()) {
                // New library - find it and check its dependencies too
                if let Some(lib_path) = find_library(source_root, &lib_name, extra_lib_paths) {
                    to_process.push(lib_path);
                }
            }
        }
    }

    Ok(all_libs)
}

/// Find a library in standard paths within a rootfs.
///
/// Searches lib64, lib, and extra paths.
/// Returns `None` if the library is not found in any search path.
pub fn find_library(source_root: &Path, lib_name: &str, extra_paths: &[&str]) -> Option<PathBuf> {
    let mut candidates = vec![
        source_root.join("usr/lib64").join(lib_name),
        source_root.join("lib64").join(lib_name),
        source_root.join("usr/lib").join(lib_name),
        source_root.join("lib").join(lib_name),
        // Systemd private libraries
        source_root.join("usr/lib64/systemd").join(lib_name),
        source_root.join("usr/lib/systemd").join(lib_name),
    ];

    for extra in extra_paths {
        candidates.push(source_root.join(extra).join(lib_name));
    }

    candidates
        .into_iter()
        .find(|p| p.exists() || p.is_symlink())
}

/// Find a binary in standard bin/sbin directories.
pub fn find_binary(source_root: &Path, binary: &str) -> Option<PathBuf> {
    let candidates = [
        source_root.join("usr/bin").join(binary),
        source_root.join("bin").join(binary),
        source_root.join("usr/sbin").join(binary),
        source_root.join("sbin").join(binary),
    ];

    candidates.into_iter().find(|p| p.exists())
}

/// Make a file executable (chmod 755).
pub fn make_executable(path: &Path) -> Result<()> {
    let mut perms = fs::metadata(path)
        .with_context(|| format!("Failed to read metadata: {}", path.display()))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)
        .with_context(|| format!("Failed to set permissions: {}", path.display()))?;
    Ok(())
}

/// Copy a directory recursively, handling symlinks.
///
/// Returns the total size in bytes of all files copied.
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<u64> {
    let mut total_size: u64 = 0;

    if !src.is_dir() {
        return Ok(0);
    }

    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest_path = dst.join(entry.file_name());

        if path.is_dir() {
            total_size += copy_dir_recursive(&path, &dest_path)?;
        } else if path.is_symlink() {
            let target = fs::read_link(&path)?;
            if !dest_path.exists() && !dest_path.is_symlink() {
                std::os::unix::fs::symlink(&target, &dest_path)?;
            }
        } else {
            fs::copy(&path, &dest_path)?;
            if let Ok(meta) = fs::metadata(&dest_path) {
                total_size += meta.len();
            }
        }
    }

    Ok(total_size)
}

/// Copy a library from source to destination, handling symlinks.
///
/// The `dest_lib64_path` and `dest_lib_path` parameters specify where
/// libraries should be copied (e.g., "lib64" for initramfs, "usr/lib64" for rootfs).
///
/// The `private_lib_dirs` parameter specifies subdirectories that should preserve
/// their structure (e.g., `&["systemd"]` for systemd private libraries).
pub fn copy_library_to(
    source_root: &Path,
    lib_name: &str,
    dest_root: &Path,
    dest_lib64_path: &str,
    dest_lib_path: &str,
    extra_lib_paths: &[&str],
    private_lib_dirs: &[&str],
) -> Result<()> {
    let src = find_library(source_root, lib_name, extra_lib_paths).with_context(|| {
        format!(
            "Could not find library '{}' in source (searched lib64, lib, extra paths)",
            lib_name
        )
    })?;

    // Check if this is a private library (e.g., systemd)
    let src_str = src.to_string_lossy();
    let private_dir = private_lib_dirs.iter().find(|dir| {
        src_str.contains(&format!("lib64/{}", dir)) || src_str.contains(&format!("lib/{}", dir))
    });

    let dest_path = if let Some(dir) = private_dir {
        let dest_dir = dest_root.join(dest_lib64_path).join(dir);
        fs::create_dir_all(&dest_dir)?;
        dest_dir.join(lib_name)
    } else if src_str.contains("lib64") {
        dest_root.join(dest_lib64_path).join(lib_name)
    } else {
        dest_root.join(dest_lib_path).join(lib_name)
    };

    if dest_path.exists() {
        return Ok(()); // Already copied
    }

    // Handle symlinks - copy both the symlink target and create the symlink
    if src.is_symlink() {
        let link_target = fs::read_link(&src)?;

        // Resolve the actual file
        let actual_src = if link_target.is_relative() {
            src.parent()
                .context("Library path has no parent")?
                .join(&link_target)
        } else {
            source_root.join(link_target.to_str().unwrap().trim_start_matches('/'))
        };

        if actual_src.exists() {
            // Copy the actual file first
            let target_name = link_target.file_name().unwrap_or(link_target.as_os_str());
            let target_dest = dest_path.parent().unwrap().join(target_name);
            if !target_dest.exists() {
                fs::copy(&actual_src, &target_dest)?;
            }
            // Create symlink
            if !dest_path.exists() {
                std::os::unix::fs::symlink(&link_target, &dest_path)?;
            }
        } else {
            // Symlink target not found, copy the symlink itself
            fs::copy(&src, &dest_path)?;
        }
    } else {
        fs::copy(&src, &dest_path)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_readelf_output() {
        let output = r#"
Dynamic section at offset 0x2d0e0 contains 28 entries:
  Tag        Type                         Name/Value
 0x0000000000000001 (NEEDED)             Shared library: [libtinfo.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x000000000000000c (INIT)               0x5000
"#;
        let libs = parse_readelf_output(output).unwrap();
        assert_eq!(libs, vec!["libtinfo.so.6", "libc.so.6"]);
    }

    #[test]
    fn test_parse_readelf_empty() {
        let output = "not an ELF file";
        let libs = parse_readelf_output(output).unwrap();
        assert!(libs.is_empty());
    }
}
