use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mini_film::{GrainSettings, apply_grain, apply_grain_8bit};
use tempfile::Builder;

use crate::app::export::{
    finalize_output, output_ext, validate_export_options, validate_output_format,
};
use crate::app::profile::{ResolvedProfile, normalize_name, resolve_profile};
use crate::app::progress::{ApplyProgress, progress_step};
use crate::app::raw::{run_convert_depth, run_raw_develop};
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

    apply_resolved(
        ApplyJob {
            raw: &args.raw,
            output: &args.output,
            rawtherapee: &args.rawtherapee,
            convert: &args.convert,
            keep_intermediate: args.keep_intermediate.as_deref(),
            no_grain: args.no_grain,
            export: &args.export,
            quiet: false,
        },
        &resolved,
        grain_seed,
        temp_dir.path(),
        None,
    )?;

    eprintln!(
        "wrote {} using {}",
        args.output.display(),
        resolved.hald_path.display()
    );
    Ok(())
}

/// Apply an already resolved profile to one RAW input.
///
/// The function owns the processing graph. It develops RAW to TIFF with
/// RawTherapee, applies the Hald, eagerly removes temporary files, optionally
/// renders grain in either 8-bit JPEG space or 16-bit TIFF space, and finally
/// exports to the requested output format while updating progress bars for
/// batch callers.
pub(crate) fn apply_resolved(
    job: ApplyJob<'_>,
    resolved: &ResolvedProfile,
    grain_seed: u64,
    temp_dir: &Path,
    progress: Option<&ApplyProgress<'_>>,
) -> Result<()> {
    validate_output_format(job.output)?;

    let grain_enabled = !job.no_grain && resolved.grain.is_enabled();

    let intermediate = job
        .keep_intermediate
        .map(Path::to_path_buf)
        .unwrap_or_else(|| temp_dir.join("rawtherapee.tif"));
    let cleanup_intermediate = job.keep_intermediate.is_none();
    let output_ext = output_ext(job.output)?;
    let jpeg_output = output_ext == "jpg" || output_ext == "jpeg";
    let converted = if grain_enabled && jpeg_output {
        temp_dir.join("converted-8.ppm")
    } else {
        temp_dir.join("converted.tif")
    };
    let final_source = if grain_enabled && !jpeg_output {
        temp_dir.join("grained.tif")
    } else {
        converted.clone()
    };

    progress_step(progress, 1, "rawtherapee");
    run_raw_develop(
        job.rawtherapee,
        &resolved.rawtherapee_profiles,
        job.raw,
        &intermediate,
        job.quiet,
    )?;
    progress_step(progress, 2, "hald");
    run_convert_depth(
        job.convert,
        &intermediate,
        &resolved.hald_path,
        &converted,
        (grain_enabled && jpeg_output).then_some(8),
    )?;
    if cleanup_intermediate {
        remove_temp_file(&intermediate)?;
    }

    if grain_enabled && jpeg_output {
        progress_step(progress, 3, "grain");
        let grained = temp_dir.join("grained-8.ppm");
        apply_grain_8bit(&converted, &grained, resolved.grain, grain_seed)?;
        remove_temp_file(&converted)?;
        if progress.is_none() {
            eprintln!(
                "applied grain amount={} size={} frequency={}",
                resolved.grain.amount, resolved.grain.size, resolved.grain.frequency
            );
        }
        progress_step(progress, 4, "jpeg export");
        finalize_output(job.convert, &grained, job.output, job.export)?;
        remove_temp_file(&grained)?;
    } else if final_source != converted {
        progress_step(progress, 3, "grain");
        apply_grain(&converted, &final_source, resolved.grain, grain_seed)?;
        remove_temp_file(&converted)?;
        if progress.is_none() {
            eprintln!(
                "applied grain amount={} size={} frequency={}",
                resolved.grain.amount, resolved.grain.size, resolved.grain.frequency
            );
        }
    }

    if !jpeg_output || !grain_enabled {
        progress_step(progress, 4, "export");
        finalize_output(job.convert, &final_source, job.output, job.export)?;
        if final_source != converted {
            remove_temp_file(&final_source)?;
        }
    }

    progress_step(progress, 5, "done");
    Ok(())
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
