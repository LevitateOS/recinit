//! recinit CLI - Rootless initramfs builder for Linux.
//!
//! Build initramfs images without root privileges or external tools like dracut.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use recinit::{build_install_initramfs, build_tiny_initramfs, InstallConfig, TinyConfig, DEFAULT_GZIP_LEVEL};

#[derive(Parser)]
#[command(name = "recinit")]
#[command(about = "Rootless initramfs builder for Linux")]
#[command(version)]
#[command(after_help = "Build an initramfs for an installed system:\n  recinit build-install --rootfs /mnt --busybox /path/to/busybox-static \\\n    --modules-dir /mnt/usr/lib/modules/<KERNEL-VERSION> --output initramfs-installed.img\n\nUse build-tiny for live ISO boot and build-install for an installed system.\nThe module tree must match the kernel you will boot. No root privileges needed.\nRun 'recinit <command> --help' for the required inputs.")]
struct Cli {
    /// Quiet mode (suppress progress messages)
    #[arg(short, long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build a tiny initramfs for live ISO boot
    BuildTiny {
        /// The kernel's module tree (e.g., /lib/modules/6.12.0)
        #[arg(long)]
        modules_dir: PathBuf,

        /// Path to static busybox binary
        #[arg(long)]
        busybox: PathBuf,

        /// Output path for initramfs
        #[arg(short, long, default_value = "initramfs.cpio.gz")]
        output: PathBuf,

        /// ISO volume label the init looks for to find the boot media
        /// (the variant's `identity.iso_label`)
        #[arg(long)]
        iso_label: String,

        /// The line the init prints first
        #[arg(long, default_value = "initramfs starting...")]
        boot_banner: String,

        /// Gzip compression level (1-9)
        #[arg(long, default_value_t = DEFAULT_GZIP_LEVEL)]
        gzip_level: u32,
    },

    /// Build the busybox initramfs that boots an installed system (either distro)
    BuildInstall {
        /// The installed system's rootfs (fsck and blkid come from it)
        #[arg(long)]
        rootfs: PathBuf,

        /// Path to static busybox binary
        #[arg(long)]
        busybox: PathBuf,

        /// The kernel's module tree (e.g., <rootfs>/usr/lib/modules/6.12.0)
        #[arg(long)]
        modules_dir: PathBuf,

        /// Output path for initramfs
        #[arg(short, long, default_value = recinit::live::INITRAMFS_INSTALLED_FILENAME)]
        output: PathBuf,

        /// Gzip compression level (1-9)
        #[arg(long, default_value_t = DEFAULT_GZIP_LEVEL)]
        gzip_level: u32,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let verbose = !cli.quiet;

    match cli.command {
        Commands::BuildTiny {
            modules_dir,
            busybox,
            output,
            iso_label,
            boot_banner,
            gzip_level,
        } => build_tiny_initramfs(
            &TinyConfig {
                modules_dir,
                busybox_path: busybox,
                output,
                iso_label,
                boot_banner,
                gzip_level,
            },
            verbose,
        ),
        Commands::BuildInstall {
            rootfs,
            busybox,
            modules_dir,
            output,
            gzip_level,
        } => build_install_initramfs(
            &InstallConfig {
                rootfs,
                busybox_path: busybox,
                modules_dir,
                output,
                gzip_level,
            },
            verbose,
        ),
    }
}
