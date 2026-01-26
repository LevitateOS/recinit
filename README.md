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
  --template templates/init_tiny.template \
  --output initramfs.cpio.gz \
  --iso-label MYLINUX \
  --rootfs-path live/filesystem.erofs
```

### Build Install Initramfs

```bash
recinit build-install \
  --rootfs /path/to/rootfs-staging \
  --output initramfs-installed.img \
  --modules install
```

### List Module Presets

```bash
# List available presets
recinit modules --list-presets

# Show modules in a preset
recinit modules --preset live
recinit modules --preset install
```

## Library Usage

```rust
use recinit::{InitramfsBuilder, TinyConfig, InstallConfig, ModulePreset};
use std::path::PathBuf;

// Build live initramfs
let builder = InitramfsBuilder::new();

let config = TinyConfig {
    modules_dir: PathBuf::from("/lib/modules/6.12.0"),
    busybox_path: PathBuf::from("/usr/bin/busybox"),
    template_path: PathBuf::from("templates/init_tiny.template"),
    output: PathBuf::from("initramfs.cpio.gz"),
    iso_label: "MYLINUX".to_string(),
    rootfs_path: "live/filesystem.erofs".to_string(),
    ..Default::default()
};

builder.build_tiny(config)?;

// Build install initramfs
let install_config = InstallConfig {
    rootfs: PathBuf::from("rootfs-staging"),
    output: PathBuf::from("initramfs-installed.img"),
    module_preset: ModulePreset::Install,
    ..Default::default()
};

builder.build_install(install_config)?;
```

## Module Presets

### Live (Tiny)

Minimal modules for booting from ISO/CD:
- Virtio (QEMU support)
- SCSI/CDROM (ISO mounting)
- NVMe/SATA (real hardware)
- Loop, squashfs, overlay (live boot)

### Install (Full)

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
| `{{LIVE_OVERLAY_PATH}}` | Path to live overlay on ISO |

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
