use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use mini_film::{apply_grain_8bit, write_rawtherapee_resize_profile};
use rayon::prelude::*;
use tempfile::Builder;
use walkdir::WalkDir;

use crate::app::export::{add_convert_thread_limit, finalize_output, validate_output_format};
use crate::app::profile::{profile_from_xmp_quiet, rawtherapee_profiles_with_hald};
use crate::app::progress::{
    ApplyProgress, StageEstimates, format_duration, progress_length, progress_position,
    progress_stage, progress_stage_adaptive, set_progress,
};
use crate::app::raw::run_raw_develop_jpeg;
use crate::app::util::{remove_temp_file, time_of_day_seed};
use crate::cli::{ExportOptions, JpegSubsampling};

const SAMPLER_PARALLEL_PROFILES: usize = 2;

pub(crate) struct SamplerArgs {
    pub(crate) raw: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_dir: PathBuf,
    pub(crate) hald_level: u32,
    pub(crate) rawtherapee: PathBuf,
    pub(crate) convert: PathBuf,
    pub(crate) montage: PathBuf,
    pub(crate) no_grain: bool,
    pub(crate) grain_seed: Option<u64>,
    pub(crate) thumbnail_long_edge: u32,
    pub(crate) jpg_quality: u8,
    pub(crate) jpeg_subsampling: JpegSubsampling,
    pub(crate) strip_metadata: bool,
    pub(crate) progressive_jpeg: bool,
}

struct SampleThumb {
    image: PathBuf,
    label: String,
}

struct SamplerProgress {
    profile: ProgressBar,
    started: Instant,
    estimates: Arc<StageEstimates>,
}

/// Render a labeled contact sheet showing every resolvable XMP profile.
///
/// Each XMP is resolved to a temporary Hald plus generated RawTherapee `.pp3`
/// profiles. The RAW is developed per profile so RawTherapee-side tone/color
/// settings and the Hald Film Simulation are reflected in the thumbnail, then
/// mini-film applies optional grain, final thumbnail sizing, and finally passes
/// everything to `montage` with a relative profile path as the label below the
/// image.
pub(crate) fn run_sampler(args: SamplerArgs) -> Result<()> {
    validate_sampler_args(&args)?;

    let emulation_root = emulation_root(&args.profiles_root);
    let profiles = collect_xmp_profiles(&emulation_root)?;
    if profiles.is_empty() {
        bail!("no XMP files found under {}", emulation_root.display());
    }

    let temp_dir = Builder::new().prefix("mini-film-sampler-").tempdir()?;
    let base_seed = args.grain_seed.unwrap_or_else(time_of_day_seed);
    let export = ExportOptions {
        jpg_quality: args.jpg_quality,
        resize: None,
        long_edge: Some(args.thumbnail_long_edge),
        max_width: None,
        max_height: None,
        jpeg_subsampling: args.jpeg_subsampling,
        strip_metadata: args.strip_metadata,
        progressive_jpeg: args.progressive_jpeg,
    };

    let multi = MultiProgress::new();
    let sampler = multi.add(ProgressBar::new(profiles.len() as u64));
    sampler.set_style(sampler_progress_style());
    sampler.set_message("starting");

    let started = Instant::now();
    let workers: Vec<_> = (0..SAMPLER_PARALLEL_PROFILES)
        .map(|index| {
            let bar = multi.add(ProgressBar::new(progress_length()));
            bar.set_style(profile_progress_style());
            bar.set_message(format!("worker {} waiting", index + 1));
            bar
        })
        .collect();
    let next_worker = AtomicUsize::new(0);
    let estimates = Arc::new(StageEstimates::default());
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(SAMPLER_PARALLEL_PROFILES)
        .build()?;
    let results: Vec<_> = pool.install(|| {
        profiles
            .par_iter()
            .enumerate()
            .map_init(
                || {
                    let worker =
                        next_worker.fetch_add(1, Ordering::Relaxed) % SAMPLER_PARALLEL_PROFILES;
                    workers[worker].clone()
                },
                |profile_progress, (index, profile)| {
                    sampler.set_message(
                        profile
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("<unknown>")
                            .to_string(),
                    );
                    profile_progress.set_length(progress_length());
                    profile_progress.set_position(0);
                    profile_progress.set_message("queued");
                    let profile_started = Instant::now();
                    let progress = SamplerProgress {
                        profile: profile_progress.clone(),
                        started: profile_started,
                        estimates: Arc::clone(&estimates),
                    };
                    let result = render_profile_thumbnail(
                        &args,
                        temp_dir.path(),
                        profile,
                        &emulation_root,
                        index,
                        base_seed,
                        &export,
                        &progress,
                    );
                    if result.is_err() {
                        profile_progress.set_message(format!(
                            "failed after {}",
                            format_duration(profile_started.elapsed())
                        ));
                    }
                    sampler.inc(1);
                    (profile.to_path_buf(), result)
                },
            )
            .collect()
    });
    for worker in &workers {
        worker.finish_and_clear();
    }
    let mut thumbs = Vec::new();
    let mut skipped = 0usize;
    for (profile, result) in results {
        match result {
            Ok(thumb) => thumbs.push(thumb),
            Err(err) => {
                skipped += 1;
                sampler.println(format!("skip {}: {err:#}", profile.display()));
            }
        }
    }
    sampler.set_position(profiles.len() as u64);

    if thumbs.is_empty() {
        sampler.abandon_with_message(format!(
            "no profiles rendered in {}",
            format_duration(started.elapsed())
        ));
        bail!(
            "no resolvable profiles found under {}",
            emulation_root.display()
        );
    }

    sampler.set_message("montage");
    let montage_progress = multi.add(ProgressBar::new(progress_length()));
    montage_progress.set_style(profile_progress_style());
    montage_progress.set_message(format!("montage {} thumbnails", thumbs.len()));
    let montage_started = Instant::now();
    let montage_apply_progress = ApplyProgress {
        file: &montage_progress,
        started: montage_started,
        estimates: None,
    };
    let montage_stage = progress_stage(
        Some(&montage_apply_progress),
        0,
        5,
        "montage",
        estimate_montage_duration(thumbs.len()),
    );
    run_montage(
        &args.montage,
        &args.output,
        &thumbs,
        args.jpg_quality,
        args.progressive_jpeg,
    )?;
    montage_stage.finish();
    montage_progress.finish_and_clear();
    sampler.finish_with_message(format!(
        "wrote {} thumbnails, skipped {} in {}",
        thumbs.len(),
        skipped,
        format_duration(started.elapsed())
    ));
    eprintln!("wrote {}", args.output.display());
    Ok(())
}

fn validate_sampler_args(args: &SamplerArgs) -> Result<()> {
    validate_output_format(&args.output)?;
    let ext = args
        .output
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if ext != "jpg" && ext != "jpeg" {
        bail!("sampler output must be .jpg or .jpeg");
    }
    if !args.profiles_root.is_dir() {
        bail!(
            "profiles root is not a directory: {}",
            args.profiles_root.display()
        );
    }
    if args.thumbnail_long_edge == 0 {
        bail!("--thumbnail-long-edge must be greater than zero");
    }
    Ok(())
}

fn collect_xmp_profiles(root: &Path) -> Result<Vec<PathBuf>> {
    let mut profiles = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry
            .path()
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("xmp"))
        {
            profiles.push(entry.path().to_path_buf());
        }
    }
    profiles.sort();
    Ok(profiles)
}

fn emulation_root(root: &Path) -> PathBuf {
    let direct = root.join("emulations");
    if direct.is_dir() {
        return direct;
    }
    if root
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("emulations"))
    {
        return root.to_path_buf();
    }
    if let Some(parent) = root
        .canonicalize()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
    {
        let sibling = parent.join("emulations");
        if sibling.is_dir() {
            return sibling;
        }
    }
    root.to_path_buf()
}

fn render_profile_thumbnail(
    args: &SamplerArgs,
    temp_root: &Path,
    profile: &Path,
    emulation_root: &Path,
    index: usize,
    base_seed: u64,
    export: &ExportOptions,
    progress: &SamplerProgress,
) -> Result<SampleThumb> {
    sampler_step(progress, 1, "resolve");
    let profile_temp = temp_root.join(format!("profile-{index}"));
    fs::create_dir_all(&profile_temp)
        .with_context(|| format!("creating {}", profile_temp.display()))?;

    let resolved = profile_from_xmp_quiet(
        profile,
        args.hald_level,
        &args.profiles_root,
        &args.hald_dir,
        &profile_temp,
    )
    .with_context(|| format!("resolving profile {}", profile.display()))?;
    let developed = profile_temp.join("rawtherapee.jpg");
    let mut rawtherapee_profiles = rawtherapee_profiles_with_hald(&resolved, &profile_temp)?;
    rawtherapee_profiles.push(write_rawtherapee_resize_profile(
        &profile_temp.join("resize.pp3"),
        args.thumbnail_long_edge,
    )?);
    let apply_progress = ApplyProgress {
        file: &progress.profile,
        started: progress.started,
        estimates: Some(Arc::clone(&progress.estimates)),
    };
    let raw_stage = progress_stage_adaptive(
        Some(&apply_progress),
        2,
        3,
        "sampler-rawtherapee",
        "rawtherapee",
        estimate_sampler_raw_duration(args.thumbnail_long_edge),
    );
    run_raw_develop_jpeg(
        &args.rawtherapee,
        &rawtherapee_profiles,
        &args.raw,
        &developed,
        args.jpg_quality,
        args.jpeg_subsampling,
        true,
    )?;
    raw_stage.finish();

    let source = if !args.no_grain && resolved.grain.is_enabled() {
        let grain_stage = progress_stage_adaptive(
            Some(&apply_progress),
            3,
            4,
            "sampler-grain",
            "grain",
            estimate_sampler_grain_duration(args.thumbnail_long_edge),
        );
        let grained = profile_temp.join("grained-8.ppm");
        apply_grain_8bit(
            &developed,
            &grained,
            resolved.grain,
            sample_seed(base_seed, index, profile),
        )?;
        grain_stage.finish();
        remove_temp_file(&developed)?;
        grained
    } else {
        sampler_step(progress, 3, "grain skipped");
        developed
    };

    let thumbnail_stage = progress_stage_adaptive(
        Some(&apply_progress),
        4,
        5,
        "sampler-thumbnail",
        "thumbnail",
        estimate_sampler_thumbnail_duration(args.thumbnail_long_edge),
    );
    let thumb = profile_temp.join("thumb.jpg");
    finalize_output(&args.convert, &source, &thumb, export)?;
    thumbnail_stage.finish();
    remove_temp_file(&source)?;

    let label = profile
        .strip_prefix(emulation_root)
        .unwrap_or(profile)
        .display()
        .to_string();
    let label = wrap_label(&label, label_width_chars(args.thumbnail_long_edge));
    let thumb = SampleThumb {
        image: thumb,
        label,
    };
    sampler_step(progress, 5, "done");
    Ok(thumb)
}

fn run_montage(
    montage: &Path,
    output: &Path,
    thumbs: &[SampleThumb],
    jpg_quality: u8,
    progressive_jpeg: bool,
) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let mut command = Command::new(montage);
    add_convert_thread_limit(&mut command);
    command
        .arg("-background")
        .arg("white")
        .arg("-fill")
        .arg("black")
        .arg("-tile")
        .arg("6x")
        .arg("-geometry")
        .arg("+12+36")
        .arg("-quality")
        .arg(jpg_quality.clamp(1, 100).to_string());
    if progressive_jpeg {
        command.arg("-interlace").arg("Line");
    }
    if let Some(font) = montage_font_path() {
        command.arg("-font").arg(font);
    }
    command.arg("-pointsize").arg("14");
    for thumb in thumbs {
        command.arg("-label").arg(&thumb.label).arg(&thumb.image);
    }
    command.arg(output);

    let status = command
        .status()
        .with_context(|| format!("running {}", montage.display()))?;
    if !status.success() {
        bail!("montage failed with status {status}");
    }
    Ok(())
}

fn sampler_progress_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{spinner:.green} sampler [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} {msg}",
    )
    .unwrap()
    .progress_chars("#>-")
}

fn profile_progress_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{spinner:.green} profile [{elapsed_precise}] [{wide_bar:.magenta/blue}] {pos}/{len} {msg}",
    )
    .unwrap()
    .progress_chars("#>-")
}

fn sampler_step(progress: &SamplerProgress, position: u64, step: &str) {
    set_progress(
        &progress.profile,
        progress.started,
        progress_position(position),
        step,
    );
}

fn estimate_sampler_raw_duration(thumbnail_long_edge: u32) -> Duration {
    let scale = (thumbnail_long_edge.max(128) as f64 / 512.0).sqrt();
    Duration::from_secs_f64((1.2 * scale).clamp(0.8, 5.0))
}

fn estimate_sampler_grain_duration(thumbnail_long_edge: u32) -> Duration {
    let pixels = thumbnail_long_edge.max(128) as f64;
    Duration::from_secs_f64((0.20 + pixels / 2400.0).clamp(0.25, 1.5))
}

fn estimate_sampler_thumbnail_duration(thumbnail_long_edge: u32) -> Duration {
    let pixels = thumbnail_long_edge.max(128) as f64;
    Duration::from_secs_f64((0.15 + pixels / 4000.0).clamp(0.2, 1.0))
}

fn estimate_montage_duration(thumbs: usize) -> Duration {
    Duration::from_secs_f64((0.5 + thumbs as f64 * 0.01).clamp(1.0, 20.0))
}

fn montage_font_path() -> Option<&'static str> {
    [
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
    ]
    .into_iter()
    .find(|path| Path::new(path).exists())
}

fn label_width_chars(thumbnail_long_edge: u32) -> usize {
    (thumbnail_long_edge as usize / 8).clamp(18, 96)
}

fn wrap_label(label: &str, width: usize) -> String {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_len = 0usize;
    for part in label.split_inclusive('/') {
        let part_len = part.chars().count();
        if !line.is_empty() && line_len + part_len > width {
            lines.push(line);
            line = String::new();
            line_len = 0;
        }

        if part_len <= width {
            line.push_str(part);
            line_len += part_len;
            continue;
        }

        if !line.is_empty() {
            lines.push(line);
            line = String::new();
            line_len = 0;
        }
        let mut chunk = String::new();
        for ch in part.chars() {
            chunk.push(ch);
            if chunk.chars().count() == width {
                lines.push(chunk);
                chunk = String::new();
            }
        }
        if !chunk.is_empty() {
            line = chunk;
            line_len = line.chars().count();
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines.join("\n")
}

fn sample_seed(base_seed: u64, index: usize, path: &Path) -> u64 {
    let path_hash = path
        .to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            hash.wrapping_mul(0x0000_0100_0000_01b3) ^ byte as u64
        });
    base_seed ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ path_hash
}
