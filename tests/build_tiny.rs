//! `recinit build-tiny` end to end, with a stand-in busybox (it only has to
//! answer `--list`) and a stand-in module tree: one loadable module plus
//! `modules.builtin` naming the rest.

use distro_spec::shared::live_boot::ROOTFS_HANDLE;
use distro_spec::shared::LIVE_MODULES;
use flate2::read::GzDecoder;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const RELEASE: &str = "6.0.0-recinit-test";

fn fixture(dir: &Path) {
    let busybox = dir.join("busybox");
    fs::write(&busybox, "#!/bin/sh\n[ \"$1\" = --list ] && printf 'sh\\nmount\\n'\n").unwrap();
    fs::set_permissions(&busybox, fs::Permissions::from_mode(0o755)).unwrap();

    let modules = dir.join(RELEASE);
    let loadable = LIVE_MODULES[0];
    let rel = format!("kernel/drivers/{loadable}.ko");
    fs::create_dir_all(modules.join("kernel/drivers")).unwrap();
    fs::write(modules.join(&rel), b"not really a module").unwrap();
    fs::write(modules.join("modules.dep"), format!("{rel}:\n")).unwrap();
    let builtin: String = LIVE_MODULES[1..]
        .iter()
        .map(|m| format!("kernel/builtin/{m}.ko\n"))
        .collect();
    fs::write(modules.join("modules.builtin"), builtin).unwrap();
}

fn build(dir: &Path, output: &str) -> Vec<u8> {
    let status = Command::new(env!("CARGO_BIN_EXE_recinit"))
        .current_dir(dir)
        .args(["build-tiny", "--busybox", "busybox", "--modules-dir", RELEASE, "-o", output])
        .args(["--set", "BOOT_BANNER=Test Live", "--set", "PRODUCT_TAG=test"])
        .status()
        .unwrap();
    assert!(status.success(), "build-tiny failed");
    fs::read(dir.join(output)).unwrap()
}

/// The newc cpio archive's regular files and symlink targets by path.
fn entries(archive: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut raw = Vec::new();
    GzDecoder::new(archive).read_to_end(&mut raw).unwrap();
    let hex = |b: &[u8]| usize::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap();
    let align = |n: usize| (n + 3) & !3;
    let (mut pos, mut out) = (0, BTreeMap::new());
    loop {
        let header = &raw[pos..pos + 110];
        assert_eq!(&header[..6], b"070701", "newc magic");
        let (filesize, namesize) = (hex(&header[54..62]), hex(&header[94..102]));
        let name = String::from_utf8(raw[pos + 110..pos + 110 + namesize - 1].to_vec()).unwrap();
        let data_start = align(pos + 110 + namesize);
        if name == "TRAILER!!!" {
            return out;
        }
        let path = name.trim_start_matches("./").trim_start_matches('/').to_string();
        out.insert(path, raw[data_start..data_start + filesize].to_vec());
        pos = align(data_start + filesize);
    }
}

#[test]
fn tiny_initramfs_is_complete_and_reproducible() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let first = build(dir.path(), "a.cpio.gz");
    let second = build(dir.path(), "b.cpio.gz");
    assert_eq!(first, second, "two builds from the same inputs differ");

    let files = entries(&first);
    let init = String::from_utf8(files["init"].clone()).unwrap();
    assert!(!init.contains("{{"), "unfilled placeholder in /init");
    assert!(init.contains(ROOTFS_HANDLE), "/init lacks the rootfs handle");
    assert!(init.contains(RELEASE), "/init lacks the kernel release");
    assert!(init.contains("Test Live"), "/init lacks the --set banner");
    assert!(
        files.keys().any(|p| p.ends_with(&format!("{}.ko", LIVE_MODULES[0]))),
        "the loadable module is missing"
    );
    assert!(files.contains_key("bin/busybox"), "busybox is missing");
}

#[test]
fn a_module_the_kernel_lacks_fails_the_build() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    fs::write(dir.path().join(RELEASE).join("modules.builtin"), "").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_recinit"))
        .current_dir(dir.path())
        .args(["build-tiny", "--busybox", "busybox", "--modules-dir", RELEASE])
        .args(["--set", "BOOT_BANNER=x", "--set", "PRODUCT_TAG=x"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("neither a module nor a built-in"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
