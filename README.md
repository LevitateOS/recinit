# recinit

Rootless initramfs builder for Linux. Build initramfs images without requiring root privileges or external tools like dracut.

## Features

- **No root required**: Build initramfs as a regular user
- **No external dependencies**: No dracut, mkinitcpio, or other tools needed
- **Two initramfs types**:
  - **Tiny/Live**: Small (~5MB) busybox-based for live ISO boot
  - **Install**: Full (~30-50MB) systemd-based for installed systems
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

Complete set for any hardware:
- All storage drivers (NVMe, SATA, USB, virtio)
- All common filesystems (ext4, xfs, btrfs, vfat)
- USB HID (keyboards for LUKS prompts)
- Device mapper (LUKS/LVM)

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
│   ├── elf.rs       # ELF analysis for library deps
│   ├── modules.rs   # Kernel module presets
│   ├── busybox.rs   # Busybox setup
│   ├── tiny.rs      # Live initramfs builder
│   ├── install.rs   # Install initramfs builder
│   └── systemd.rs   # Systemd copying
└── templates/
    └── init_tiny.template  # Init script template
```

## License

MIT OR Apache-2.0
