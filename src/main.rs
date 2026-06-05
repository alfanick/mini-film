mod app;
mod cli;

use anyhow::Result;
use clap::Parser;

use crate::app::apply::{ApplyArgs, run_apply};
use crate::app::batch::{BatchArgs, run_batch};
use crate::app::info::{InfoArgs, run_info};
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

    match Cli::parse().command {
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
            montage,
            no_grain,
            grain_seed,
            thumbnail_long_edge,
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
            montage,
            no_grain,
            grain_seed,
            thumbnail_long_edge,
            jpg_quality,
            jpeg_subsampling,
            strip_metadata,
            progressive_jpeg,
        }),
    }
}
