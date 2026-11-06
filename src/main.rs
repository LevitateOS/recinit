//! recinit CLI - Rootless initramfs builder for Linux.
//!
//! Build initramfs images without root privileges or external tools like dracut.

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use recinit::{
    build_install_initramfs, build_tiny_initramfs, default_boot_devices, InstallConfig,
    ModulePreset, TinyConfig, DEFAULT_GZIP_LEVEL,
};

#[derive(Parser)]
#[command(name = "recinit")]
#[command(about = "Rootless initramfs builder for Linux")]
#[command(version)]
struct Cli {
    /// Quiet mode (suppress progress messages)
    #[arg(short, long)]
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

        /// Path to a custom init script template (default: the built-in one)
        #[arg(long)]
        template: Option<PathBuf>,

        /// Output path for initramfs
        #[arg(short, long, default_value = "initramfs.cpio.gz")]
        output: PathBuf,

        /// ISO volume label the init looks for to find the boot media
        /// (the variant's `identity.iso_label`)
        #[arg(long)]
        iso_label: String,

        /// Path to rootfs inside ISO
        #[arg(long, default_value = recinit::live::ROOTFS_ISO_PATH)]
        rootfs_path: String,

        /// Path to live overlay payload image inside ISO
        #[arg(long, default_value = recinit::live::LIVE_OVERLAYFS_ISO_PATH)]
        live_overlay_image_path: String,

        /// Set a template value (KEY=VALUE), e.g. BOOT_BANNER or PRODUCT_TAG
        #[arg(long = "set", value_name = "KEY=VALUE", value_parser = parse_key_value)]
        template_vars: Vec<(String, String)>,

        /// Gzip compression level (1-9)
        #[arg(long, default_value_t = DEFAULT_GZIP_LEVEL)]
        gzip_level: u32,

    },

    /// Build an install initramfs for booting from disk
    BuildInstall {
        /// Path to rootfs staging directory
        #[arg(long)]
        rootfs: PathBuf,

        /// Path to static busybox binary (emergency shell)
        #[arg(long)]
        busybox: PathBuf,

        /// The kernel's module tree (e.g., <rootfs>/usr/lib/modules/6.12.0)
        #[arg(long)]
        modules_dir: PathBuf,

        /// Output path for initramfs
        #[arg(short, long, default_value = "initramfs-installed.img")]
        output: PathBuf,

        /// Module preset (live, install) or comma-separated list
        #[arg(long, default_value = "install")]
        modules: String,

        /// Gzip compression level (1-9)
        #[arg(long, default_value_t = DEFAULT_GZIP_LEVEL)]
        gzip_level: u32,

        /// Skip firmware copying
        #[arg(long)]
        no_firmware: bool,
    },

    /// List available module presets
    Modules {
        /// Show modules for a specific preset
        #[arg(long)]
        preset: Option<String>,

        /// List all available presets
        #[arg(long)]
        list_presets: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let verbose = !cli.quiet;

    match cli.command {
        Commands::BuildTiny {
            modules_dir,
            busybox,
            template,
            output,
            iso_label,
            rootfs_path,
            live_overlay_image_path,
            template_vars,
            gzip_level,
        } => {
            let config = TinyConfig {
                modules_dir,
                busybox_path: busybox,
                template_path: template,
                output,
                boot_devices: default_boot_devices(),
                module_preset: ModulePreset::Live,
                iso_label,
                rootfs_path,
                live_overlay_image_path: Some(live_overlay_image_path),
                extra_template_vars: template_vars,
                gzip_level,
            };
            build_tiny_initramfs(&config, verbose)?;
        }

        Commands::BuildInstall {
            rootfs,
            busybox,
            modules_dir,
            output,
            modules,
            gzip_level,
            no_firmware,
        } => {
            let module_preset = parse_module_preset(&modules)?;

            let config = InstallConfig {
                rootfs,
                busybox_path: busybox,
                modules_dir,
                output,
                module_preset,
                gzip_level,
                include_firmware: !no_firmware,
            };

            build_install_initramfs(&config, verbose)?;
        }

        Commands::Modules {
            preset,
            list_presets,
        } => {
            if list_presets {
                println!("Available module presets:");
                println!(
                    "  live    - Minimal modules for live ISO boot ({} modules)",
                    ModulePreset::Live.names().len()
                );
                println!(
                    "  install - Full modules for installed systems ({} modules)",
                    ModulePreset::Install.names().len()
                );
                return Ok(());
            }

            let preset_name = preset.unwrap_or_else(|| "live".to_string());
            let modules = match ModulePreset::parse_name(&preset_name) {
                Some(preset) => preset.names(),
                _ => bail!(
                    "Unknown preset: {}. Use --list-presets to see available options.",
                    preset_name
                ),
            };

            println!(
                "Modules for '{}' preset ({} modules):",
                preset_name,
                modules.len()
            );
            for module in modules {
                println!("  {}", module);
            }
        }
    }

    Ok(())
}

fn parse_module_preset(s: &str) -> Result<ModulePreset> {
    if let Some(preset) = ModulePreset::parse_name(s) {
        return Ok(preset);
    }

    Ok(ModulePreset::Custom(
        s.split(',').map(|s| s.trim().to_string()).collect(),
    ))
}

fn parse_key_value(arg: &str) -> std::result::Result<(String, String), String> {
    let (key, value) = arg
        .split_once('=')
        .ok_or_else(|| format!("expected KEY=VALUE, got '{arg}'"))?;
    Ok((key.to_string(), value.to_string()))
}
