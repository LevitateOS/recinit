//! recinit CLI - Rootless initramfs builder for Linux.
//!
//! Build initramfs images without root privileges or external tools like dracut.

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use recinit::{
    InitramfsBuilder, InstallConfig, ModulePreset, TinyConfig,
    INSTALL_MODULES, LIVE_MODULES,
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
        /// Path to kernel modules directory (e.g., /lib/modules/6.12.0)
        #[arg(long)]
        modules_dir: PathBuf,

        /// Path to static busybox binary
        #[arg(long)]
        busybox: PathBuf,

        /// Path to init script template
        #[arg(long)]
        template: PathBuf,

        /// Output path for initramfs
        #[arg(short, long, default_value = "initramfs.cpio.gz")]
        output: PathBuf,

        /// ISO volume label for boot device detection
        #[arg(long, default_value = "LINUX")]
        iso_label: String,

        /// Path to rootfs inside ISO (e.g., live/filesystem.erofs)
        #[arg(long, default_value = "live/filesystem.erofs")]
        rootfs_path: String,

        /// Path to live overlay inside ISO
        #[arg(long)]
        live_overlay_path: Option<String>,

        /// Gzip compression level (1-9)
        #[arg(long, default_value = "9")]
        gzip_level: u32,

        /// Skip checking for built-in modules
        #[arg(long)]
        no_builtin_check: bool,
    },

    /// Build an install initramfs for booting from disk
    BuildInstall {
        /// Path to rootfs staging directory
        #[arg(long)]
        rootfs: PathBuf,

        /// Optional path to kernel modules (if different from rootfs)
        #[arg(long)]
        modules_path: Option<PathBuf>,

        /// Output path for initramfs
        #[arg(short, long, default_value = "initramfs-installed.img")]
        output: PathBuf,

        /// Module preset (live, install) or comma-separated list
        #[arg(long, default_value = "install")]
        modules: String,

        /// Gzip compression level (1-9)
        #[arg(long, default_value = "9")]
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
            live_overlay_path,
            gzip_level,
            no_builtin_check,
        } => {
            let config = TinyConfig {
                modules_dir,
                busybox_path: busybox,
                template_path: template,
                output,
                iso_label,
                rootfs_path,
                live_overlay_path,
                gzip_level,
                check_builtin: !no_builtin_check,
                ..Default::default()
            };

            let builder = InitramfsBuilder::new().verbose(verbose);
            builder.build_tiny(config)?;
        }

        Commands::BuildInstall {
            rootfs,
            modules_path,
            output,
            modules,
            gzip_level,
            no_firmware,
        } => {
            let module_preset = parse_module_preset(&modules)?;

            let config = InstallConfig {
                rootfs,
                modules_path,
                output,
                module_preset,
                gzip_level,
                include_firmware: !no_firmware,
            };

            let builder = InitramfsBuilder::new().verbose(verbose);
            builder.build_install(config)?;
        }

        Commands::Modules {
            preset,
            list_presets,
        } => {
            if list_presets {
                println!("Available module presets:");
                println!("  live    - Minimal modules for live ISO boot ({} modules)", LIVE_MODULES.len());
                println!("  install - Full modules for installed systems ({} modules)", INSTALL_MODULES.len());
                return Ok(());
            }

            let preset_name = preset.unwrap_or_else(|| "live".to_string());
            let modules = match preset_name.as_str() {
                "live" | "tiny" => LIVE_MODULES,
                "install" | "full" => INSTALL_MODULES,
                _ => bail!("Unknown preset: {}. Use --list-presets to see available options.", preset_name),
            };

            println!("Modules for '{}' preset ({} modules):", preset_name, modules.len());
            for module in modules {
                println!("  {}", module);
            }
        }
    }

    Ok(())
}

/// Parse module preset from string.
fn parse_module_preset(s: &str) -> Result<ModulePreset> {
    // Check for preset names
    if let Some(preset) = ModulePreset::from_str(s) {
        return Ok(preset);
    }

    // Otherwise, treat as comma-separated custom list
    let modules: Vec<String> = s.split(',').map(|s| s.trim().to_string()).collect();
    if modules.is_empty() {
        bail!("No modules specified");
    }

    Ok(ModulePreset::Custom(modules))
}
