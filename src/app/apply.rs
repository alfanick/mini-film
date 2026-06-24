use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use mini_film::{
    GrainEngine, GrainSettings, apply_grain_8bit_with_engine, apply_grain_with_engine,
};
use tempfile::Builder;

use crate::app::export::{
    finalize_auto_oriented_output_with_retouch, finalize_output_with_retouch, output_ext,
    validate_export_options, validate_output_format,
};
use crate::app::pp3::{
    write_rawtherapee_color_noise_profile, write_rawtherapee_lens_corrections_profile,
};
use crate::app::profile::{ResolvedProfile, normalize_name, resolve_profile};
use crate::app::progress::{
    ApplyProgress, file_progress_style, progress_length, progress_stage_adaptive, progress_step,
};
use crate::app::raw::{run_raw_develop, run_raw_develop_jpeg};
use crate::app::retouch::{RetouchSettings, write_rawtherapee_retouch_profile};
use crate::app::util::{
    OutputEditMetadata, extract_capture_iso, is_jpeg_input_file, remove_temp_file,
    sync_output_metadata_from_image_with_color_profile,
    sync_output_metadata_from_raw_with_color_profile, sync_output_timestamps_from_exif,
    time_of_day_seed,
};
use crate::cli::{ExportOptions, LensCorrections};
use mini_film::rawtherapee_hald_clut_profile_text;

pub(crate) struct ApplyArgs {
    pub(crate) raw: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) profile: Option<String>,
    pub(crate) hald_dir: PathBuf,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_level: u32,
    pub(crate) rawtherapee: PathBuf,
    pub(crate) convert: PathBuf,
    pub(crate) keep_intermediate: Option<PathBuf>,
    pub(crate) no_grain: bool,
    pub(crate) color_noise_iso_threshold: u32,
    pub(crate) lens_corrections: LensCorrections,
    pub(crate) lcp_root: Option<PathBuf>,
    pub(crate) grain: Option<String>,
    pub(crate) grain_preset: Option<String>,
    pub(crate) grain_seed: Option<u64>,
    pub(crate) grain_engine: GrainEngine,
    pub(crate) export: ExportOptions,
    pub(crate) retouch: Option<RetouchSettings>,
}

pub(crate) struct ApplyJob<'a> {
    pub(crate) raw: &'a Path,
    pub(crate) output: &'a Path,
    pub(crate) rawtherapee: &'a Path,
    pub(crate) convert: &'a Path,
    pub(crate) keep_intermediate: Option<&'a Path>,
    pub(crate) no_grain: bool,
    pub(crate) grain_engine: GrainEngine,
    pub(crate) color_noise_iso_threshold: u32,
    pub(crate) lens_corrections: LensCorrections,
    pub(crate) lcp_root: Option<&'a Path>,
    pub(crate) export: &'a ExportOptions,
    pub(crate) quiet: bool,
    pub(crate) exif_comment: Option<String>,
    pub(crate) retouch: Option<&'a RetouchSettings>,
}

pub(crate) struct CompressedApplyJob<'a> {
    pub(crate) input: &'a Path,
    pub(crate) output: &'a Path,
    pub(crate) convert: &'a Path,
    pub(crate) export: &'a ExportOptions,
    pub(crate) exif_comment: Option<String>,
    pub(crate) retouch: Option<&'a RetouchSettings>,
}

fn exif_comment_for_command(command: &str, profile: Option<&str>) -> String {
    let profile = profile
        .map(str::trim)
        .filter(|profile| !profile.is_empty())
        .unwrap_or("none");
    format!(
        "mini-film {} usage={command} profile={profile}",
        env!("CARGO_PKG_VERSION")
    )
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

    if is_jpeg_input_file(&args.raw) {
        if args
            .profile
            .as_deref()
            .is_some_and(|profile| !profile.trim().is_empty())
        {
            bail!("--profile is only supported for RAW inputs");
        }
        if args.lens_corrections.is_enabled() {
            bail!("--lens-corrections is only supported for RAW inputs");
        }
        if args.grain.is_some() || args.grain_preset.is_some() {
            bail!("--grain and --grain-preset are only supported for RAW inputs");
        }

        let file = ProgressBar::new(progress_length());
        file.set_style(file_progress_style());
        file.set_message("starting");
        let started = std::time::Instant::now();
        let progress = ApplyProgress {
            file: &file,
            started,
            estimates: None,
        };
        let result = apply_compressed(
            CompressedApplyJob {
                input: &args.raw,
                output: &args.output,
                convert: &args.convert,
                export: &args.export,
                exif_comment: Some(exif_comment_for_command("apply", None)),
                retouch: args.retouch.as_ref(),
            },
            Some(&progress),
        );
        match &result {
            Ok(()) => file.finish_and_clear(),
            Err(_) => file.abandon_with_message("failed"),
        }
        result?;
        eprintln!(
            "wrote {} from compressed source {}",
            args.output.display(),
            args.raw.display()
        );
        return Ok(());
    }

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
            grain_engine: args.grain_engine,
            color_noise_iso_threshold: args.color_noise_iso_threshold,
            lens_corrections: args.lens_corrections,
            lcp_root: args.lcp_root.as_deref(),
            export: &args.export,
            quiet: true,
            exif_comment: Some(exif_comment_for_command("apply", args.profile.as_deref())),
            retouch: args.retouch.as_ref(),
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

    if args.profile.is_none() {
        eprintln!("wrote {} using RawTherapee defaults", args.output.display());
    } else if let Some(hald_path) = &resolved.hald_path {
        eprintln!(
            "wrote {} using {}",
            args.output.display(),
            hald_path.display()
        );
    } else {
        eprintln!("wrote {} using RawTherapee PP3", args.output.display());
    }
    Ok(())
}

pub(crate) fn apply_compressed(
    job: CompressedApplyJob<'_>,
    progress: Option<&ApplyProgress<'_>>,
) -> Result<()> {
    validate_output_format(job.output)?;

    progress_step(progress, 1, "compressed input");
    progress_step(progress, 3, "raw development skipped");

    let output_ext = output_ext(job.output)?;
    let jpeg_output = output_ext == "jpg" || output_ext == "jpeg";
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
    finalize_auto_oriented_output_with_retouch(
        job.convert,
        job.input,
        job.output,
        job.export,
        job.retouch,
    )?;
    export_stage.finish();

    if !job.export.strip_metadata {
        let exif_stage = progress_stage_adaptive(
            progress,
            5,
            6,
            "image-metadata",
            "metadata",
            estimate_exif_duration(job.input),
        );
        sync_output_metadata_from_image_with_color_profile(
            job.input,
            job.output,
            job.exif_comment.as_deref(),
            Some(job.input),
        )?;
        sync_output_timestamps_from_exif(job.input, job.output)?;
        exif_stage.finish();
    } else {
        let timestamp_stage = progress_stage_adaptive(
            progress,
            5,
            6,
            "timestamps",
            "timestamps",
            estimate_timestamp_sync_duration(),
        );
        sync_output_timestamps_from_exif(job.input, job.output)?;
        timestamp_stage.finish();
    }
    progress_step(progress, 6, "done");
    Ok(())
}

/// Compute whether sharpening and color-noise denoising are expected to be active for
/// a specific input before invoking expensive processing.
///
/// Sharpening is derived from resolved emulation metadata. Denoise reflects the
/// same threshold/ISO logic used by `with_optional_color_noise_profile`.
pub(crate) fn resolve_apply_effects(
    raw: &Path,
    resolved: &ResolvedProfile,
    color_noise_iso_threshold: u32,
) -> (bool, bool) {
    let denoise_applied = denoise_profile_applied(raw, color_noise_iso_threshold);
    (resolved.sharpening_applied, denoise_applied)
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

    let rawtherapee_profiles = rawtherapee_profiles_for_apply(resolved, temp_dir, job.retouch)?;
    let rawtherapee_profiles = with_optional_color_noise_profile(
        job.raw,
        &rawtherapee_profiles,
        temp_dir,
        job.color_noise_iso_threshold,
    )?;
    let rawtherapee_profiles = with_optional_lens_corrections_profile(
        &rawtherapee_profiles,
        temp_dir,
        job.lens_corrections,
    )?;
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
    let effective_lcp_root = if job.lens_corrections.is_enabled() {
        job.lcp_root
    } else {
        None
    };

    if jpeg_intermediate {
        run_raw_develop_jpeg(
            job.rawtherapee,
            &rawtherapee_profiles,
            job.raw,
            &intermediate,
            job.export.jpg_quality,
            job.export.jpeg_subsampling,
            effective_lcp_root,
            job.quiet,
        )?;
    } else {
        run_raw_develop(
            job.rawtherapee,
            &rawtherapee_profiles,
            job.raw,
            &intermediate,
            effective_lcp_root,
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
        apply_grain_8bit_with_engine(
            &intermediate,
            &grained,
            resolved.grain,
            grain_seed,
            job.grain_engine,
        )?;
        grain_stage.finish();
        if progress.is_none() {
            eprintln!(
                "applied grain amount={} size={} frequency={} engine={}",
                resolved.grain.amount,
                resolved.grain.size,
                resolved.grain.frequency,
                job.grain_engine
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
        finalize_output_with_retouch(job.convert, &grained, job.output, job.export, job.retouch)?;
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
        apply_grain_with_engine(
            &intermediate,
            &grained,
            resolved.grain,
            grain_seed,
            job.grain_engine,
        )?;
        grain_stage.finish();
        if progress.is_none() {
            eprintln!(
                "applied grain amount={} size={} frequency={} engine={}",
                resolved.grain.amount,
                resolved.grain.size,
                resolved.grain.frequency,
                job.grain_engine
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
        finalize_output_with_retouch(job.convert, &grained, job.output, job.export, job.retouch)?;
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
        finalize_output_with_retouch(
            job.convert,
            &intermediate,
            job.output,
            job.export,
            job.retouch,
        )?;
        export_stage.finish();
    }

    if !job.export.strip_metadata {
        let exif_stage = progress_stage_adaptive(
            progress,
            5,
            6,
            "exif-metadata",
            "exif",
            estimate_exif_duration(job.raw),
        );
        let actual_grain = if grain_enabled {
            resolved.grain
        } else {
            GrainSettings::default()
        };
        sync_output_metadata_from_raw_with_color_profile(
            job.raw,
            job.output,
            OutputEditMetadata {
                comment: job.exif_comment.as_deref(),
                profile: &resolved.metadata,
                grain: actual_grain,
                grain_seed: grain_enabled.then_some(grain_seed),
                grain_engine: grain_enabled.then_some(job.grain_engine),
            },
            Some(&intermediate),
        )?;
        sync_output_timestamps_from_exif(job.raw, job.output)?;
        exif_stage.finish();
    } else {
        let timestamp_stage = progress_stage_adaptive(
            progress,
            5,
            6,
            "timestamps",
            "timestamps",
            estimate_timestamp_sync_duration(),
        );
        sync_output_timestamps_from_exif(job.raw, job.output)?;
        timestamp_stage.finish();
    }
    if cleanup_intermediate {
        remove_temp_file(&intermediate)?;
    }
    progress_step(progress, 6, "done");
    Ok(())
}

fn rawtherapee_profiles_for_apply(
    resolved: &ResolvedProfile,
    temp_dir: &Path,
    retouch: Option<&RetouchSettings>,
) -> Result<Vec<PathBuf>> {
    let mut profiles = resolved.rawtherapee_profiles.clone();
    if let Some(retouch) = retouch
        && let Some(profile) = write_rawtherapee_retouch_profile(
            &temp_dir.join("retouch.pp3"),
            resolved.retouch_base,
            retouch,
        )?
    {
        profiles.push(profile);
    }
    if let Some(hald_path) = &resolved.hald_path {
        let lut_profile = temp_dir.join("rt-hald-clut.pp3");
        std::fs::write(&lut_profile, rawtherapee_hald_clut_profile_text(hald_path))
            .with_context(|| format!("writing {}", lut_profile.display()))?;
        profiles.push(lut_profile);
    }
    Ok(profiles)
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

fn with_optional_color_noise_profile(
    raw: &Path,
    base_profiles: &[PathBuf],
    temp_dir: &Path,
    threshold: u32,
) -> Result<Vec<PathBuf>> {
    let mut profiles = Vec::from(base_profiles);
    if threshold == 0 {
        return Ok(profiles);
    }

    let iso = match extract_capture_iso(raw)? {
        Some(iso) => iso,
        None => return Ok(profiles),
    };
    if iso < threshold {
        return Ok(profiles);
    }

    if let Some(path) =
        write_rawtherapee_color_noise_profile(&temp_dir.join("color-noise.pp3"), iso)?
    {
        profiles.push(path);
    }
    Ok(profiles)
}

fn denoise_profile_applied(raw: &Path, threshold: u32) -> bool {
    if threshold == 0 {
        return false;
    }

    let Ok(Some(iso)) = extract_capture_iso(raw) else {
        return false;
    };
    iso >= threshold
}

fn with_optional_lens_corrections_profile(
    base_profiles: &[PathBuf],
    temp_dir: &Path,
    lens_corrections: LensCorrections,
) -> Result<Vec<PathBuf>> {
    let mut profiles = Vec::from(base_profiles);
    if let Some(path) = write_rawtherapee_lens_corrections_profile(
        &temp_dir.join("lens-corrections.pp3"),
        lens_corrections,
    )? {
        profiles.push(path);
    }
    Ok(profiles)
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

fn estimate_exif_duration(raw: &Path) -> Duration {
    let mib = file_size_mib(raw).unwrap_or(2.0);
    let seconds = 0.15 + mib * 0.015;
    Duration::from_secs_f64(seconds.clamp(0.2, 2.0))
}

fn estimate_timestamp_sync_duration() -> Duration {
    Duration::from_millis(150)
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

#[cfg(test)]
mod tests {
    const RAWTHAPE_HELPER_SCRIPT: &str = include_str!("../../scripts/tests/rawtherapee_helper.sh");
    const CONVERT_HELPER_SCRIPT: &str = include_str!("../../scripts/tests/convert_helper.sh");

    use super::*;
    use crate::app::profile::ResolvedProfile;
    use crate::cli::ExportOptions;
    use image::{ImageBuffer, ImageFormat, Rgb};
    use std::{fs, io::Write, os::unix::fs::PermissionsExt};

    #[test]
    fn grain_override_accepts_tuple_presets_and_none() {
        let grain = resolve_grain_override(Some("10, 20,30"), None)
            .unwrap()
            .unwrap();
        assert_eq!(grain.amount, 10);
        assert_eq!(grain.size, 20);
        assert_eq!(grain.frequency, 30);

        let grain = resolve_grain_override(None, Some("heavy"))
            .unwrap()
            .unwrap();
        assert_eq!(grain.amount, 45);
        assert_eq!(grain.size, 60);
        assert_eq!(grain.frequency, 55);

        assert!(
            !resolve_grain_override(None, Some("none"))
                .unwrap()
                .unwrap()
                .is_enabled()
        );
    }

    #[test]
    fn grain_override_rejects_ambiguous_or_out_of_range_values() {
        assert!(resolve_grain_override(Some("1,2,3"), Some("light")).is_err());
        assert!(resolve_grain_override(Some("1,2"), None).is_err());
        assert!(resolve_grain_override(Some("1,2,101"), None).is_err());
        assert!(resolve_grain_override(None, Some("huge")).is_err());
    }

    #[test]
    fn duration_estimates_are_clamped_and_depend_on_output_path() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("raw.dng");
        fs::write(&raw, vec![0u8; 2 * 1024 * 1024]).unwrap();

        let jpeg = estimate_rawtherapee_duration(&raw, true);
        let tiff = estimate_rawtherapee_duration(&raw, false);
        assert!(jpeg >= Duration::from_secs(2));
        assert!(tiff >= jpeg);
        assert_eq!(estimate_export_duration(true), Duration::from_millis(900));
        assert_eq!(estimate_export_duration(false), Duration::from_secs(2));
    }

    #[test]
    fn missing_file_size_returns_none() {
        assert!(file_size_mib(Path::new("/definitely/missing/raw.dng")).is_none());
    }

    struct FakeRawtherapee {
        log: PathBuf,
    }

    fn write_fake_rawtherapee(path: &Path, output_image: &Path) -> Result<FakeRawtherapee> {
        let rendered = RAWTHAPE_HELPER_SCRIPT
            .replace(
                "__LOG_FILE__",
                &path.with_file_name("raw.log").display().to_string(),
            )
            .replace("__OUTPUT_IMAGE__", &output_image.display().to_string())
            .replace("__CREATE_OUTPUT__", "1")
            .replace("__EXIT_CODE__", "0");
        write_executable_script(path, &rendered)?;

        Ok(FakeRawtherapee {
            log: path.with_file_name("raw.log"),
        })
    }

    fn write_fake_convert(path: &Path) -> Result<PathBuf> {
        let rendered = CONVERT_HELPER_SCRIPT
            .replace(
                "__LOG_FILE__",
                &path.with_file_name("convert.log").display().to_string(),
            )
            .replace("__EXIT_CODE__", "0");
        write_executable_script(path, &rendered)?;
        Ok(path.with_file_name("convert.log"))
    }

    fn write_executable_script(path: &Path, rendered: &str) -> Result<()> {
        let temp_path = path.with_extension("tmp");
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(rendered.as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        let mut permissions = fs::metadata(&temp_path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&temp_path, permissions)?;
        fs::rename(&temp_path, path)?;
        Ok(())
    }

    fn make_source_image(path: &Path) {
        let image = ImageBuffer::from_fn(2, 2, |x, y| {
            if (x + y) % 2 == 0 {
                Rgb([32u8, 96, 160])
            } else {
                Rgb([64u8, 128, 192])
            }
        });
        let ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "jpg" | "jpeg" => image.save_with_format(path, ImageFormat::Jpeg).unwrap(),
            "tif" | "tiff" => image.save_with_format(path, ImageFormat::Tiff).unwrap(),
            _ => image.save(path).unwrap(),
        }
    }

    fn test_export_options() -> ExportOptions {
        ExportOptions {
            jpg_quality: 90,
            resize: None,
            long_edge: None,
            max_width: None,
            max_height: None,
            jpeg_subsampling: crate::cli::JpegSubsampling::S420,
            strip_metadata: false,
            progressive_jpeg: false,
        }
    }

    fn resolved_profile(grain: GrainSettings, hald: Option<PathBuf>) -> ResolvedProfile {
        ResolvedProfile {
            hald_path: hald,
            rawtherapee_profiles: Vec::new(),
            grain,
            sharpening_applied: false,
            resolved_stem: "profile".to_string(),
            retouch_base: Default::default(),
            metadata: crate::app::profile::ResolvedProfileMetadata {
                profile_name: "profile".to_string(),
                profile_uuid: None,
                look_name: None,
                look_uuid: None,
                source_profile_name: None,
                source_profile_uuid: None,
                hald_path: None,
                pp3_path: None,
                grain,
                source_adjustments: Default::default(),
                source_sharpening: Default::default(),
                emulation_adjustments: Default::default(),
                emulation_sharpening: Default::default(),
                has_camera_raw_settings: false,
            },
        }
    }

    #[test]
    fn apply_resolved_runs_jpeg_without_grain_and_calls_final_export() {
        let temp = tempfile::tempdir().unwrap();
        let raw = temp.path().join("frame.NEF");
        fs::write(&raw, b"raw").unwrap();
        let source_image = temp.path().join("rawtherapee.jpg");
        make_source_image(&source_image);

        let raw_log = write_fake_rawtherapee(&temp.path().join("rawtherapee"), &source_image)
            .unwrap()
            .log;
        let convert_log = write_fake_convert(&temp.path().join("convert")).unwrap();
        let out = temp.path().join("out.jpg");
        let resolved = resolved_profile(GrainSettings::default(), None);

        apply_resolved(
            ApplyJob {
                raw: &raw,
                output: &out,
                rawtherapee: &temp.path().join("rawtherapee"),
                convert: &temp.path().join("convert"),
                keep_intermediate: None,
                no_grain: true,
                grain_engine: GrainEngine::default(),
                color_noise_iso_threshold: 0,
                lens_corrections: LensCorrections::default(),
                lcp_root: None,
                export: &test_export_options(),
                quiet: true,
                exif_comment: Some("mini-film test".to_string()),
                retouch: None,
            },
            &resolved,
            0,
            temp.path(),
            None,
        )
        .unwrap();

        assert!(out.exists());
        let raw_invocation = fs::read_to_string(raw_log).unwrap();
        assert!(raw_invocation.contains("-j90"));
        assert!(raw_invocation.contains("-c"));
        let convert_invocation = fs::read_to_string(convert_log).unwrap();
        assert!(convert_invocation.contains(&out.to_string_lossy().to_string()));
    }

    #[test]
    fn apply_resolved_runs_tiff_with_grain_and_intermediate_grain_step() {
        let temp = tempfile::tempdir().unwrap();
        let raw = temp.path().join("frame.ARW");
        fs::write(&raw, b"raw").unwrap();
        let source_image = temp.path().join("rawtherapee.tif");
        make_source_image(&source_image);

        let raw_log = write_fake_rawtherapee(&temp.path().join("rawtherapee"), &source_image)
            .unwrap()
            .log;
        let convert_log = write_fake_convert(&temp.path().join("convert")).unwrap();
        let out = temp.path().join("out.tif");
        let resolved = resolved_profile(
            GrainSettings {
                amount: 20,
                size: 40,
                frequency: 40,
            },
            None,
        );

        apply_resolved(
            ApplyJob {
                raw: &raw,
                output: &out,
                rawtherapee: &temp.path().join("rawtherapee"),
                convert: &temp.path().join("convert"),
                keep_intermediate: None,
                no_grain: false,
                grain_engine: GrainEngine::default(),
                color_noise_iso_threshold: 0,
                lens_corrections: LensCorrections::default(),
                lcp_root: None,
                export: &test_export_options(),
                quiet: true,
                exif_comment: Some("mini-film test".to_string()),
                retouch: None,
            },
            &resolved,
            1,
            temp.path(),
            None,
        )
        .unwrap();

        assert!(out.exists());
        let raw_invocation = fs::read_to_string(raw_log).unwrap();
        assert!(raw_invocation.contains("-t"));
        let convert_invocation = fs::read_to_string(convert_log).unwrap();
        assert!(convert_invocation.contains(&out.to_string_lossy().to_string()));
    }

    #[test]
    fn apply_resolved_runs_grain_jpeg_path() {
        let temp = tempfile::tempdir().unwrap();
        let raw = temp.path().join("frame.CRF");
        fs::write(&raw, b"raw").unwrap();
        let source_image = temp.path().join("rawtherapee.jpg");
        make_source_image(&source_image);

        let raw_log = write_fake_rawtherapee(&temp.path().join("rawtherapee"), &source_image)
            .unwrap()
            .log;
        let convert_log = write_fake_convert(&temp.path().join("convert")).unwrap();
        let out = temp.path().join("out.jpeg");
        let resolved = resolved_profile(
            GrainSettings {
                amount: 25,
                size: 50,
                frequency: 55,
            },
            None,
        );

        let mut export = test_export_options();
        export.jpg_quality = 84;

        apply_resolved(
            ApplyJob {
                raw: &raw,
                output: &out,
                rawtherapee: &temp.path().join("rawtherapee"),
                convert: &temp.path().join("convert"),
                keep_intermediate: None,
                no_grain: false,
                grain_engine: GrainEngine::default(),
                color_noise_iso_threshold: 0,
                lens_corrections: LensCorrections::default(),
                lcp_root: None,
                export: &export,
                quiet: true,
                exif_comment: Some("mini-film test".to_string()),
                retouch: None,
            },
            &resolved,
            2,
            temp.path(),
            None,
        )
        .unwrap();

        assert!(out.exists());
        assert!(fs::read_to_string(raw_log).unwrap().contains("-j84"));
        assert!(
            fs::read_to_string(convert_log)
                .unwrap()
                .contains(&out.to_string_lossy().to_string())
        );
    }
}
