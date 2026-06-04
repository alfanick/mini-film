use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mini_film::{GrainSettings, apply_grain, apply_grain_8bit};
use tempfile::Builder;

use crate::app::export::{
    finalize_output, output_ext, validate_export_options, validate_output_format,
};
use crate::app::profile::{ResolvedProfile, normalize_name, resolve_profile};
use crate::app::progress::{ApplyProgress, progress_step};
use crate::app::raw::{
    raw_engine_step, run_convert_depth, run_dcraw_convert_final, run_raw_develop,
};
use crate::app::util::{remove_temp_file, time_of_day_seed};
use crate::cli::{ExportOptions, RawEngine};

pub(crate) struct ApplyArgs {
    pub(crate) raw: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) profile: String,
    pub(crate) hald_dir: PathBuf,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_level: u32,
    pub(crate) dcraw_args: Vec<String>,
    pub(crate) raw_engine: RawEngine,
    pub(crate) rawtherapee: PathBuf,
    pub(crate) camera_profile: Option<String>,
    pub(crate) dcraw: PathBuf,
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
    pub(crate) dcraw_args: &'a [String],
    pub(crate) raw_engine: RawEngine,
    pub(crate) rawtherapee: &'a Path,
    pub(crate) camera_profile: Option<&'a str>,
    pub(crate) dcraw: &'a Path,
    pub(crate) convert: &'a Path,
    pub(crate) keep_intermediate: Option<&'a Path>,
    pub(crate) no_grain: bool,
    pub(crate) export: &'a ExportOptions,
    pub(crate) quiet: bool,
}

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
            dcraw_args: &args.dcraw_args,
            raw_engine: args.raw_engine,
            rawtherapee: &args.rawtherapee,
            camera_profile: args.camera_profile.as_deref(),
            dcraw: &args.dcraw,
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

pub(crate) fn apply_resolved(
    job: ApplyJob<'_>,
    resolved: &ResolvedProfile,
    grain_seed: u64,
    temp_dir: &Path,
    progress: Option<&ApplyProgress<'_>>,
) -> Result<()> {
    validate_output_format(job.output)?;

    let grain_enabled = !job.no_grain && resolved.grain.is_enabled();

    if !grain_enabled && job.keep_intermediate.is_none() && job.raw_engine == RawEngine::Dcraw {
        let step = if resolved.sharpening.is_enabled() {
            "dcraw + hald/sharpen + export"
        } else {
            "dcraw + hald + export"
        };
        progress_step(progress, 1, step);
        run_dcraw_convert_final(
            job.dcraw,
            job.dcraw_args,
            job.camera_profile,
            job.raw,
            job.convert,
            &resolved.hald_path,
            resolved.sharpening,
            job.output,
            job.export,
        )?;
        progress_step(progress, 5, "done");
        return Ok(());
    }

    let intermediate = job
        .keep_intermediate
        .map(Path::to_path_buf)
        .unwrap_or_else(|| temp_dir.join("dcraw.tif"));
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

    progress_step(progress, 1, raw_engine_step(job.raw_engine));
    run_raw_develop(
        job.raw_engine,
        job.rawtherapee,
        job.dcraw,
        job.dcraw_args,
        job.camera_profile,
        job.raw,
        &intermediate,
        job.quiet,
    )?;
    progress_step(
        progress,
        2,
        if resolved.sharpening.is_enabled() {
            "hald/sharpen"
        } else {
            "hald"
        },
    );
    run_convert_depth(
        job.convert,
        &intermediate,
        &resolved.hald_path,
        resolved.sharpening,
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
