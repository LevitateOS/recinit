//! CPIO archive creation for initramfs.
//!
//! Provides utilities for creating compressed cpio archives
//! used as initramfs images.

use anyhow::{Context, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use walkdir::WalkDir;

/// CPIO newc format header size (110 bytes + filename + padding)
const CPIO_HEADER_MAGIC: &str = "070701";

/// Build a compressed cpio archive from a directory.
///
/// Creates a gzip-compressed cpio archive in newc format, suitable for
/// use as a Linux initramfs.
///
/// # Arguments
///
/// * `root` - Directory containing the initramfs contents
/// * `output` - Path for the output .cpio.gz file
/// * `gzip_level` - Gzip compression level (1-9, higher = smaller but slower)
///
/// # Example
///
/// ```rust,ignore
/// use recinit::build_cpio;
/// use std::path::Path;
///
/// build_cpio(
///     Path::new("/tmp/initramfs-root"),
///     Path::new("/tmp/initramfs.cpio.gz"),
///     6,
/// )?;
/// ```
pub fn build_cpio(root: &Path, output: &Path, gzip_level: u32) -> Result<()> {
    let file = File::create(output).with_context(|| format!("Failed to create {}", output.display()))?;
    let encoder = GzEncoder::new(file, Compression::new(gzip_level));
    let mut writer = CpioWriter::new(encoder);

    // Walk the directory tree
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        let path = entry.path();

        // Get path relative to root, starting with "."
        let rel_path = path.strip_prefix(root).unwrap_or(path);
        let name = if rel_path.as_os_str().is_empty() {
            ".".to_string()
        } else {
            format!("./{}", rel_path.display())
        };

        writer.add_entry(path, &name)?;
    }

    // Write trailer
    writer.finish()?;

    Ok(())
}

/// CPIO archive writer in newc format.
struct CpioWriter<W: Write> {
    writer: W,
}

impl<W: Write> CpioWriter<W> {
    fn new(writer: W) -> Self {
        Self { writer }
    }

    /// Add an entry to the archive.
    fn add_entry(&mut self, path: &Path, name: &str) -> Result<()> {
        let metadata = fs::symlink_metadata(path)?;

        // Determine file type mode bits
        let file_type = metadata.file_type();
        let mode = if file_type.is_dir() {
            0o040000 | (metadata.permissions().mode() & 0o7777)
        } else if file_type.is_symlink() {
            0o120000 | 0o777
        } else {
            0o100000 | (metadata.permissions().mode() & 0o7777)
        };

        // Get file size (for regular files and symlinks)
        let filesize = if file_type.is_symlink() {
            fs::read_link(path)?.as_os_str().len() as u32
        } else if file_type.is_file() {
            metadata.len() as u32
        } else {
            0
        };

        // Write header
        self.write_header(
            metadata.ino() as u32,
            mode,
            metadata.uid(),
            metadata.gid(),
            metadata.nlink() as u32,
            0, // mtime - use 0 for reproducibility
            filesize,
            0, // dev major
            0, // dev minor
            0, // rdev major
            0, // rdev minor
            name,
        )?;

        // Write file content
        if file_type.is_symlink() {
            let target = fs::read_link(path)?;
            let target_bytes = target.as_os_str().as_encoded_bytes();
            self.writer.write_all(target_bytes)?;
            self.write_padding(target_bytes.len())?;
        } else if file_type.is_file() && filesize > 0 {
            let mut file = File::open(path)?;
            io::copy(&mut file, &mut self.writer)?;
            self.write_padding(filesize as usize)?;
        }

        Ok(())
    }

    /// Write a CPIO header in newc format.
    #[allow(clippy::too_many_arguments)]
    fn write_header(
        &mut self,
        ino: u32,
        mode: u32,
        uid: u32,
        gid: u32,
        nlink: u32,
        mtime: u32,
        filesize: u32,
        dev_major: u32,
        dev_minor: u32,
        rdev_major: u32,
        rdev_minor: u32,
        name: &str,
    ) -> Result<()> {
        let name_bytes = name.as_bytes();
        let namesize = name_bytes.len() + 1; // Include null terminator

        // newc format: magic + 13 8-char hex fields
        let header = format!(
            "{}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}",
            CPIO_HEADER_MAGIC,
            ino,
            mode,
            uid,
            gid,
            nlink,
            mtime,
            filesize,
            dev_major,
            dev_minor,
            rdev_major,
            rdev_minor,
            namesize,
            0, // checksum (unused in newc)
        );

        self.writer.write_all(header.as_bytes())?;
        self.writer.write_all(name_bytes)?;
        self.writer.write_all(&[0])?; // Null terminator

        // Pad header + name to 4-byte boundary
        let header_total = 110 + namesize;
        self.write_padding(header_total)?;

        Ok(())
    }

    /// Write padding to align to 4-byte boundary.
    fn write_padding(&mut self, size: usize) -> Result<()> {
        let padding = (4 - (size % 4)) % 4;
        if padding > 0 {
            self.writer.write_all(&[0u8; 4][..padding])?;
        }
        Ok(())
    }

    /// Write the trailer entry and finish the archive.
    fn finish(mut self) -> Result<W> {
        // TRAILER!!! entry marks end of archive
        self.write_header(
            0,                 // ino
            0,                 // mode
            0,                 // uid
            0,                 // gid
            1,                 // nlink
            0,                 // mtime
            0,                 // filesize
            0,                 // dev_major
            0,                 // dev_minor
            0,                 // rdev_major
            0,                 // rdev_minor
            "TRAILER!!!",
        )?;

        Ok(self.writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_build_cpio() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("root");
        let output = temp.path().join("test.cpio.gz");

        // Create a simple directory structure
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(root.join("bin/test"), "#!/bin/sh\necho hello\n").unwrap();
        fs::write(root.join("init"), "#!/bin/sh\nexec /bin/sh\n").unwrap();

        // Build the cpio archive
        build_cpio(&root, &output, 6).unwrap();

        // Verify output exists and has content
        assert!(output.exists());
        let size = fs::metadata(&output).unwrap().len();
        assert!(size > 0, "CPIO archive should not be empty");

        // Verify gzip magic
        let content = fs::read(&output).unwrap();
        assert_eq!(content[0], 0x1f, "Should start with gzip magic");
        assert_eq!(content[1], 0x8b, "Should have gzip magic byte 2");
    }

    #[test]
    fn test_build_cpio_with_symlink() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("root");
        let output = temp.path().join("test.cpio.gz");

        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(root.join("bin/busybox"), "binary content").unwrap();
        std::os::unix::fs::symlink("busybox", root.join("bin/sh")).unwrap();

        build_cpio(&root, &output, 6).unwrap();

        assert!(output.exists());
        assert!(fs::metadata(&output).unwrap().len() > 0);
    }
}
