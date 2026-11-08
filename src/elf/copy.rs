//! Copying files out of a rootfs with their symlinks.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Follow `rel` (a path inside `root`) through symlinks that resolve inside
/// `root`, and return the regular file's path relative to `root`.
pub(crate) fn resolve_in(root: &Path, rel: &Path) -> Result<PathBuf> {
    let mut current = rel.to_path_buf();
    for _ in 0..16 {
        let full = root.join(&current);
        if !full.is_symlink() {
            return Ok(current);
        }
        current = link_target(&current, &fs::read_link(&full)?);
    }
    bail!("too many symlinks resolving {} in {}", rel.display(), root.display())
}

/// Where the symlink at `link` (relative to its root) points, relative to
/// that root; an absolute target is taken inside the root.
fn link_target(link: &Path, target: &Path) -> PathBuf {
    let joined = if target.is_absolute() {
        target.strip_prefix("/").unwrap_or(target).to_path_buf()
    } else {
        link.parent().unwrap_or(Path::new("")).join(target)
    };
    let mut out = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(name) => out.push(name),
            _ => {}
        }
    }
    out
}

/// Copy `rel` from `src_root` to the same path under `dst_root`. A symlink is
/// recreated and what it points at is copied too, so the link resolves the
/// way it does in the source. Does nothing when the destination exists.
pub(crate) fn copy_preserving(src_root: &Path, rel: &Path, dst_root: &Path) -> Result<()> {
    let src = src_root.join(rel);
    let dst = dst_root.join(rel);
    if dst.exists() || dst.is_symlink() {
        return Ok(());
    }
    fs::create_dir_all(dst.parent().context("path has no parent")?)?;
    if src.is_symlink() {
        let target = fs::read_link(&src)?;
        std::os::unix::fs::symlink(&target, &dst)?;
        copy_preserving(src_root, &link_target(rel, &target), dst_root)
    } else {
        fs::copy(&src, &dst).with_context(|| format!("copying {}", src.display()))?;
        Ok(())
    }
}
