//! Gzip-compressed newc cpio archives for initramfs images.

use anyhow::{bail, Context, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use walkdir::WalkDir;

/// newc header magic.
const CPIO_HEADER_MAGIC: &str = "070701";

/// Build an initramfs at `output`: `fill` populates a private scratch tree
/// (removed whatever happens), which is archived to a temporary file next to
/// `output` and renamed into place only once it is complete.
pub(crate) fn build_archive(
    output: &Path,
    gzip_level: u32,
    fill: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let dir = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let scratch = tempfile::Builder::new()
        .prefix(".initramfs-root-")
        .tempdir_in(dir)
        .with_context(|| format!("creating scratch tree in {}", dir.display()))?;
    fill(scratch.path())?;
    if fs::read_dir(scratch.path())?.next().is_none() {
        anyhow::bail!("initramfs build put nothing into the archive");
    }
    let archive = tempfile::Builder::new()
        .prefix(".initramfs-")
        .tempfile_in(dir)?;
    build_cpio(scratch.path(), archive.path(), gzip_level)?;
    // A temporary file is private; the initramfs is an ordinary readable file.
    fs::set_permissions(archive.path(), fs::Permissions::from_mode(0o644))?;
    archive
        .persist(output)
        .with_context(|| format!("writing {}", output.display()))?;
    Ok(())
}

/// Write `root` as a gzip-compressed newc cpio archive.
fn build_cpio(root: &Path, output: &Path, gzip_level: u32) -> Result<()> {
    let file =
        File::create(output).with_context(|| format!("Failed to create {}", output.display()))?;
    let encoder = GzEncoder::new(file, Compression::new(gzip_level));
    let mut writer = CpioWriter::new(encoder);

    // Sorted, so the same tree always gives the same archive.
    for entry in WalkDir::new(root).follow_links(false).sort_by_file_name() {
        let entry = entry?;
        let path = entry.path();

        let rel_path = path.strip_prefix(root).unwrap_or(path);
        let rel_name = rel_path
            .to_str()
            .with_context(|| format!("'{}' is not a UTF-8 name, which the archive cannot hold", path.display()))?;
        let name = if rel_name.is_empty() {
            ".".to_string()
        } else {
            format!("./{rel_name}")
        };

        writer.add_entry(path, &name)?;
    }

    writer
        .finish()?
        .finish()
        .with_context(|| format!("finishing {}", output.display()))?;
    Ok(())
}

struct CpioWriter<W: Write> {
    writer: W,
    next_ino: u32,
}

impl<W: Write> CpioWriter<W> {
    fn new(writer: W) -> Self {
        Self { writer, next_ino: 1 }
    }

    fn add_entry(&mut self, path: &Path, name: &str) -> Result<()> {
        let metadata = fs::symlink_metadata(path)?;

        let file_type = metadata.file_type();
        let mode = if file_type.is_dir() {
            0o040000 | (metadata.permissions().mode() & 0o7777)
        } else if file_type.is_symlink() {
            0o120000 | 0o777
        } else if file_type.is_file() {
            0o100000 | (metadata.permissions().mode() & 0o7777)
        } else {
            // Device nodes, FIFOs and sockets: the init creates its own /dev.
            bail!("'{}' is not a file, directory or symlink, which recinit does not archive", path.display());
        };

        // newc sizes are 32-bit.
        let too_large = |_| anyhow::anyhow!("'{}' is too large for a newc archive (4 GiB at most)", path.display());
        let filesize = if file_type.is_symlink() {
            u32::try_from(fs::read_link(path)?.as_os_str().len()).map_err(too_large)?
        } else if file_type.is_file() {
            u32::try_from(metadata.len()).map_err(too_large)?
        } else {
            0
        };

        // Nothing from the build host leaks in: entries are numbered in
        // archive order, owned by root, and never hard links (each file's
        // data is written in full).
        let ino = self.next_ino;
        self.next_ino += 1;
        let nlink = if file_type.is_dir() { 2 } else { 1 };
        self.write_header(ino, mode, nlink, filesize, name)?;

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

    /// A newc header; uid, gid, mtime and device numbers are always 0.
    fn write_header(
        &mut self,
        ino: u32,
        mode: u32,
        nlink: u32,
        filesize: u32,
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
            0, // uid
            0, // gid
            nlink,
            0, // mtime
            filesize,
            0, // dev major
            0, // dev minor
            0, // rdev major
            0, // rdev minor
            namesize,
            0, // checksum (unused in newc)
        );

        self.writer.write_all(header.as_bytes())?;
        self.writer.write_all(name_bytes)?;
        self.writer.write_all(&[0])?;

        // The header is 110 bytes; header + name are padded to 4 bytes.
        let header_total = 110 + namesize;
        self.write_padding(header_total)?;

        Ok(())
    }

    fn write_padding(&mut self, size: usize) -> Result<()> {
        let padding = (4 - (size % 4)) % 4;
        if padding > 0 {
            self.writer.write_all(&[0u8; 4][..padding])?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<W> {
        // TRAILER!!! entry marks end of archive
        self.write_header(0, 0, 1, 0, "TRAILER!!!")?;

        Ok(self.writer)
    }
}
