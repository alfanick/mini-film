use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use mini_film::{GrainSettings, apply_grain, apply_grain_8bit};
use tempfile::Builder;

use crate::app::export::{
    finalize_output, output_ext, validate_export_options, validate_output_format,
};
use crate::app::profile::{
    ResolvedProfile, normalize_name, rawtherapee_profiles_with_hald, resolve_profile,
};
use crate::app::progress::{
    ApplyProgress, file_progress_style, progress_length, progress_stage_adaptive, progress_step,
};
use crate::app::raw::{run_raw_develop, run_raw_develop_jpeg};
use crate::app::util::{remove_temp_file, time_of_day_seed};
use crate::cli::ExportOptions;

pub(crate) struct ApplyArgs {
    pub(crate) raw: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) profile: String,
    pub(crate) hald_dir: PathBuf,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_level: u32,
    pub(crate) rawtherapee: PathBuf,
    pub(crate) convert: PathBuf,
    pub(crate) keep_intermediate: Option<PathBuf>,
    pub(crate) no_grain: bool,
    pub(crate) grain: Option<String>,
    pub(crate) grain_preset: Option<String>,
    pub(crate) grain_seed: Option<u64>,
    pub(crate) export: ExportOptions,
}

pub(crate) struct ApplyJob<'a> {
    pub(crate) raw: &'a Path,
    pub(crate) output: &'a Path,
    pub(crate) rawtherapee: &'a Path,
    pub(crate) convert: &'a Path,
    pub(crate) keep_intermediate: Option<&'a Path>,
    pub(crate) no_grain: bool,
    pub(crate) export: &'a ExportOptions,
    pub(crate) quiet: bool,
}

/// Run the single-file apply command.
///
/// This validates output/export options, creates a temporary workspace, resolves
/// the selected profile into a Hald plus RawTherapee/grain metadata, applies any
/// explicit grain override, chooses a deterministic-or-time-based seed, and then
/// delegates the actual RAW/Hald/grain/export pipeline to `apply_resolved`.
pub(crate) fn run_apply(args: ApplyArgs) -> Result<()> {
    validate_output_format(&args.output)?;
    validate_export_options(&args.export)?;

    let temp_dir = Builder::new().prefix("mini-film-").tempdir()?;
    let mut resolved = resolve_profile(&args, temp_dir.path())?;
    if let Some(grain) =
        resolve_grain_override(args.grain.as_deref(), args.grain_preset.as_deref())?
    {
        resolved.grain = grain;
    }
    let grain_seed = args.grain_seed.unwrap_or_else(time_of_day_seed);

    let file = ProgressBar::new(progress_length());
    file.set_style(file_progress_style());
    file.set_message("starting");
    let started = std::time::Instant::now();
    let progress = ApplyProgress {
        file: &file,
        started,
        estimates: None,
    };

    let result = apply_resolved(
        ApplyJob {
            raw: &args.raw,
            output: &args.output,
            rawtherapee: &args.rawtherapee,
            convert: &args.convert,
            keep_intermediate: args.keep_intermediate.as_deref(),
            no_grain: args.no_grain,
            export: &args.export,
            quiet: true,
        },
        &resolved,
        grain_seed,
        temp_dir.path(),
        Some(&progress),
    );
    match &result {
        Ok(()) => file.finish_and_clear(),
        Err(_) => file.abandon_with_message("failed"),
    }
    result?;

    eprintln!(
        "wrote {} using {}",
        args.output.display(),
        resolved.hald_path.display()
    );
    Ok(())
}

/// Apply an already resolved profile to one RAW input.
///
/// The function owns the processing graph. It develops RAW with RawTherapee
/// while applying generated `.pp3` adjustments and the Hald CLUT via Film
/// Simulation, using 8-bit JPEG intermediates for JPEG-bound outputs and 16-bit
/// TIFF intermediates for TIFF-bound outputs. It eagerly removes temporary
/// files, optionally renders grain in either 8-bit JPEG space or 16-bit TIFF
/// space, and finally exports to the requested output format while updating
/// progress bars for batch callers.
pub(crate) fn apply_resolved(
    job: ApplyJob<'_>,
    resolved: &ResolvedProfile,
    grain_seed: u64,
    temp_dir: &Path,
    progress: Option<&ApplyProgress<'_>>,
) -> Result<()> {
    validate_output_format(job.output)?;

    let grain_enabled = !job.no_grain && resolved.grain.is_enabled();
    let output_ext = output_ext(job.output)?;
    let jpeg_output = output_ext == "jpg" || output_ext == "jpeg";
    let jpeg_intermediate = jpeg_output && job.keep_intermediate.is_none();
    let intermediate = job
        .keep_intermediate
        .map(Path::to_path_buf)
        .unwrap_or_else(|| {
            if jpeg_intermediate {
                temp_dir.join("rawtherapee.jpg")
            } else {
                temp_dir.join("rawtherapee.tif")
            }
        });
    let cleanup_intermediate = job.keep_intermediate.is_none();

    let rawtherapee_profiles = rawtherapee_profiles_with_hald(resolved, temp_dir)?;
    let raw_stage = progress_stage_adaptive(
        progress,
        1,
        3,
        if jpeg_intermediate {
            "rawtherapee-jpeg"
        } else {
            "rawtherapee-tiff"
        },
        "rawtherapee",
        estimate_rawtherapee_duration(job.raw, jpeg_intermediate),
    );
    if jpeg_intermediate {
        run_raw_develop_jpeg(
            job.rawtherapee,
            &rawtherapee_profiles,
            job.raw,
            &intermediate,
            job.export.jpg_quality,
            job.export.jpeg_subsampling,
            job.quiet,
        )?;
    } else {
        run_raw_develop(
            job.rawtherapee,
            &rawtherapee_profiles,
            job.raw,
            &intermediate,
            job.quiet,
        )?;
    }
    raw_stage.finish();

    if grain_enabled && jpeg_output {
        let grain_stage = progress_stage_adaptive(
            progress,
            3,
            4,
            "grain-jpeg",
            "grain",
            estimate_grain_duration(job.raw, true),
        );
        let grained = temp_dir.join("grained-8.ppm");
        apply_grain_8bit(&intermediate, &grained, resolved.grain, grain_seed)?;
        grain_stage.finish();
        if progress.is_none() {
            eprintln!(
                "applied grain amount={} size={} frequency={}",
                resolved.grain.amount, resolved.grain.size, resolved.grain.frequency
            );
        }
        let export_stage = progress_stage_adaptive(
            progress,
            4,
            5,
            "export-jpeg",
            "jpeg export",
            estimate_export_duration(true),
        );
        finalize_output(job.convert, &grained, job.output, job.export)?;
        export_stage.finish();
        remove_temp_file(&grained)?;
    } else if grain_enabled {
        let grain_stage = progress_stage_adaptive(
            progress,
            3,
            4,
            "grain-tiff",
            "grain",
            estimate_grain_duration(job.raw, false),
        );
        let grained = temp_dir.join("grained.tif");
        apply_grain(&intermediate, &grained, resolved.grain, grain_seed)?;
        grain_stage.finish();
        if progress.is_none() {
            eprintln!(
                "applied grain amount={} size={} frequency={}",
                resolved.grain.amount, resolved.grain.size, resolved.grain.frequency
            );
        }
        let export_stage = progress_stage_adaptive(
            progress,
            4,
            5,
            "export-tiff",
            "export",
            estimate_export_duration(false),
        );
        finalize_output(job.convert, &grained, job.output, job.export)?;
        export_stage.finish();
        remove_temp_file(&grained)?;
    } else {
        progress_step(progress, 3, "grain skipped");
        let export_stage = progress_stage_adaptive(
            progress,
            4,
            5,
            if jpeg_output {
                "export-jpeg"
            } else {
                "export-tiff"
            },
            "export",
            estimate_export_duration(jpeg_output),
        );
        finalize_output(job.convert, &intermediate, job.output, job.export)?;
        export_stage.finish();
    }

    if cleanup_intermediate {
        remove_temp_file(&intermediate)?;
    }

    progress_step(progress, 5, "done");
    Ok(())
}

fn estimate_rawtherapee_duration(raw: &Path, jpeg_intermediate: bool) -> Duration {
    let mib = file_size_mib(raw).unwrap_or(45.0);
    let seconds = if jpeg_intermediate {
        1.2 + mib * 0.075
    } else {
        2.0 + mib * 0.11
    };
    Duration::from_secs_f64(seconds.clamp(2.0, 18.0))
}

fn estimate_grain_duration(raw: &Path, jpeg_output: bool) -> Duration {
    let mib = file_size_mib(raw).unwrap_or(45.0);
    let seconds = if jpeg_output {
        0.35 + mib * 0.018
    } else {
        0.9 + mib * 0.045
    };
    Duration::from_secs_f64(seconds.clamp(0.6, 8.0))
}

fn estimate_export_duration(jpeg_output: bool) -> Duration {
    if jpeg_output {
        Duration::from_millis(900)
    } else {
        Duration::from_secs(2)
    }
}

fn file_size_mib(path: &Path) -> Option<f64> {
    Some(fs::metadata(path).ok()?.len() as f64 / 1_048_576.0)
}

/// Resolve command-line grain overrides.
///
/// XMP grain is used by default, but users can override it with either an
/// explicit `amount,size,frequency` tuple or a named preset. The two override
/// forms are mutually exclusive, and `none`/`off` intentionally resolves to
/// disabled grain rather than an error.
pub(crate) fn resolve_grain_override(
    grain: Option<&str>,
    preset: Option<&str>,
) -> Result<Option<GrainSettings>> {
    match (grain, preset) {
        (Some(_), Some(_)) => bail!("use either --grain or --grain-preset, not both"),
        (Some(value), None) => Ok(Some(parse_grain(value)?)),
        (None, Some(value)) => Ok(Some(match normalize_name(value).as_str() {
            "none" | "off" => GrainSettings::default(),
            "light" => GrainSettings {
                amount: 18,
                size: 35,
                frequency: 40,
            },
            "medium" => GrainSettings {
                amount: 30,
                size: 45,
                frequency: 45,
            },
            "heavy" => GrainSettings {
                amount: 45,
                size: 60,
                frequency: 55,
            },
            _ => bail!("unknown grain preset {value:?}; use light, medium, heavy, or none"),
        })),
        (None, None) => Ok(None),
    }
}

fn parse_grain(value: &str) -> Result<GrainSettings> {
    let parts: Vec<_> = value.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        bail!("--grain must be amount,size,frequency, for example --grain 30,45,45");
    }
    Ok(GrainSettings {
        amount: parse_grain_part(parts[0], "amount")?,
        size: parse_grain_part(parts[1], "size")?,
        frequency: parse_grain_part(parts[2], "frequency")?,
    })
}

fn parse_grain_part(value: &str, name: &str) -> Result<u8> {
    let parsed: u16 = value
        .parse()
        .with_context(|| format!("invalid grain {name} value {value:?}"))?;
    if parsed > 100 {
        bail!("grain {name} must be in 0..100");
    }
    Ok(parsed as u8)
}
