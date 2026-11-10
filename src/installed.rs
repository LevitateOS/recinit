//! The installed A/B boot contract, shared by recab and its initramfs.
pub const IMMUTABLE_OPTION: &str = "recab.immutable";
pub const IMMUTABLE_CONFIG: &str = "usr/lib/recab/immutable";
pub const BOOT_RECORD: &str = "usr/lib/recab/boot-generation";
pub const SLOT_STATE_DIR: &str = "lib/recab/slots";
pub const IMMUTABLE_INIT_MARKER: &str = "# recab-immutable-v3";

#[cfg(feature = "inspect")]
#[derive(Debug)]
pub struct InstalledArchive {
    pub kernel_release: String,
    pub files: std::collections::BTreeMap<String, String>,
    pub machines: std::collections::BTreeSet<u16>,
}

/// Parse the whole gzip/newc archive. Duplicate entries, unsafe paths, malformed
/// trailers and truncated streams must not hide a different executable /init.
#[cfg(feature = "inspect")]
pub fn inspect_installed(path: &std::path::Path) -> std::io::Result<InstalledArchive> {
    use sha2::{Digest, Sha256};
    use std::collections::{BTreeMap, BTreeSet};
    use std::io::{self, BufReader, Read};
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid installed initramfs or unsupported immutable boot protocol (needs v3)",
        )
    };
    let mut archive = flate2::bufread::GzDecoder::new(BufReader::new(std::fs::File::open(path)?));
    let mut names = BTreeSet::new();
    let mut files = BTreeMap::new();
    let mut machines = BTreeSet::new();
    let mut release = None;
    let mut total = 0u64;
    let mut executable_init = false;
    loop {
        let mut header = [0u8; 110];
        archive.read_exact(&mut header)?;
        if &header[..6] != b"070701" {
            return Err(invalid());
        }
        let hex = |a, b| -> io::Result<u64> {
            u64::from_str_radix(
                std::str::from_utf8(&header[a..b]).map_err(|_| invalid())?,
                16,
            )
            .map_err(|_| invalid())
        };
        let mode = hex(14, 22)?;
        let size = hex(54, 62)?;
        let count = hex(94, 102)?;
        if !(1..=4096).contains(&count) || names.len() > 100_000 {
            return Err(invalid());
        }
        total = total.checked_add(size).ok_or_else(invalid)?;
        if total > 512 * 1024 * 1024 {
            return Err(invalid());
        }
        let mut name = vec![0; count as usize];
        archive.read_exact(&mut name)?;
        if name.pop() != Some(0) {
            return Err(invalid());
        }
        let mut padding = [0u8; 3];
        archive.read_exact(&mut padding[..((4 - (110 + count) % 4) % 4) as usize])?;
        if name == b"TRAILER!!!" {
            if size != 0 || !executable_init {
                return Err(invalid());
            }
            let mut tail = [0u8; 4096];
            loop {
                let n = archive.read(&mut tail)?;
                if n == 0 {
                    break;
                }
                if tail[..n].iter().any(|b| *b != 0) {
                    return Err(invalid());
                }
            }
            if archive.into_inner().read(&mut [0u8; 1])? != 0 {
                return Err(invalid());
            }
            return Ok(InstalledArchive {
                kernel_release: release.ok_or_else(invalid)?,
                files,
                machines,
            });
        }
        let name = std::str::from_utf8(&name)
            .map_err(|_| invalid())?
            .trim_start_matches("./")
            .to_string();
        if name.is_empty() || name == "." {
            if mode & 0o170000 != 0o040000 || size != 0 || !names.insert(String::new()) {
                return Err(invalid());
            }
            continue;
        }
        if !std::path::Path::new(&name)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
            || !names.insert(name.clone())
        {
            return Err(invalid());
        }
        if !matches!(mode & 0o170000, 0o100000 | 0o040000 | 0o120000)
            || (mode & 0o170000 != 0o040000 && hex(38, 46)? != 1)
        {
            return Err(invalid());
        }
        let mut hash = Sha256::new();
        let mut left = size;
        let mut prefix = Vec::new();
        let mut init = Vec::new();
        let mut buffer = [0u8; 64 * 1024];
        while left > 0 {
            let n = (left as usize).min(buffer.len());
            archive.read_exact(&mut buffer[..n])?;
            hash.update(&buffer[..n]);
            if prefix.len() < 64 {
                prefix.extend_from_slice(&buffer[..n.min(64 - prefix.len())]);
            }
            if name == "init" {
                if size > 1024 * 1024 {
                    return Err(invalid());
                }
                init.extend_from_slice(&buffer[..n]);
            }
            left -= n as u64;
        }
        if prefix.starts_with(b"\x7fELF") {
            if prefix.len() < 64 || prefix[4] != 2 || prefix[5] != 1 {
                return Err(invalid());
            }
            machines.insert(u16::from_le_bytes([prefix[18], prefix[19]]));
        }
        if mode & 0o170000 == 0o100000 {
            files.insert(name.clone(), format!("{:x}", hash.finalize()));
        }
        if name == "init" {
            if mode & 0o170000 != 0o100000 || mode & 0o111 == 0 {
                return Err(invalid());
            }
            let text = std::str::from_utf8(&init).map_err(|_| invalid())?;
            if !text.lines().any(|l| l == IMMUTABLE_INIT_MARKER) {
                return Err(invalid());
            }
            let versions = text
                .lines()
                .filter_map(|l| l.strip_prefix("# recab-kernel-release="))
                .collect::<Vec<_>>();
            if versions.len() != 1
                || !versions[0]
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".+_-".contains(&b))
                || versions[0].is_empty()
            {
                return Err(invalid());
            }
            release = Some(versions[0].to_string());
            executable_init = true;
        }
        archive.read_exact(&mut padding[..((4 - size % 4) % 4) as usize])?;
    }
}
#[cfg(feature = "inspect")]
pub fn check_immutable_initramfs(path: &std::path::Path) -> std::io::Result<()> {
    inspect_installed(path).map(|_| ())
}

/// Content/type identity of a module tree; independent of build-host ownership
/// and extraction timestamps. Symlinks are recorded, never followed.
#[cfg(feature = "inspect")]
pub fn modules_digest(root: &std::path::Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        io::{self, Read},
        os::unix::ffi::OsStrExt,
        path::Path,
    };
    fn walk(root: &Path, rel: &Path, hash: &mut Sha256) -> io::Result<()> {
        let mut entries = fs::read_dir(root.join(rel))?.collect::<io::Result<Vec<_>>>()?;
        entries.sort_by_key(|e| e.file_name());
        for item in entries {
            let p = rel.join(item.file_name());
            let m = fs::symlink_metadata(item.path())?;
            let bytes = p.as_os_str().as_bytes();
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
            if m.is_dir() {
                hash.update(b"d");
                walk(root, &p, hash)?;
            } else if m.file_type().is_symlink() {
                let value = fs::read_link(item.path())?;
                hash.update(b"l");
                hash.update((value.as_os_str().len() as u64).to_le_bytes());
                hash.update(value.as_os_str().as_bytes());
            } else if m.is_file() {
                hash.update(b"f");
                let mut content = Sha256::new();
                let mut file = fs::File::open(item.path())?;
                let mut buffer = [0u8; 65536];
                loop {
                    let n = file.read(&mut buffer)?;
                    if n == 0 {
                        break;
                    }
                    content.update(&buffer[..n]);
                }
                hash.update(content.finalize());
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "special file in kernel modules",
                ));
            }
        }
        Ok(())
    }
    let mut hash = Sha256::new();
    walk(root, Path::new(""), &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}
