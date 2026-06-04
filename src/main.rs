mod app;
mod cli;

use anyhow::Result;
use clap::Parser;

use crate::app::apply::{ApplyArgs, run_apply};
use crate::app::batch::{BatchArgs, run_batch};
use crate::app::run_hald;
use crate::app::util::configure_threads;
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
        } => run_hald(&input, &output, hald_level, overwrite, info_only),
        CommandKind::Apply {
            raw,
            output,
            profile,
            hald_dir,
            profiles_root,
            hald_level,
            dcraw_args,
            raw_engine,
            rawtherapee,
            camera_profile,
            dcraw,
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
            hald_dir,
            profiles_root,
            hald_level,
            dcraw_args,
            raw_engine,
            rawtherapee,
            camera_profile,
            dcraw,
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
            dcraw_args,
            raw_engine,
            rawtherapee,
            camera_profile,
            dcraw,
            convert,
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
        } => run_batch(BatchArgs {
            input,
            output,
            profile,
            hald_dir,
            profiles_root,
            hald_level,
            dcraw_args,
            raw_engine,
            rawtherapee,
            camera_profile,
            dcraw,
            convert,
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
    }
}
