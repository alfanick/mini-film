use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

use anyhow::{Context, Result, bail};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use mini_film::apply_grain_8bit;
use rayon::prelude::*;
use tempfile::Builder;
use walkdir::WalkDir;

use crate::app::export::{add_convert_thread_limit, finalize_output, validate_output_format};
use crate::app::profile::profile_from_xmp_quiet;
use crate::app::progress::format_duration;
use crate::app::raw::{raw_engine_step, run_convert_depth, run_raw_develop};
use crate::app::util::{remove_temp_file, time_of_day_seed};
use crate::cli::{ExportOptions, JpegSubsampling, RawEngine};

const SAMPLER_PARALLEL_PROFILES: usize = 2;

pub(crate) struct SamplerArgs {
    pub(crate) raw: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_level: u32,
    pub(crate) dcraw_args: Vec<String>,
    pub(crate) raw_engine: RawEngine,
    pub(crate) rawtherapee: PathBuf,
    pub(crate) camera_profile: Option<String>,
    pub(crate) dcraw: PathBuf,
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
}

/// Render a labeled contact sheet showing every resolvable XMP profile.
///
/// The sampler develops the RAW once to a neutral 16-bit TIFF, then reuses that
/// intermediate for every profile. Each XMP is resolved to a temporary Hald,
/// applied to the shared base TIFF, optionally grained, resized to a thumbnail,
/// and finally passed to `montage` with the complete profile path as the label
/// below the image.
pub(crate) fn run_sampler(args: SamplerArgs) -> Result<()> {
    validate_sampler_args(&args)?;

    let emulation_root = emulation_root(&args.profiles_root);
    let profiles = collect_xmp_profiles(&emulation_root)?;
    if profiles.is_empty() {
        bail!("no XMP files found under {}", emulation_root.display());
    }

    let temp_dir = Builder::new().prefix("mini-film-sampler-").tempdir()?;
    let base_tiff = temp_dir.path().join("base.tif");
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
    sampler.set_message("raw develop");
    let raw_progress = multi.add(ProgressBar::new(5));
    raw_progress.set_style(profile_progress_style());
    raw_progress.set_message(raw_engine_step(args.raw_engine));

    run_raw_develop(
        args.raw_engine,
        &args.rawtherapee,
        &args.dcraw,
        &args.dcraw_args,
        args.camera_profile.as_deref(),
        &args.raw,
        &base_tiff,
        true,
    )?;
    raw_progress.set_position(5);
    raw_progress.finish_and_clear();

    let started = Instant::now();
    let workers: Vec<_> = (0..SAMPLER_PARALLEL_PROFILES)
        .map(|index| {
            let bar = multi.add(ProgressBar::new(5));
            bar.set_style(profile_progress_style());
            bar.set_message(format!("worker {} waiting", index + 1));
            bar
        })
        .collect();
    let next_worker = AtomicUsize::new(0);
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
                    profile_progress.set_length(5);
                    profile_progress.set_position(0);
                    profile_progress.set_message("queued");
                    let profile_started = Instant::now();
                    let progress = SamplerProgress {
                        profile: profile_progress.clone(),
                        started: profile_started,
                    };
                    let result = render_profile_thumbnail(
                        &args,
                        temp_dir.path(),
                        &base_tiff,
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
    let montage_progress = multi.add(ProgressBar::new(1));
    montage_progress.set_style(profile_progress_style());
    montage_progress.set_message(format!("montage {} thumbnails", thumbs.len()));
    run_montage(
        &args.montage,
        &args.output,
        &thumbs,
        args.jpg_quality,
        args.progressive_jpeg,
    )?;
    montage_progress.set_position(1);
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
    base_tiff: &Path,
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

    let resolved =
        profile_from_xmp_quiet(profile, args.hald_level, &args.profiles_root, &profile_temp)
            .with_context(|| format!("resolving profile {}", profile.display()))?;
    let converted = profile_temp.join("converted-8.ppm");
    sampler_step(progress, 2, "hald");
    run_convert_depth(
        &args.convert,
        base_tiff,
        &resolved.hald_path,
        resolved.sharpening,
        &converted,
        Some(8),
    )?;

    let source = if !args.no_grain && resolved.grain.is_enabled() {
        sampler_step(progress, 3, "grain");
        let grained = profile_temp.join("grained-8.ppm");
        apply_grain_8bit(
            &converted,
            &grained,
            resolved.grain,
            sample_seed(base_seed, index, profile),
        )?;
        remove_temp_file(&converted)?;
        grained
    } else {
        sampler_step(progress, 3, "grain skipped");
        converted
    };

    sampler_step(progress, 4, "thumbnail");
    let thumb = profile_temp.join("thumb.jpg");
    finalize_output(&args.convert, &source, &thumb, export)?;
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
    progress.profile.set_position(position);
    progress.profile.set_message(format!(
        "{} ({})",
        step,
        format_duration(progress.started.elapsed())
    ));
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
