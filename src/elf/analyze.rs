//! ELF binary analysis using readelf.

use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use super::paths::find_library;

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
#[must_use = "library dependencies should be processed"]
pub(crate) fn get_library_dependencies(binary_path: &Path) -> Result<Vec<String>> {
    // Check file exists first for a clear error message
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
        // These are legitimate "not an ELF" or permission cases, not fatal errors
        // For unreadable files (like setuid binaries), we return empty deps and
        // let the caller copy the binary without library analysis
        if stderr.contains("Not an ELF file")
            || stderr.contains("not a dynamic executable")
            || stderr.contains("File format not recognized")
            || stderr.contains("Failed to read file header")
            || stderr.contains("is not readable")
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
pub(crate) fn parse_readelf_output(output: &str) -> Result<Vec<String>> {
    let mut libs = Vec::new();

    for line in output.lines() {
        // Look for lines containing "(NEEDED)" and "Shared library:"
        if line.contains("(NEEDED)") && line.contains("Shared library:") {
            // Extract library name from [libname.so.X]
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
/// The `extra_lib_paths` parameter is passed to `find_library` for each lookup.
pub(crate) fn get_all_dependencies(
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
