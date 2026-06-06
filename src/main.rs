mod app;
mod cli;

use std::{
    env,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;

use crate::app::apply::{ApplyArgs, run_apply};
use crate::app::batch::{BatchArgs, run_batch};
use crate::app::info::{InfoArgs, run_info};
use crate::app::nikon::{NikonArgs, run_nikon};
use crate::app::pp3::{Pp3Args, run_pp3};
use crate::app::run_hald;
use crate::app::sampler::{SamplerArgs, run_sampler};
use crate::app::util::{configure_threads, default_hald_dir};
use crate::cli::{Cli, CommandKind, ExportOptions};

/// Parse CLI arguments and dispatch to the selected mini-film workflow.
///
/// The top-level binary keeps clap-generated command shapes separate from the
/// runtime structs used by the application modules. It initializes the Rayon
/// thread pool once, then maps shared apply/batch flags into `ExportOptions` so
/// the downstream pipeline can handle single-file and batch processing through
/// the same conversion/export code.
fn main() -> Result<()> {
    configure_threads();

    let args = env::args().collect::<Vec<_>>();
    startup_dependency_check(&args)?;
    let cli = Cli::parse_from(&args);

    match cli.command {
        CommandKind::Hald {
            input,
            output,
            hald_level,
            overwrite,
            info_only,
        } => run_hald(
            &input,
            &output.unwrap_or_else(default_hald_dir),
            hald_level,
            overwrite,
            info_only,
        ),
        CommandKind::Info {
            profile,
            profiles_root,
            hald_dir,
            hald_level,
        } => run_info(InfoArgs {
            profile,
            profiles_root,
            hald_dir: hald_dir.unwrap_or_else(default_hald_dir),
            hald_level,
        }),
        CommandKind::Pp3 {
            profile,
            output,
            profiles_root,
            hald_dir,
            hald_level,
        } => run_pp3(Pp3Args {
            profile,
            output,
            profiles_root,
            hald_dir: hald_dir.unwrap_or_else(default_hald_dir),
            hald_level,
        }),
        CommandKind::Nikon {
            profile,
            output,
            report,
            name,
            profiles_root,
            hald_dir,
            hald_level,
        } => run_nikon(NikonArgs {
            profile,
            output,
            report,
            name,
            profiles_root,
            hald_dir: hald_dir.unwrap_or_else(default_hald_dir),
            hald_level,
        }),
        CommandKind::Apply {
            raw,
            output,
            profile,
            hald_dir,
            profiles_root,
            hald_level,
            rawtherapee,
            convert,
            keep_intermediate,
            no_grain,
            grain,
            grain_preset,
            grain_seed,
            jpg_quality,
            resize,
            long_edge,
            max_width,
            max_height,
            jpeg_subsampling,
            strip_metadata,
            progressive_jpeg,
        } => run_apply(ApplyArgs {
            raw,
            output,
            profile,
            hald_dir: hald_dir.unwrap_or_else(default_hald_dir),
            profiles_root,
            hald_level,
            rawtherapee,
            convert,
            keep_intermediate,
            no_grain,
            grain,
            grain_preset,
            grain_seed,
            export: ExportOptions {
                jpg_quality,
                resize,
                long_edge,
                max_width,
                max_height,
                jpeg_subsampling,
                strip_metadata,
                progressive_jpeg,
            },
        }),
        CommandKind::Batch {
            input,
            output,
            profile,
            hald_dir,
            profiles_root,
            hald_level,
            rawtherapee,
            convert,
            no_grain,
            grain,
            grain_preset,
            grain_seed,
            jobs,
            output_format,
            jpg_quality,
            resize,
            long_edge,
            max_width,
            max_height,
            jpeg_subsampling,
            strip_metadata,
            progressive_jpeg,
        } => run_batch(BatchArgs {
            input,
            output,
            profile,
            hald_dir: hald_dir.unwrap_or_else(default_hald_dir),
            profiles_root,
            hald_level,
            rawtherapee,
            convert,
            no_grain,
            grain,
            grain_preset,
            grain_seed,
            jobs,
            output_format,
            export: ExportOptions {
                jpg_quality,
                resize,
                long_edge,
                max_width,
                max_height,
                jpeg_subsampling,
                strip_metadata,
                progressive_jpeg,
            },
        }),
        CommandKind::Sampler {
            raw,
            output,
            profiles_root,
            hald_dir,
            hald_level,
            rawtherapee,
            convert,
            montage: _,
            no_grain,
            grain_seed,
            no_cache,
            jobs,
            thumbnail_long_edge,
            columns,
            jpg_quality,
            jpeg_subsampling,
            strip_metadata,
            progressive_jpeg,
        } => run_sampler(SamplerArgs {
            raw,
            output,
            profiles_root,
            hald_dir: hald_dir.unwrap_or_else(default_hald_dir),
            hald_level,
            rawtherapee,
            convert,
            no_grain,
            grain_seed,
            no_cache,
            jobs,
            thumbnail_long_edge,
            columns,
            jpg_quality,
            jpeg_subsampling,
            strip_metadata,
            progressive_jpeg,
        }),
    }
}

const RAWTHERAPEE_BINARY: &str = "rawtherapee-cli";
const CONVERT_BINARY: &str = "convert";

fn startup_dependency_check(args: &[String]) -> Result<()> {
    let command = active_command_for_dependency_check(args);
    let help_mode = is_help_mode(args);
    let needs_externals = match command {
        Some("apply") | Some("batch") | Some("sampler") => true,
        Some(_) => false,
        None => help_mode,
    };

    if !needs_externals {
        return Ok(());
    }

    let rawtherapee = resolve_dependency_path(args, "--rawtherapee", RAWTHERAPEE_BINARY);
    let convert = resolve_dependency_path(args, "--convert", CONVERT_BINARY);

    let mut failures = Vec::new();
    if let Err(error) = verify_dependency_binary("rawtherapee-cli", &rawtherapee) {
        failures.push(error.to_string());
    }
    if let Err(error) = verify_dependency_binary("convert", &convert) {
        failures.push(error.to_string());
    }
    if failures.is_empty() {
        return Ok(());
    }

    bail!("dependency check failed:\n  - {}", failures.join("\n  - "));
}

fn active_command_for_dependency_check(args: &[String]) -> Option<&str> {
    let mut positionals = args.iter().skip(1).filter(|arg| !arg.starts_with('-'));
    let first = positionals.next()?;
    if first == "help" {
        return positionals.next().map(String::as_str);
    }
    Some(first.as_str())
}

fn is_help_mode(args: &[String]) -> bool {
    args.len() == 1
        || args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--help" | "-h" | "help" | "--version" | "-V"))
}

fn resolve_dependency_path(args: &[String], flag: &str, default: &str) -> PathBuf {
    for index in 0..args.len() {
        let arg = &args[index];
        if arg == flag
            && let Some(value) = args.get(index + 1)
        {
            return PathBuf::from(value);
        }
        if let Some(value) = arg.strip_prefix(&format!("{flag}=")) {
            return PathBuf::from(value);
        }
    }
    PathBuf::from(default)
}

fn verify_dependency_binary(name: &str, path: &Path) -> Result<()> {
    Command::new(path)
        .arg("--help")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| {
            if matches!(err.kind(), ErrorKind::NotFound) {
                anyhow!("{} not found: {}", name, path.display())
            } else {
                anyhow!("{} is not executable: {}", name, err)
            }
        })
        .with_context(|| format!("running dependency probe for {name} at {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_command_for_dependency_check_unwraps_help_command_prefix() {
        let args = ["mini-film", "help", "sampler", "--rawtherapee", "/tmp/rt"]
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        assert_eq!(active_command_for_dependency_check(&args), Some("sampler"));
    }

    #[test]
    fn active_command_for_dependency_check_keeps_main_command() {
        let args = ["mini-film", "batch", "--convert", "/tmp/convert"]
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        assert_eq!(active_command_for_dependency_check(&args), Some("batch"));
    }

    #[test]
    fn resolve_dependency_path_parses_flags() {
        let args = [
            "mini-film",
            "apply",
            "--rawtherapee",
            "/tmp/rt",
            "--convert=/tmp/conv",
        ]
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
        assert_eq!(
            resolve_dependency_path(&args, "--rawtherapee", "rawtherapee-cli"),
            PathBuf::from("/tmp/rt")
        );
        assert_eq!(
            resolve_dependency_path(&args, "--convert", "convert"),
            PathBuf::from("/tmp/conv")
        );
    }
}
