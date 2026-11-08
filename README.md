# recinit

Rootless initramfs builder for Linux. Build initramfs images without requiring root privileges or external tools like dracut.

## Features

- **No root required**: Build initramfs as a regular user
- **No external dependencies**: No dracut, mkinitcpio, or other tools needed
- **Two initramfs types**:
  - **Tiny/Live**: Small (~5MB) busybox-based for live ISO boot
  - **Install**: busybox-based (~5MB) for installed systems of either distro; independent of the installed init and libc
- **Pure Rust CPIO**: Generates CPIO archives natively without shelling out
- **Configurable module presets**: Predefined module lists for common scenarios

## Installation

```bash
cargo install --path .
```

Or add as a dependency:

```toml
[dependencies]
recinit = { git = "https://github.com/LevitateOS/recinit" }
```

## CLI Usage

### Build Live Initramfs

```bash
recinit build-tiny \
  --modules-dir /lib/modules/$(uname -r) \
  --busybox /usr/bin/busybox \
  --output initramfs.cpio.gz \
  --iso-label MYLINUX \
  --boot-banner "MyLinux initramfs starting..."
```

The init script is `templates/init_tiny.template`, built in; the payload paths
it looks for on the medium are `recinit::live`'s.

### Build Install Initramfs

One busybox `/init` (`templates/init_installed.template`) boots both installed
systems: it loads the storage modules, finds `root=PARTUUID|UUID|LABEL=` (with
a deadline), runs `e2fsck -p` on ext2/3/4, mounts read-only and `switch_root`s
to `/sbin/init`. A failure reboots when the command line has
`rd.emergency=reboot` (an A/B trial) and opens a shell otherwise. `blkid`
(util-linux) and `e2fsck` are copied from `--rootfs` with their libraries,
found through each binary's own ELF interpreter, so the rootfs may be glibc or
musl. No LUKS/LVM.

```bash
recinit build-install \
  --rootfs /path/to/rootfs-staging \
  --busybox /usr/bin/busybox \
  --modules-dir /path/to/rootfs-staging/usr/lib/modules/6.12.0 \
  --output initramfs-installed.img
```

## Library Usage

`build_tiny_initramfs(&TinyConfig, verbose)` and
`build_install_initramfs(&InstallConfig, verbose)` do what the two commands do.

## Module Presets

### Live (`LIVE_MODULES`, build-tiny)

Minimal modules for booting from ISO/CD:
- Virtio (QEMU support)
- SCSI/CDROM (ISO mounting)
- NVMe/SATA (real hardware)
- Loop, EROFS, overlay (live boot)

### Install (`INSTALL_MODULES`, build-install)

Storage drivers (NVMe, SATA, USB, virtio) and ext4/vfat. No LUKS/LVM, xfs or btrfs.

## Init Script Template

The live initramfs uses a template-based init script with these variables:

| Variable | Description |
|----------|-------------|
| `{{ISO_LABEL}}` | ISO volume label for device detection |
| `{{ROOTFS_PATH}}` | Path to rootfs inside ISO |
| `{{BOOT_MODULES}}` | Space-separated module names |
| `{{BOOT_DEVICES}}` | Space-separated device paths to probe |
| `{{LIVE_OVERLAY_IMAGE_PATH}}` | Path to live overlay payload image on ISO |

## Architecture

```
recinit/
├── src/
│   ├── lib.rs       # Public API
│   ├── main.rs      # CLI
│   ├── cpio.rs      # CPIO archive building
│   ├── elf/         # ELF analysis and copying with symlinks
│   ├── modules.rs   # Kernel module presets
│   ├── busybox.rs   # Busybox setup
│   ├── tiny.rs      # Live initramfs builder
│   └── install.rs   # Install initramfs builder
└── templates/
    ├── init_tiny.template       # Live init
    └── init_installed.template  # Installed-system init
```

## License

MIT OR Apache-2.0
