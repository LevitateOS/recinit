//! The live-boot protocol: where the live init finds its payloads and how a
//! test boot hands it data.
//!
//! The live init this crate builds implements the protocol, so the protocol
//! lives here. The ISO assembler (reciso), the installer's rootfs discovery
//! (recstrap), the overlay builder (distro-builder) and the boot tests follow
//! it. The init template spells no path, command-line word or filesystem
//! type: recinit fills them from `init_template_vars` and refuses a template
//! that still has a placeholder left.
//!
//! This module needs none of recinit's builder dependencies: tools that only
//! follow the protocol depend on recinit with `default-features = false`.

macro_rules! iso_boot_dir {
    () => {
        "boot"
    };
}
macro_rules! iso_live_dir {
    () => {
        "live"
    };
}
macro_rules! live_media_dir {
    () => {
        "/run/live-media"
    };
}
macro_rules! rootfs_iso_path {
    () => {
        concat!(iso_live_dir!(), "/filesystem.erofs")
    };
}
macro_rules! initramfs_installed_file {
    () => {
        "initramfs-installed.img"
    };
}
macro_rules! rootfs_mount {
    () => {
        "/rootfs"
    };
}
macro_rules! live_overlay_mount {
    () => {
        "/live-overlay"
    };
}
macro_rules! fail_reboot_option {
    () => {
        "rd.emergency=reboot"
    };
}
#[allow(unused_imports)]
pub(crate) use fail_reboot_option;
macro_rules! boot_injection_dir {
    () => {
        "/run/boot-injection"
    };
}

// ---------------------------------------------------------------------------
// Live media layout (paths relative to the ISO root)
// ---------------------------------------------------------------------------

/// Kernel and live initramfs.
pub const ISO_BOOT_DIR: &str = iso_boot_dir!();
/// Rootfs and live overlay payload images.
pub const ISO_LIVE_DIR: &str = iso_live_dir!();
pub const KERNEL_ISO_PATH: &str = concat!(iso_boot_dir!(), "/vmlinuz");
pub const INITRAMFS_LIVE_ISO_PATH: &str = concat!(iso_boot_dir!(), "/initramfs.img");
/// The installed system's initramfs, which the release puts beside the live
/// one for the variants that boot through one.
pub const INITRAMFS_INSTALLED_FILENAME: &str = initramfs_installed_file!();
pub const INITRAMFS_INSTALLED_ISO_PATH: &str =
    concat!(iso_boot_dir!(), "/", initramfs_installed_file!());
/// The EROFS image holding the whole system.
pub const ROOTFS_ISO_PATH: &str = rootfs_iso_path!();
/// The EROFS image mounted as the upper read-only layer of the live root.
pub const LIVE_OVERLAYFS_ISO_PATH: &str = concat!(iso_live_dir!(), "/overlayfs.erofs");

// ---------------------------------------------------------------------------
// The live system
// ---------------------------------------------------------------------------

/// Where the boot medium stays mounted in the live system.
pub const LIVE_MEDIA_DIR: &str = live_media_dir!();

/// Stable handle to the rootfs payload in the live system: a symlink to the
/// image file on the boot medium.
pub const ROOTFS_HANDLE: &str = "/run/live-rootfs.erofs";

/// Stable handle to the live overlay payload, when there is one.
pub const OVERLAY_HANDLE: &str = "/run/live-overlay.erofs";

/// Where an installer finds the rootfs on the live system, in order.
pub const ROOTFS_SEARCH_PATHS: &[&str] = &[
    ROOTFS_HANDLE,
    concat!(live_media_dir!(), "/", rootfs_iso_path!()),
];

/// Left from the appended-partition layout, which no build makes any more:
/// the live init reads payload files only. The builder still writes the role
/// file into payload images; these go once it stops importing them.
pub const PAYLOAD_ROLE_FILE: &str = ".live-payload-role";
pub const ROLE_ROOTFS: &str = "rootfs";
pub const ROLE_OVERLAY: &str = "overlay";

/// Created in the live root so services can tell a live boot.
pub const BOOT_MARKER: &str = "/live-boot-marker";

// ---------------------------------------------------------------------------
// The live init's own mounts (in the initramfs, before switch_root)
// ---------------------------------------------------------------------------

/// Where the init mounts the boot medium; moved to [`LIVE_MEDIA_DIR`] at switch_root.
pub const MEDIA_MOUNT: &str = "/mnt";
/// The rootfs payload, the overlay's lowest layer.
pub const ROOTFS_MOUNT: &str = rootfs_mount!();
/// The live overlay payload, the read-only layer above the rootfs.
pub const LIVE_OVERLAY_MOUNT: &str = live_overlay_mount!();
/// The overlay's `lowerdir`: the live overlay over the rootfs (as `mount`
/// shows it in the live system).
pub const LIVE_LOWERDIR: &str = concat!(live_overlay_mount!(), ":", rootfs_mount!());
/// The tmpfs holding the overlay's upper and work directories.
pub const OVERLAY_DIR: &str = "/overlay";
/// Where the live root is assembled before switch_root.
pub const NEWROOT: &str = "/newroot";
/// Where the init mounts a candidate payload to read its role file.
pub const PROBE_MOUNT: &str = "/probe";
/// Where the init mounts a candidate boot-injection medium.
pub const BOOT_INJECTION_MEDIA_MOUNT: &str = "/boot-injection-media";
/// The program the init hands over to.
pub const SWITCH_ROOT_INIT: &str = "/sbin/init";
/// Seconds the init waits for the boot medium to appear.
pub const MEDIA_DEADLINE_SECS: u32 = 30;

/// Every directory the live initramfs holds, relative to its root: the
/// archive creates them, so the init makes none of its own.
pub const INIT_DIRS: &[&str] = &[
    "bin",
    "dev",
    "proc",
    "sys",
    "tmp",
    "lib/modules",
    "mnt",
    "rootfs",
    "live-overlay",
    "overlay",
    "newroot",
    "probe",
    "boot-injection-media",
];

// ---------------------------------------------------------------------------
// Kernel command-line words
// ---------------------------------------------------------------------------

/// Open a shell on the live init's console before the rootfs is mounted.
pub const CMDLINE_EMERGENCY: &str = "emergency";
/// Stop the live boot when no boot-injection payload arrives.
pub const CMDLINE_BOOT_INJECTION_REQUIRED: &str = "boot_injection=required";
/// An installed initramfs reboots instead of opening an emergency shell (the
/// name dracut uses). recab puts it on an A/B trial boot's entry, so a slot
/// whose root cannot mount returns to the known-good slot.
pub const FAIL_REBOOT_OPTION: &str = fail_reboot_option!();

// ---------------------------------------------------------------------------
// The serial console the boot tests drive
// ---------------------------------------------------------------------------

/// The serial console's kernel `console=` value (the first serial port,
/// which a libvirt domain's `<target port='0'/>` is).
pub const SERIAL_CONSOLE: &str = "ttyS0,115200n8";
/// Printed on the serial console when the live shell is ready for commands.
pub const SHELL_READY_MARKER: &str = "___SHELL_READY___";

/// Printed by the live init when no rootfs payload is found; boot tests match it.
pub const ROOTFS_MISSING_MESSAGE: &str = "Rootfs payload not found";

// ---------------------------------------------------------------------------
// Boot injection: the test channel into a live boot
// ---------------------------------------------------------------------------

/// QEMU fw_cfg entry carrying a boot-injection payload.
pub const BOOT_INJECTION_FW_CFG: &str = "opt/rec/boot-injection";
/// Payload file name on a secondary medium.
pub const BOOT_INJECTION_FILE: &str = "boot-injection.env";
/// Volume label of that secondary medium; the live init finds it by this label.
pub const BOOT_INJECTION_ISO_LABEL: &str = "BOOT_INJECT";
/// Payload keys the live init reads: one SSH public key, or several separated
/// by `;`, which become root's authorized keys in the live system.
pub const BOOT_INJECTION_SSH_KEY: &str = "SSH_AUTHORIZED_KEY";
pub const BOOT_INJECTION_SSH_KEYS: &str = "SSH_AUTHORIZED_KEYS";
/// Where the injected keys go.
pub const ROOT_SSH_DIR: &str = "/root/.ssh";
/// Where the live init leaves the imported payload (KEY=VALUE lines) and, in
/// `source`, where it came from.
pub const BOOT_INJECTION_DIR: &str = boot_injection_dir!();
pub const BOOT_INJECTION_ENV: &str = concat!(boot_injection_dir!(), "/payload.env");

/// Placeholder values for the live init template (besides the build's own:
/// the ISO label, the banner and the boot modules).
pub fn init_template_vars() -> Vec<(&'static str, String)> {
    vec![
        ("LIVE_MEDIA_DIR", LIVE_MEDIA_DIR.to_string()),
        ("ROOTFS_HANDLE", ROOTFS_HANDLE.to_string()),
        ("OVERLAY_HANDLE", OVERLAY_HANDLE.to_string()),
        ("BOOT_MARKER", BOOT_MARKER.to_string()),
        ("BOOT_INJECTION_FW_CFG", BOOT_INJECTION_FW_CFG.to_string()),
        ("BOOT_INJECTION_FILE", BOOT_INJECTION_FILE.to_string()),
        ("BOOT_INJECTION_ISO_LABEL", BOOT_INJECTION_ISO_LABEL.to_string()),
        ("SSH_KEY_VAR", BOOT_INJECTION_SSH_KEY.to_string()),
        ("SSH_KEYS_VAR", BOOT_INJECTION_SSH_KEYS.to_string()),
        ("ROOT_SSH_DIR", ROOT_SSH_DIR.to_string()),
        ("BOOT_INJECTION_DIR", BOOT_INJECTION_DIR.to_string()),
        ("BOOT_INJECTION_ENV", BOOT_INJECTION_ENV.to_string()),
        ("ROOTFS_MISSING_MESSAGE", ROOTFS_MISSING_MESSAGE.to_string()),
        ("ROOTFS_PATH", format!("/{ROOTFS_ISO_PATH}")),
        ("LIVE_OVERLAY_IMAGE_PATH", format!("/{LIVE_OVERLAYFS_ISO_PATH}")),
        ("MEDIA_MOUNT", MEDIA_MOUNT.to_string()),
        ("ROOTFS_MOUNT", ROOTFS_MOUNT.to_string()),
        ("LIVE_OVERLAY_MOUNT", LIVE_OVERLAY_MOUNT.to_string()),
        ("LIVE_LOWERDIR", LIVE_LOWERDIR.to_string()),
        ("OVERLAY_DIR", OVERLAY_DIR.to_string()),
        ("NEWROOT", NEWROOT.to_string()),
        ("PROBE_MOUNT", PROBE_MOUNT.to_string()),
        ("BOOT_INJECTION_MEDIA_MOUNT", BOOT_INJECTION_MEDIA_MOUNT.to_string()),
        ("SWITCH_ROOT_INIT", SWITCH_ROOT_INIT.to_string()),
        ("MEDIA_DEADLINE_SECS", MEDIA_DEADLINE_SECS.to_string()),
        ("CMDLINE_EMERGENCY", CMDLINE_EMERGENCY.to_string()),
        ("FAIL_REBOOT_OPTION", FAIL_REBOOT_OPTION.to_string()),
        ("CMDLINE_BOOT_INJECTION_REQUIRED", CMDLINE_BOOT_INJECTION_REQUIRED.to_string()),
    ]
}
