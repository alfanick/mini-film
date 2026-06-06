use std::{
    collections::BTreeMap,
    env,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use handlebars::Handlebars;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use mini_film::{GrainSettings, apply_grain_8bit, write_rawtherapee_resize_profile};
use rayon::prelude::*;
use serde_json::json;
use sha1::{Digest, Sha1};
use tempfile::Builder;
use walkdir::WalkDir;

use crate::app::export::{add_convert_thread_limit, finalize_output, output_ext};
use crate::app::profile::{profile_from_xmp_quiet, rawtherapee_profiles_with_hald};
use crate::app::progress::{
    ApplyProgress, StageEstimates, format_duration, progress_length, progress_position,
    progress_stage, progress_stage_adaptive, set_progress,
};
use crate::app::raw::run_raw_develop_jpeg;
use crate::app::sampler_assets::{
    html_children_template, html_grid_template, html_page_template, html_script,
    html_section_template, html_styles, html_tile_template,
};
use crate::app::util::{half_cpu_thread_count, remove_temp_file, time_of_day_seed};
use crate::cli::{ExportOptions, JpegSubsampling};

pub(crate) struct SamplerArgs {
    pub(crate) raw: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_dir: PathBuf,
    pub(crate) hald_level: u32,
    pub(crate) rawtherapee: PathBuf,
    pub(crate) convert: PathBuf,
    pub(crate) no_grain: bool,
    pub(crate) grain_seed: Option<u64>,
    pub(crate) no_cache: bool,
    pub(crate) jobs: Option<usize>,
    pub(crate) thumbnail_long_edge: u32,
    pub(crate) columns: u32,
    pub(crate) jpg_quality: u8,
    pub(crate) jpeg_subsampling: JpegSubsampling,
    pub(crate) strip_metadata: bool,
    pub(crate) progressive_jpeg: bool,
}

struct SampleThumb {
    image: PathBuf,
    profile: PathBuf,
    pp3: Option<PathBuf>,
    hald: Option<PathBuf>,
    name: String,
    filename: String,
    parts: Vec<String>,
    width: u32,
    height: u32,
}

struct SheetEntry<'a> {
    label: String,
    full_name: String,
    sort_key: String,
    thumb: &'a SampleThumb,
}

#[derive(Default)]
struct ProfileTrie {
    thumbs: Vec<SampleThumb>,
    children: BTreeMap<String, ProfileTrie>,
}

struct SheetLayout {
    body: String,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SheetOutputKind {
    Jpeg,
    Tiff,
    Pdf,
    Html,
}

struct SamplerProgress {
    profile: ProgressBar,
    started: Instant,
    estimates: Arc<StageEstimates>,
}

struct ThumbnailCache {
    dir: PathBuf,
    raw_sha1: String,
}

/// Render a structured contact sheet showing every resolvable XMP profile.
///
/// Each XMP is resolved to a temporary Hald plus generated RawTherapee `.pp3`
/// profiles. The RAW is developed per profile so RawTherapee-side tone/color
/// settings and the Hald Film Simulation are reflected in the thumbnail. After
/// optional grain and final thumbnail sizing, mini-film groups profile names in
/// a trie and asks ImageMagick/GraphicsMagick `convert` to render an SVG sheet
/// where indentation shows each shared-name level.
pub(crate) fn run_sampler(args: SamplerArgs) -> Result<()> {
    validate_sampler_args(&args)?;
    let jobs = resolve_sampler_jobs(args.jobs)?;

    let emulation_root = emulation_root(&args.profiles_root);
    let profiles = collect_xmp_profiles(&emulation_root)?;
    if profiles.is_empty() {
        bail!("no XMP files found under {}", emulation_root.display());
    }

    let temp_dir = Builder::new().prefix("mini-film-sampler-").tempdir()?;
    let base_seed = args.grain_seed.unwrap_or_else(time_of_day_seed);
    let cache = if args.no_cache {
        None
    } else {
        Some(Arc::new(ThumbnailCache::new(&args.raw)?))
    };
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
    let workers: Vec<_> = (0..jobs)
        .map(|index| {
            let bar = multi.add(ProgressBar::new(progress_length()));
            bar.set_style(profile_progress_style());
            bar.set_message(format!("worker {} waiting", index + 1));
            bar
        })
        .collect();
    let next_worker = AtomicUsize::new(0);
    let estimates = Arc::new(StageEstimates::default());
    let pool = rayon::ThreadPoolBuilder::new().num_threads(jobs).build()?;
    let results: Vec<_> = pool.install(|| {
        profiles
            .par_iter()
            .enumerate()
            .map_init(
                || {
                    let worker = next_worker.fetch_add(1, Ordering::Relaxed) % jobs;
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
                        cache.as_deref(),
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

    sampler.set_message("sheet");
    let sheet_progress = multi.add(ProgressBar::new(progress_length()));
    sheet_progress.set_style(profile_progress_style());
    sheet_progress.set_message(format!("sheet {} thumbnails", thumbs.len()));
    let sheet_started = Instant::now();
    let sheet_apply_progress = ApplyProgress {
        file: &sheet_progress,
        started: sheet_started,
        estimates: None,
    };
    let sheet_stage = progress_stage(
        Some(&sheet_apply_progress),
        0,
        5,
        "sheet",
        estimate_sheet_duration(thumbs.len()),
    );
    run_structured_sheet(
        &args.convert,
        &args.output,
        &thumbs,
        &args.profiles_root,
        &args.hald_dir,
        args.hald_level,
        args.thumbnail_long_edge,
        args.columns,
        args.jpg_quality,
        args.jpeg_subsampling,
        args.progressive_jpeg,
    )?;
    sheet_stage.finish();
    sheet_progress.finish_and_clear();
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
    if sampler_output_kind(&args.output)?.is_none() {
        bail!("sampler output must be .jpg, .jpeg, .tif, .tiff, .pdf, or .html");
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
    if args.columns == 0 {
        bail!("--columns must be greater than zero");
    }
    Ok(())
}

impl ThumbnailCache {
    fn new(raw: &Path) -> Result<Self> {
        let raw_sha1 = sha1_file(raw).with_context(|| format!("hashing RAW {}", raw.display()))?;
        let dir = env::temp_dir().join("mini-film-sampler-cache");
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        Ok(Self { dir, raw_sha1 })
    }

    fn path_for(&self, profile: &Path, args: &SamplerArgs) -> Result<PathBuf> {
        let xmp_sha1 =
            sha1_file(profile).with_context(|| format!("hashing XMP {}", profile.display()))?;
        let grain_mode = if args.no_grain { "nograin" } else { "grain" };
        let subsampling = format!("{:?}", args.jpeg_subsampling).to_ascii_lowercase();
        Ok(self.dir.join(format!(
            "{}-{}-l{}-{}px-q{}-{}-{}-sg2-strip{}-prog{}.jpg",
            self.raw_sha1,
            xmp_sha1,
            args.hald_level,
            args.thumbnail_long_edge,
            args.jpg_quality,
            subsampling,
            grain_mode,
            args.strip_metadata as u8,
            args.progressive_jpeg as u8,
        )))
    }
}

fn sha1_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("reading {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn sample_thumb_from_image(
    image: PathBuf,
    profile: &Path,
    emulation_root: &Path,
) -> Result<SampleThumb> {
    let (width, height) =
        image::image_dimensions(&image).with_context(|| format!("reading {}", image.display()))?;
    let relative = profile
        .strip_prefix(emulation_root)
        .unwrap_or(profile)
        .display()
        .to_string();
    let name = profile_display_name_from_relative(&relative);
    let parts = profile_name_parts(&name);
    Ok(SampleThumb {
        image,
        profile: profile.to_path_buf(),
        pp3: None,
        hald: None,
        name,
        filename: relative,
        parts,
        width,
        height,
    })
}

fn sampler_output_kind(output: &Path) -> Result<Option<SheetOutputKind>> {
    Ok(match output_ext(output)?.as_str() {
        "jpg" | "jpeg" => Some(SheetOutputKind::Jpeg),
        "tif" | "tiff" => Some(SheetOutputKind::Tiff),
        "pdf" => Some(SheetOutputKind::Pdf),
        "html" | "htm" => Some(SheetOutputKind::Html),
        _ => None,
    })
}

fn resolve_sampler_jobs(jobs: Option<usize>) -> Result<usize> {
    let jobs = jobs.unwrap_or_else(half_cpu_thread_count);
    if jobs == 0 {
        bail!("--jobs must be at least 1");
    }
    Ok(jobs)
}

fn collect_xmp_profiles(root: &Path) -> Result<Vec<PathBuf>> {
    let mut profiles = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(Result::ok)
    {
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
    cache: Option<&ThumbnailCache>,
    progress: &SamplerProgress,
) -> Result<SampleThumb> {
    if let Some(cache) = cache {
        let cached = cache.path_for(profile, args)?;
        if cached.is_file() {
            sampler_step(progress, 5, "cache hit");
            return sample_thumb_from_image(cached, profile, emulation_root);
        }
    }

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
        let grain = scale_sampler_grain(resolved.grain, args.thumbnail_long_edge);
        apply_grain_8bit(
            &developed,
            &grained,
            grain,
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
    let image = if let Some(cache) = cache {
        let cached = cache.path_for(profile, args)?;
        copy_thumbnail_to_cache(&thumb, &cached)?;
        cached
    } else {
        thumb
    };
    sampler_step(progress, 5, "done");
    sample_thumb_from_image(image, profile, emulation_root)
}

fn copy_thumbnail_to_cache(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let temp = destination.with_extension("jpg.tmp");
    fs::copy(source, &temp)
        .with_context(|| format!("copying {} to {}", source.display(), temp.display()))?;
    match fs::rename(&temp, destination) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            remove_temp_file(&temp)?;
            Ok(())
        }
        Err(err) => Err(err)
            .with_context(|| format!("renaming {} to {}", temp.display(), destination.display())),
    }
}

fn run_structured_sheet(
    convert: &Path,
    output: &Path,
    thumbs: &[SampleThumb],
    profiles_root: &Path,
    hald_dir: &Path,
    hald_level: u32,
    thumbnail_long_edge: u32,
    columns: u32,
    jpg_quality: u8,
    jpeg_subsampling: JpegSubsampling,
    progressive_jpeg: bool,
) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let output_kind = sampler_output_kind(output)?.context("unsupported sampler output format")?;
    if output_kind == SheetOutputKind::Html {
        return run_html_sheet(
            convert,
            output,
            thumbs,
            columns,
            jpg_quality,
            jpeg_subsampling,
            hald_level,
            profiles_root,
            hald_dir,
        );
    }

    let mut trie = ProfileTrie::default();
    for thumb in thumbs {
        trie.insert(thumb.clone_for_tree());
    }
    let layout = build_sheet_layout(&trie, thumbnail_long_edge, columns, output_kind);
    let svg = render_sheet_svg(&layout);
    let svg_path = output.with_extension("mini-film-sampler.svg");
    fs::write(&svg_path, svg).with_context(|| format!("writing {}", svg_path.display()))?;

    let mut command = Command::new(convert);
    add_convert_thread_limit(&mut command);
    command.arg(&svg_path);
    match output_kind {
        SheetOutputKind::Jpeg => {
            command
                .arg("-quality")
                .arg(jpg_quality.clamp(1, 100).to_string());
            if progressive_jpeg {
                command.arg("-interlace").arg("Line");
            }
        }
        SheetOutputKind::Tiff => {
            command.arg("-compress").arg("Zip");
        }
        SheetOutputKind::Pdf | SheetOutputKind::Html => {}
    }
    command.arg(output);

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output_result = command
        .output()
        .with_context(|| format!("running {}", convert.display()))?;
    if !output_result.status.success() {
        let stdout = String::from_utf8_lossy(&output_result.stdout);
        let stderr = String::from_utf8_lossy(&output_result.stderr);
        bail!(
            "structured sampler sheet export failed with status {}\nsvg kept at {}\nstdout:\n{}\nstderr:\n{}",
            output_result.status,
            svg_path.display(),
            stdout.trim(),
            stderr.trim()
        );
    }
    let _ = fs::remove_file(&svg_path);
    Ok(())
}

fn run_html_sheet(
    convert: &Path,
    output: &Path,
    thumbs: &[SampleThumb],
    columns: u32,
    jpg_quality: u8,
    jpeg_subsampling: JpegSubsampling,
    hald_level: u32,
    profiles_root: &Path,
    hald_dir: &Path,
) -> Result<()> {
    let output_dir = output.parent().unwrap_or_else(|| Path::new("."));
    let thumbnail_dir = output_dir.join("thumbnails");
    let pp3_dir = output_dir.join("pp3");
    fs::create_dir_all(&thumbnail_dir)
        .with_context(|| format!("creating {}", thumbnail_dir.display()))?;
    fs::create_dir_all(&pp3_dir).with_context(|| format!("creating {}", pp3_dir.display()))?;

    let jobs = half_cpu_thread_count();
    let pool = rayon::ThreadPoolBuilder::new().num_threads(jobs).build()?;
    let mut html_thumbs: Vec<(usize, SampleThumb)> = pool.install(|| {
        thumbs
            .par_iter()
            .enumerate()
            .map(|(index, thumb)| {
                let file_name = html_thumbnail_file_name(index, thumb);
                let destination = thumbnail_dir.join(&file_name);
                write_cached_progressive_html_thumbnail(
                    convert,
                    &thumb.image,
                    &destination,
                    jpg_quality,
                    jpeg_subsampling,
                )?;
                let mut exported = thumb.clone_for_tree();
                exported.image = PathBuf::from("thumbnails").join(file_name);
                Ok::<_, anyhow::Error>((index, exported))
            })
            .collect::<Result<Vec<_>>>()
    })?;
    html_thumbs.sort_by_key(|(index, _)| *index);

    let mut sidecar_names = BTreeMap::new();
    let mut sidecar_exports = Vec::with_capacity(html_thumbs.len());
    for (html_index, (_, thumb)) in html_thumbs.iter_mut().enumerate() {
        let stem = unique_html_sidecar_stem(&mut sidecar_names, &thumb);
        let pp3_name = format!("{stem}.pp3");
        let pp3_output = pp3_dir.join(&pp3_name);
        sidecar_exports.push((html_index, thumb.profile.clone(), pp3_output));
        thumb.pp3 = Some(PathBuf::from("pp3").join(pp3_name));
    }
    let mut hald_links = pool.install(|| {
        sidecar_exports
            .par_iter()
            .map(|(html_index, profile, pp3_output)| {
                let hald = write_html_sampler_sidecars(
                    profile,
                    hald_level,
                    profiles_root,
                    hald_dir,
                    pp3_output,
                )
                .with_context(|| format!("exporting sampler sidecars for {}", profile.display()))?;
                Ok::<_, anyhow::Error>((*html_index, hald))
            })
            .collect::<Result<Vec<_>>>()
    })?;
    hald_links.sort_by_key(|(html_index, _)| *html_index);
    for (html_index, hald) in hald_links {
        html_thumbs[html_index].1.hald = Some(hald);
    }

    let mut trie = ProfileTrie::default();
    for (_, thumb) in html_thumbs {
        trie.insert(thumb);
    }
    let html = render_sheet_html(&trie, columns)?;
    fs::write(output, html).with_context(|| format!("writing {}", output.display()))?;
    Ok(())
}

fn write_html_sampler_sidecars(
    profile: &Path,
    hald_level: u32,
    profiles_root: &Path,
    hald_dir: &Path,
    pp3_output: &Path,
) -> Result<PathBuf> {
    let temp_dir = Builder::new().prefix("mini-film-sampler-pp3-").tempdir()?;
    let resolved = profile_from_xmp_quiet(
        profile,
        hald_level,
        profiles_root,
        hald_dir,
        temp_dir.path(),
    )?;
    let hald_path = resolved
        .hald_path
        .as_ref()
        .context("resolved emulation did not produce a HALD path")?;
    let rawtherapee_profiles = rawtherapee_profiles_with_hald(&resolved, temp_dir.path())?;
    let mut text = String::new();
    for profile in rawtherapee_profiles {
        text.push_str(
            &fs::read_to_string(&profile)
                .with_context(|| format!("reading generated PP3 {}", profile.display()))?,
        );
        if !text.ends_with('\n') {
            text.push('\n');
        }
    }
    fs::write(pp3_output, text).with_context(|| format!("writing {}", pp3_output.display()))?;
    Ok(hald_path.clone())
}

fn unique_html_sidecar_stem(names: &mut BTreeMap<String, usize>, thumb: &SampleThumb) -> String {
    let base = html_pp3_file_stem(thumb);
    let count = names.entry(base.clone()).or_insert(0);
    let stem = if *count == 0 {
        base
    } else {
        format!("{base}-{}", *count + 1)
    };
    *count += 1;
    stem
}

fn html_pp3_file_stem(thumb: &SampleThumb) -> String {
    let stem = sanitize_filename::sanitize(profile_display_name_from_relative(&thumb.filename))
        .into_owned();
    if stem.is_empty() {
        "profile".to_string()
    } else {
        stem
    }
}

fn write_cached_progressive_html_thumbnail(
    convert: &Path,
    source: &Path,
    destination: &Path,
    jpg_quality: u8,
    jpeg_subsampling: JpegSubsampling,
) -> Result<()> {
    let cached = progressive_html_cache_path(source, jpg_quality, jpeg_subsampling)?;
    if cached.is_file() {
        copy_file(&cached, destination)?;
        return Ok(());
    }

    write_progressive_html_thumbnail(convert, source, &cached, jpg_quality, jpeg_subsampling)?;
    copy_file(&cached, destination)?;
    Ok(())
}

fn progressive_html_cache_path(
    source: &Path,
    jpg_quality: u8,
    jpeg_subsampling: JpegSubsampling,
) -> Result<PathBuf> {
    let source_sha1 =
        sha1_file(source).with_context(|| format!("hashing thumbnail {}", source.display()))?;
    let subsampling = format!("{:?}", jpeg_subsampling).to_ascii_lowercase();
    let dir = env::temp_dir()
        .join("mini-film-sampler-cache")
        .join("html-progressive");
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir.join(format!(
        "{source_sha1}-q{}-{subsampling}-progressive.jpg",
        jpg_quality.clamp(1, 100)
    )))
}

fn copy_file(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::copy(source, destination)
        .with_context(|| format!("copying {} to {}", source.display(), destination.display()))?;
    Ok(())
}

fn write_progressive_html_thumbnail(
    convert: &Path,
    source: &Path,
    destination: &Path,
    jpg_quality: u8,
    jpeg_subsampling: JpegSubsampling,
) -> Result<()> {
    let mut command = Command::new(convert);
    add_convert_thread_limit(&mut command);
    command
        .arg(source)
        .arg("-interlace")
        .arg("Line")
        .arg("-sampling-factor")
        .arg(jpeg_subsampling.graphicsmagick_sampling_factor())
        .arg("-quality")
        .arg(jpg_quality.clamp(1, 100).to_string())
        .arg(destination);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = command
        .output()
        .with_context(|| format!("running {}", convert.display()))?;
    if !output.status.success() {
        bail!(
            "HTML thumbnail export failed with status {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn html_thumbnail_file_name(index: usize, thumb: &SampleThumb) -> String {
    let stem = sanitize_filename::sanitize(profile_display_name_from_relative(&thumb.filename))
        .into_owned();
    let stem = if stem.is_empty() {
        "profile".to_string()
    } else {
        stem
    };
    format!("{index:04}-{stem}.jpg")
}

fn render_sheet_html(trie: &ProfileTrie, columns: u32) -> Result<String> {
    let templates = html_templates()?;
    let mut sections = String::new();
    for (part, child) in &trie.children {
        sections.push_str(&render_html_node(&templates, child, &[part.clone()], 0)?);
    }

    templates
        .render(
            "page",
            &json!({
                "columns": columns.max(1),
                "styles": html_styles(),
                "script": html_script(),
                "sections": sections,
                "version": env!("CARGO_PKG_VERSION"),
            }),
        )
        .context("rendering HTML sampler page")
}

fn html_templates() -> Result<Handlebars<'static>> {
    let mut templates = Handlebars::new();
    templates
        .register_template_string("page", html_page_template())
        .context("registering page template")?;
    templates
        .register_template_string("section", html_section_template())
        .context("registering section template")?;
    templates
        .register_template_string("grid", html_grid_template())
        .context("registering grid template")?;
    templates
        .register_template_string("tile", html_tile_template())
        .context("registering tile template")?;
    templates
        .register_template_string("children", html_children_template())
        .context("registering children template")?;
    Ok(templates)
}

fn render_html_node(
    templates: &Handlebars<'_>,
    node: &ProfileTrie,
    prefix: &[String],
    depth: usize,
) -> Result<String> {
    let depth_class = html_depth_class(depth);
    let title = prefix.join(" ");
    let key = html_branch_key(prefix);
    let mut body = String::new();

    if (depth >= 1 || subtree_depth(node) <= 2) && !contains_forced_branch(node) {
        let mut entries = Vec::new();
        collect_subtree_entries(node, prefix.len(), &mut entries);
        if !entries.is_empty() {
            body.push_str(&render_html_grid(templates, &entries)?);
        }
        return render_html_section(
            templates,
            depth_class,
            html_heading_tag(depth),
            &key,
            &title,
            body,
        );
    }

    let mut leaf_entries: Vec<_> = node
        .thumbs
        .iter()
        .map(|thumb| {
            sheet_entry(
                prefix.last().cloned().unwrap_or_else(|| thumb.name.clone()),
                thumb,
            )
        })
        .collect();
    leaf_entries.extend(
        node.children
            .iter()
            .filter(|(_, child)| child.children.is_empty() && !child.thumbs.is_empty())
            .flat_map(|(part, child)| {
                let label = child_variant_label(prefix, part);
                child
                    .thumbs
                    .iter()
                    .map(move |thumb| sheet_entry(label.clone(), thumb))
            }),
    );
    sort_sheet_entries_with_common_prefix(&mut leaf_entries, prefix.len());
    if !leaf_entries.is_empty() {
        body.push_str(&render_html_grid(templates, &leaf_entries)?);
    }

    let mut children = String::new();
    for (part, child) in &node.children {
        if child.children.is_empty() && !child.thumbs.is_empty() {
            continue;
        }
        let mut child_prefix = prefix.to_vec();
        child_prefix.push(part.clone());
        children.push_str(&render_html_node(
            templates,
            child,
            &child_prefix,
            depth + 1,
        )?);
    }
    if !children.is_empty() {
        body.push_str(
            &templates
                .render("children", &json!({ "children": children }))
                .context("rendering HTML sampler children")?,
        );
    }
    render_html_section(
        templates,
        depth_class,
        html_heading_tag(depth),
        &key,
        &title,
        body,
    )
}

fn render_html_section(
    templates: &Handlebars<'_>,
    depth_class: &str,
    heading_tag: &str,
    key: &str,
    title: &str,
    body: String,
) -> Result<String> {
    templates
        .render(
            "section",
            &json!({
                "depth_class": depth_class,
                "heading_tag": heading_tag,
                "branch_key": key,
                "title": title,
                "body": body,
            }),
        )
        .context("rendering HTML sampler section")
}

fn html_branch_key(prefix: &[String]) -> String {
    prefix.join("/")
}

fn html_heading_tag(depth: usize) -> &'static str {
    match depth {
        0 => "h2",
        1 => "h3",
        2 => "h4",
        3 => "h5",
        _ => "h6",
    }
}

fn file_url(path: &Path) -> String {
    let absolute = path.canonicalize().unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        }
    });
    format!("file://{}", url_escape_path(&absolute, true))
}

fn relative_url(path: &PathBuf) -> String {
    url_escape_path(path, true)
}

fn url_escape_path(path: &Path, keep_slashes: bool) -> String {
    let mut out = String::new();
    for byte in path.to_string_lossy().bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~')
            || (keep_slashes && byte == b'/');
        if keep {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn render_html_grid(templates: &Handlebars<'_>, entries: &[SheetEntry<'_>]) -> Result<String> {
    let mut tiles = String::new();
    for entry in entries {
        let image = entry.thumb.image.to_string_lossy();
        tiles.push_str(
            &templates
                .render(
                    "tile",
                    &json!({
                        "label": entry.label,
                        "filename": entry.full_name,
                        "image": image,
                        "xmp_href": file_url(&entry.thumb.profile),
                        "pp3_href": entry.thumb.pp3.as_ref().map(relative_url).unwrap_or_default(),
                        "hald_href": entry.thumb.hald.as_ref().map(|path| file_url(path)).unwrap_or_default(),
                    }),
                )
                .context("rendering HTML sampler tile")?,
        );
    }
    templates
        .render("grid", &json!({ "tiles": tiles }))
        .context("rendering HTML sampler grid")
}

fn html_depth_class(depth: usize) -> &'static str {
    match depth {
        0 => "depth-0",
        1 => "depth-1",
        2 => "depth-2",
        _ => "depth-deep",
    }
}

fn sampler_progress_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{spinner:.green} sampler [{elapsed_precise}] [{wide_bar:.cyan/blue}] {percent:>3}% {msg}",
    )
    .unwrap()
    .progress_chars("█▌░")
}

fn profile_progress_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{spinner:.green} profile [{wide_bar:.magenta/blue}] {percent:>3}% {msg:.40}",
    )
    .unwrap()
    .progress_chars("█▌░")
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

fn estimate_sheet_duration(thumbs: usize) -> Duration {
    Duration::from_secs_f64((0.5 + thumbs as f64 * 0.01).clamp(1.0, 20.0))
}

fn scale_sampler_grain(grain: GrainSettings, thumbnail_long_edge: u32) -> GrainSettings {
    if !grain.is_enabled() || thumbnail_long_edge >= 3000 {
        return grain;
    }

    let linear = (thumbnail_long_edge.max(1) as f32 / 3600.0).clamp(0.25, 1.0);
    let amount_scale = linear.sqrt().clamp(0.45, 1.0);
    let size_scale = linear.clamp(0.35, 1.0);

    GrainSettings {
        amount: scale_grain_byte(grain.amount, amount_scale),
        size: scale_grain_byte(grain.size, size_scale),
        frequency: grain.frequency,
    }
}

fn scale_grain_byte(value: u8, scale: f32) -> u8 {
    if value == 0 {
        0
    } else {
        ((value as f32 * scale).round() as u8).clamp(1, 100)
    }
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

impl SampleThumb {
    fn clone_for_tree(&self) -> Self {
        Self {
            image: self.image.clone(),
            profile: self.profile.clone(),
            pp3: self.pp3.clone(),
            hald: self.hald.clone(),
            name: self.name.clone(),
            filename: self.filename.clone(),
            parts: self.parts.clone(),
            width: self.width,
            height: self.height,
        }
    }
}

impl ProfileTrie {
    fn insert(&mut self, thumb: SampleThumb) {
        let mut node = self;
        for part in &thumb.parts {
            node = node.children.entry(part.clone()).or_default();
        }
        node.thumbs.push(thumb);
    }
}

fn profile_display_name_from_relative(relative: &str) -> String {
    let stem = Path::new(relative)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(relative);
    stem.trim().to_string()
}

fn profile_name_parts(name: &str) -> Vec<String> {
    let parts: Vec<_> = name
        .replace(['_', '-', '/', '.'], " ")
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if parts.is_empty() {
        vec!["Profile".to_string()]
    } else {
        parts
    }
}

fn build_sheet_layout(
    trie: &ProfileTrie,
    thumbnail_long_edge: u32,
    columns: u32,
    output_kind: SheetOutputKind,
) -> SheetLayout {
    let mut thumb = thumbnail_long_edge.max(64);
    let columns = sampler_sheet_columns(trie_thumb_count(trie), columns);
    if output_kind != SheetOutputKind::Jpeg {
        return build_sheet_layout_with_thumb(trie, thumb, columns);
    }
    loop {
        let layout = build_sheet_layout_with_thumb(trie, thumb, columns);
        if layout.height < 60_000 || thumb <= 64 {
            return layout;
        }
        thumb = ((thumb as f64) * 0.9).round().max(64.0) as u32;
    }
}

fn build_sheet_layout_with_thumb(trie: &ProfileTrie, thumb: u32, columns: u32) -> SheetLayout {
    let margin = 28u32;
    let indent = (thumb / 9).clamp(20, 72);
    let gap = (thumb / 28).clamp(8, 22);
    let columns = columns.max(1);
    let max_grid_indent = max_rendered_grid_depth(trie) as u32 + 1;
    let width =
        (thumb * columns + margin * 2 + indent * max_grid_indent + gap * columns.saturating_sub(1))
            .clamp(1200, 32_000);
    let mut ctx = LayoutContext {
        body: String::new(),
        y: margin + 64,
        margin,
        indent,
        gap,
        thumb,
        columns,
    };
    ctx.text(margin, ctx.y, "mini-film sampler", 44, 700, "#111");
    ctx.y += 60;
    ctx.text(
        margin,
        ctx.y,
        "Profiles are grouped by shared name prefixes; indentation shows trie depth.",
        18,
        400,
        "#666",
    );
    ctx.y += 38;
    for (part, child) in &trie.children {
        ctx.render_node(child, &[part.clone()], 0);
    }
    SheetLayout {
        body: ctx.body,
        width,
        height: ctx.y + margin,
    }
}

fn render_sheet_svg(layout: &SheetLayout) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
<rect width="100%" height="100%" fill="white"/>
<style>
{font_css}
.tile {{ stroke: #dddddd; stroke-width: 1; fill: none; }}
.thumb {{ stroke: #d0d0d0; stroke-width: 1; fill: #f8f8f8; }}
</style>
{body}
</svg>
"#,
        width = layout.width,
        height = layout.height,
        font_css = sampler_font_css(),
        body = layout.body
    )
}

struct LayoutContext {
    body: String,
    y: u32,
    margin: u32,
    indent: u32,
    gap: u32,
    thumb: u32,
    columns: u32,
}

impl LayoutContext {
    fn render_node(&mut self, node: &ProfileTrie, prefix: &[String], depth: usize) {
        let x = self.margin + self.indent * depth as u32;
        let text = prefix.join(" ");
        let size = match depth {
            0 => 32,
            1 => 25,
            2 => 19,
            _ => 15,
        };
        let weight = if depth <= 1 { 700 } else { 600 };
        self.y += match depth {
            0 => 62,
            1 => 50,
            2 => 38,
            _ => 26,
        };
        self.text(x, self.y, &text, size, weight, header_color(depth));
        self.y += size + 10;

        if (depth >= 1 || subtree_depth(node) <= 2) && !contains_forced_branch(node) {
            let mut entries = Vec::new();
            collect_subtree_entries(node, prefix.len(), &mut entries);
            if !entries.is_empty() {
                self.render_labeled_thumbs(&entries, depth);
            }
            return;
        }

        let mut leaf_entries: Vec<_> = node
            .thumbs
            .iter()
            .map(|thumb| {
                sheet_entry(
                    prefix.last().cloned().unwrap_or_else(|| thumb.name.clone()),
                    thumb,
                )
            })
            .collect();
        leaf_entries.extend(
            node.children
                .iter()
                .filter(|(_, child)| child.children.is_empty() && !child.thumbs.is_empty())
                .flat_map(|(part, child)| {
                    let label = child_variant_label(prefix, part);
                    child
                        .thumbs
                        .iter()
                        .map(move |thumb| sheet_entry(label.clone(), thumb))
                }),
        );
        sort_sheet_entries_with_common_prefix(&mut leaf_entries, prefix.len());
        if !leaf_entries.is_empty() {
            self.render_labeled_thumbs(&leaf_entries, depth);
        }

        for (part, child) in &node.children {
            if child.children.is_empty() && !child.thumbs.is_empty() {
                continue;
            }
            let mut child_prefix = prefix.to_vec();
            child_prefix.push(part.clone());
            self.render_node(child, &child_prefix, depth + 1);
        }
    }

    fn render_labeled_thumbs(&mut self, entries: &[SheetEntry<'_>], depth: usize) {
        let x = self.margin + self.indent * (depth as u32 + 1);
        let tile = self.thumb + self.gap;
        let padding = (self.thumb / 72).clamp(6, 14);
        let label_height = 66u32;
        let image_box = self.thumb.saturating_sub(padding * 2).max(1);
        let tile_height = label_height + image_box + padding;
        let columns = self.columns.max(1);
        for (index, entry) in entries.iter().enumerate() {
            if index > 0 && index as u32 % columns == 0 {
                self.y += tile_height + self.gap;
            }
            let col = index as u32 % columns;
            let tx = x + col * tile;
            let thumb = entry.thumb;
            let (display_width, display_height) = thumb_display_size(thumb, image_box);
            self.tile_rect(tx, self.y, self.thumb, tile_height);
            self.text(tx + padding, self.y + 48, &entry.label, 30, 500, "#444444");
            self.text(
                tx + padding,
                self.y + 64,
                &entry.full_name,
                12,
                400,
                "#777777",
            );
            let image_x = tx + padding + (image_box - display_width) / 2;
            let image_y = self.y + label_height + padding + (image_box - display_height) / 2;
            self.rect(image_x, image_y, display_width, display_height);
            self.image(
                image_x,
                image_y,
                display_width,
                display_height,
                &thumb.image,
            );
        }
        self.y += tile_height + self.gap;
    }

    fn text(&mut self, x: u32, y: u32, text: &str, size: u32, weight: u32, color: &str) {
        self.body.push_str(&format!(
            r#"<text x="{x}" y="{y}" font-size="{size}" font-weight="{weight}" fill="{color}">{}</text>
"#,
            escape_xml(text)
        ));
    }

    fn rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        self.body.push_str(&format!(
            r#"<rect class="thumb" x="{x}" y="{y}" width="{width}" height="{height}" rx="2"/>
"#
        ));
    }

    fn tile_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        self.body.push_str(&format!(
            r#"<rect class="tile" x="{x}" y="{y}" width="{width}" height="{height}" rx="2"/>
"#
        ));
    }

    fn image(&mut self, x: u32, y: u32, width: u32, height: u32, path: &Path) {
        self.body.push_str(&format!(
            r#"<image x="{x}" y="{y}" width="{width}" height="{height}" preserveAspectRatio="xMidYMid meet" href="{}"/>
"#,
            escape_xml(&path.to_string_lossy())
        ));
    }
}

fn collect_subtree_entries<'a>(
    node: &'a ProfileTrie,
    prefix_len: usize,
    out: &mut Vec<SheetEntry<'a>>,
) {
    for thumb in &node.thumbs {
        out.push(sheet_entry(
            thumb_label_after_prefix(thumb, prefix_len),
            thumb,
        ));
    }
    for child in node.children.values() {
        collect_subtree_entries(child, prefix_len, out);
    }
    sort_sheet_entries_with_common_prefix(out, prefix_len);
}

fn sort_sheet_entries_with_common_prefix(entries: &mut [SheetEntry<'_>], prefix_len: usize) {
    let suffixes = entries
        .iter()
        .map(|entry| entry.thumb.parts.get(prefix_len..).unwrap_or(&[]))
        .collect::<Vec<_>>();
    let common_prefix_len = common_prefix_len(&suffixes);

    for entry in entries.iter_mut() {
        let suffix = entry.thumb.parts.get(prefix_len..).unwrap_or(&[]);
        let start = common_prefix_len.min(suffix.len());
        entry.sort_key = variant_sort_key_from_parts(&suffix[start..], &entry.label);
    }
    entries.sort_by(|left, right| left.sort_key.cmp(&right.sort_key));
}

fn common_prefix_len(parts: &[&[String]]) -> usize {
    if parts.is_empty() {
        return 0;
    }
    let mut prefix_len = parts[0].len();
    for part in parts.iter().skip(1) {
        let max_len = prefix_len.min(part.len());
        let mut next = 0usize;
        while next < max_len && parts[0][next].eq_ignore_ascii_case(&part[next]) {
            next += 1;
        }
        prefix_len = next;
        if prefix_len == 0 {
            break;
        }
    }
    prefix_len
}

fn thumb_label_after_prefix(thumb: &SampleThumb, prefix_len: usize) -> String {
    let suffix = thumb.parts.get(prefix_len..).unwrap_or(&[]);
    if suffix.len() == 1
        && suffix[0].eq_ignore_ascii_case("grainy")
        && let Some(base) = prefix_len
            .checked_sub(1)
            .and_then(|index| thumb.parts.get(index))
    {
        return format!("{base} {}", suffix[0]);
    }
    let label = suffix.join(" ");
    if label.is_empty() {
        thumb
            .parts
            .last()
            .cloned()
            .unwrap_or_else(|| thumb.name.clone())
    } else {
        label
    }
}

fn child_variant_label(prefix: &[String], part: &str) -> String {
    prefix
        .last()
        .map(|base| format!("{base} {part}"))
        .unwrap_or_else(|| part.to_string())
}

fn sheet_entry(label: String, thumb: &SampleThumb) -> SheetEntry<'_> {
    SheetEntry {
        sort_key: variant_sort_key(&label),
        full_name: profile_filename_without_xmp(&thumb.filename),
        label,
        thumb,
    }
}

fn profile_filename_without_xmp(filename: &str) -> String {
    filename
        .strip_suffix(".xmp")
        .or_else(|| filename.strip_suffix(".XMP"))
        .unwrap_or(filename)
        .to_string()
}

fn variant_sort_key(label: &str) -> String {
    variant_sort_key_from_parts(&profile_name_parts(label), label)
}

fn variant_sort_key_from_parts(parts: &[String], fallback: &str) -> String {
    let mut marker_parts = Vec::new();
    let mut non_grainy_markers = Vec::new();
    for part in parts.iter() {
        if let Some(marker) = normalize_variant_marker(part) {
            marker_parts.push(marker);
            if marker != "grainy" {
                non_grainy_markers.push(marker);
            }
        }
    }

    let (variant_group, variant_markers_key, grainy_position) = if non_grainy_markers.is_empty() {
        if marker_parts.is_empty() {
            (0u16, String::new(), 0u8)
        } else {
            (1u16, "grainy".to_string(), 1u8)
        }
    } else {
        non_grainy_markers.sort_unstable_by_key(|part| variant_marker_rank(part));
        let group = non_grainy_markers
            .first()
            .copied()
            .map_or(99, variant_marker_rank)
            .min(999);
        let markers_key = non_grainy_markers.join(" ");
        let grainy_position = if marker_parts.iter().any(|part| *part == "grainy") {
            1u8
        } else {
            0u8
        };
        (group, markers_key, grainy_position)
    };

    let normalized = parts
        .iter()
        .filter(|part| !is_variant_marker(part))
        .map(|part| natural_sort_part(part))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    format!(
        "{normalized}\u{0}{variant_group:03}\u{0}{variant_markers_key}\u{0}{grainy_position}\u{0}{}",
        fallback.to_ascii_lowercase()
    )
}

fn normalize_variant_marker(part: &str) -> Option<&'static str> {
    let normalized = part.trim_matches('+').to_ascii_lowercase();
    if normalized.is_empty() {
        return Some("plus");
    }
    match normalized.as_str() {
        "grainy" => Some("grainy"),
        "plus" => Some("plus"),
        "hc" => Some("hc"),
        "faded" | "fade" => Some("faded"),
        "warm" => Some("warm"),
        "cool" => Some("cool"),
        "vibrant" => Some("vibrant"),
        "muted" => Some("muted"),
        "contrast" => Some("contrast"),
        "contrasty" => Some("contrasty"),
        "expired" => Some("expired"),
        _ => None,
    }
}

fn variant_marker_rank(marker: &str) -> u16 {
    match marker {
        "grainy" => 1,
        "faded" => 2,
        "plus" => 3,
        "hc" => 4,
        "warm" => 5,
        "cool" => 6,
        "vibrant" => 7,
        "muted" => 8,
        "contrast" => 9,
        "contrasty" => 10,
        "expired" => 11,
        _ => 98,
    }
}

fn is_variant_marker(part: &str) -> bool {
    normalize_variant_marker(part).is_some()
}

fn natural_sort_part(part: &str) -> String {
    if let Some(version) = part.strip_prefix('v').or_else(|| part.strip_prefix('V')) {
        if let Ok(number) = version.parse::<u32>() {
            return format!("v{number:06}");
        }
    }
    if let Ok(number) = part.parse::<u32>() {
        return format!("{number:06}");
    }
    part.to_string()
}

fn is_version_part(part: &str) -> bool {
    part.strip_prefix('v')
        .or_else(|| part.strip_prefix('V'))
        .is_some_and(|version| version.parse::<u32>().is_ok())
}

fn is_film_speed_part(part: &str) -> bool {
    part.parse::<u32>()
        .is_ok_and(|speed| (25..=12800).contains(&speed))
}

fn trie_thumb_count(trie: &ProfileTrie) -> u32 {
    let children: u32 = trie.children.values().map(trie_thumb_count).sum();
    trie.thumbs.len() as u32 + children
}

fn subtree_depth(trie: &ProfileTrie) -> usize {
    trie.children
        .values()
        .map(subtree_depth)
        .max()
        .map_or(0, |depth| depth + 1)
}

fn contains_forced_branch(trie: &ProfileTrie) -> bool {
    trie.children.iter().any(|(part, child)| {
        is_version_part(part) || is_film_speed_part(part) || contains_forced_branch(child)
    })
}

fn max_rendered_grid_depth(trie: &ProfileTrie) -> usize {
    trie.children
        .values()
        .map(|child| max_rendered_grid_depth_at(child, 0))
        .max()
        .unwrap_or(0)
}

fn max_rendered_grid_depth_at(node: &ProfileTrie, depth: usize) -> usize {
    if (depth >= 1 || subtree_depth(node) <= 2) && !contains_forced_branch(node) {
        return depth;
    }

    let has_leaf_grid = !node.thumbs.is_empty()
        || node
            .children
            .values()
            .any(|child| child.children.is_empty() && !child.thumbs.is_empty());
    let own_depth = has_leaf_grid.then_some(depth);
    let child_depth = node
        .children
        .values()
        .filter(|child| !(child.children.is_empty() && !child.thumbs.is_empty()))
        .map(|child| max_rendered_grid_depth_at(child, depth + 1))
        .max();

    own_depth
        .into_iter()
        .chain(child_depth)
        .max()
        .unwrap_or(depth)
}

fn sampler_sheet_columns(thumb_count: u32, requested_columns: u32) -> u32 {
    thumb_count.clamp(1, requested_columns.max(1))
}

fn header_color(depth: usize) -> &'static str {
    match depth {
        0 => "#111111",
        1 => "#333333",
        2 => "#4b4b4b",
        _ => "#666666",
    }
}

fn thumb_display_size(thumb: &SampleThumb, long_edge: u32) -> (u32, u32) {
    let width = thumb.width.max(1) as f64;
    let height = thumb.height.max(1) as f64;
    let scale = long_edge as f64 / width.max(height);
    (
        (width * scale).round().max(1.0) as u32,
        (height * scale).round().max(1.0) as u32,
    )
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn sampler_font_css() -> String {
    r#"text {
font-family: "PragmataPro Mono Liga", "PragmataProMonoLiga", "Pragmata Pro", ui-monospace, "DejaVu Sans Mono", "Noto Sans Mono", "Cascadia Code", "SFMono-Regular", Menlo, Monaco, Consolas, monospace;
letter-spacing: 0;
}"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_thumb(name: &str, width: u32, height: u32) -> SampleThumb {
        SampleThumb {
            image: PathBuf::from(format!("/tmp/{name}.jpg")),
            profile: PathBuf::from(format!("/tmp/{name}.xmp")),
            pp3: None,
            hald: None,
            name: name.to_string(),
            filename: format!("{name}.xmp"),
            parts: profile_name_parts(name),
            width,
            height,
        }
    }

    fn sampler_args_for_cache(
        raw: PathBuf,
        profiles_root: PathBuf,
        hald_dir: PathBuf,
    ) -> SamplerArgs {
        SamplerArgs {
            raw,
            output: PathBuf::from("sheet.jpg"),
            profiles_root,
            hald_dir,
            hald_level: 16,
            rawtherapee: PathBuf::from("rawtherapee-cli"),
            convert: PathBuf::from("convert"),
            no_grain: false,
            grain_seed: Some(123),
            no_cache: false,
            jobs: Some(1),
            thumbnail_long_edge: 512,
            columns: 8,
            jpg_quality: 95,
            jpeg_subsampling: JpegSubsampling::S444,
            strip_metadata: false,
            progressive_jpeg: false,
        }
    }

    #[test]
    fn profile_names_strip_extension_and_split_into_visible_levels() {
        assert_eq!(
            profile_display_name_from_relative("Kodak Portra 400 Grainy.xmp"),
            "Kodak Portra 400 Grainy"
        );
        assert_eq!(
            profile_display_name_from_relative("RNI/Kodak Portra 400.xmp"),
            "Kodak Portra 400"
        );
        assert_eq!(
            profile_name_parts("Kodak Portra 400 Grainy"),
            vec!["Kodak", "Portra", "400", "Grainy"]
        );
        assert_eq!(profile_name_parts(""), vec!["Profile"]);
    }

    #[test]
    fn structured_layout_renders_prefix_headers_and_thumbnail_refs() {
        let mut trie = ProfileTrie::default();
        let mut thumb = sample_thumb("Kodak Portra 400 Grainy", 128, 85);
        thumb.image = PathBuf::from("/tmp/kodak.jpg");
        trie.insert(thumb);

        let layout = build_sheet_layout(&trie, 128, 8, SheetOutputKind::Jpeg);
        let svg = render_sheet_svg(&layout);

        assert!(svg.contains(">Kodak<"));
        assert!(svg.contains(">Kodak Portra<"));
        assert!(svg.contains(">400 Grainy<"));
        assert!(svg.contains(">Kodak Portra 400 Grainy<"));
        assert!(svg.contains("/tmp/kodak.jpg"));
        assert!(svg.contains(r#"width="116" height="77""#));
        assert!(svg.contains("font-family"));
    }

    #[test]
    fn family_layout_keeps_versions_and_grainy_variants_in_one_grid() {
        let mut trie = ProfileTrie::default();
        for name in [
            "Fuji Superia 200 v2",
            "Fuji Superia 200 v2 grainy",
            "Fuji Superia 200 v3",
            "Fuji Superia 200 v3 grainy",
        ] {
            trie.insert(sample_thumb(name, 1024, 683));
        }

        let layout = build_sheet_layout(&trie, 256, 8, SheetOutputKind::Jpeg);
        let svg = render_sheet_svg(&layout);

        assert!(svg.contains(">Fuji<"));
        assert!(svg.contains(">Fuji Superia<"));
        assert!(svg.contains(">Fuji Superia 200 v2<"));
        assert!(svg.contains(">v2<"));
        assert!(svg.contains(">Fuji Superia 200 v2<"));
        assert!(svg.contains(">v2 grainy<"));
        assert!(svg.contains(">Fuji Superia 200 v2 grainy<"));
        assert!(svg.contains(">Fuji Superia 200 v3<"));
        assert!(svg.contains(">v3<"));
        assert!(svg.contains(">v3 grainy<"));
        assert!(svg.find(">v2<") < svg.find(">v2 grainy<"));
        assert!(svg.find(">Fuji Superia 200 v2<") < svg.find(">Fuji Superia 200 v3<"));
        assert!(svg.find(">v3<") < svg.find(">v3 grainy<"));
    }

    #[test]
    fn base_profile_sorts_before_grainy_when_family_name_is_the_prefix() {
        let mut trie = ProfileTrie::default();
        for name in ["Ilford FP4", "Ilford FP4 grainy"] {
            trie.insert(sample_thumb(name, 1024, 683));
        }

        let layout = build_sheet_layout(&trie, 256, 8, SheetOutputKind::Jpeg);
        let svg = render_sheet_svg(&layout);

        assert!(svg.contains(">Ilford<"));
        assert!(svg.contains(">FP4<"));
        assert!(svg.contains(">FP4 grainy<"));
        assert!(svg.find(">FP4<") < svg.find(">FP4 grainy<"));
    }

    #[test]
    fn base_profile_sorts_before_named_variants() {
        let mut trie = ProfileTrie::default();
        for name in [
            "Ilford FP4",
            "Ilford FP4 faded",
            "Ilford FP4 contrast",
            "Ilford FP4 grainy",
        ] {
            trie.insert(sample_thumb(name, 1024, 683));
        }

        let layout = build_sheet_layout(&trie, 256, 8, SheetOutputKind::Jpeg);
        let svg = render_sheet_svg(&layout);

        assert!(svg.contains(">FP4<"));
        assert!(svg.contains(">FP4 faded<"));
        assert!(svg.contains(">FP4 contrast<"));
        assert!(svg.find(">FP4<") < svg.find(">FP4 faded<"));
        assert!(svg.find(">FP4<") < svg.find(">FP4 grainy<"));
        assert!(svg.find(">FP4 grainy<") < svg.find(">FP4 faded<"));
        assert!(svg.find(">FP4 faded<") < svg.find(">FP4 contrast<"));
    }

    #[test]
    fn base_profile_sorts_before_common_variant_suffixes() {
        let base = variant_sort_key("Kodak Portra 160");

        for variant in [
            "Kodak Portra 160 HC",
            "Kodak Portra 160 plus",
            "Kodak Portra 160 faded",
            "Kodak Portra 160 grainy",
            "Kodak Portra 160 warm",
            "Kodak Portra 160 muted",
        ] {
            assert!(base < variant_sort_key(variant), "{variant}");
        }
    }

    #[test]
    fn max_common_prefix_prefers_exact_profile() {
        let base = variant_sort_key("Agfa Scala 200");
        let plus = variant_sort_key("Agfa Scala 200 +");
        let plus_grainy = variant_sort_key("Agfa Scala 200 + grainy");
        let plus_plus = variant_sort_key("Agfa Scala 200 ++");
        let plus_plus_grainy = variant_sort_key("Agfa Scala 200 ++ grainy");
        let faded = variant_sort_key("Agfa Scala 200 faded");

        assert!(base < plus);
        assert!(plus < plus_grainy);
        assert!(plus < plus_plus);
        assert!(plus_plus < plus_plus_grainy);
        assert!(faded < plus_plus);
    }

    #[test]
    fn plus_and_plus_grainy_are_adjacent() {
        let generic = variant_sort_key("Agfa Scala 200");
        let plus = variant_sort_key("Agfa Scala 200 +");
        let plus_grainy = variant_sort_key("Agfa Scala 200 + grainy");
        let plus_plus = variant_sort_key("Agfa Scala 200 ++");
        let plus_plus_grainy = variant_sort_key("Agfa Scala 200 ++ grainy");

        assert!(generic < plus, "base should stay first");
        assert!(plus < plus_grainy, "plus should sort before plus grainy");
        assert!(
            plus < plus_plus,
            "plus variants should stay before ++ variants"
        );
        assert!(
            plus_plus < plus_plus_grainy,
            "++ should stay before ++ grainy"
        );
    }

    #[test]
    fn base_profile_sorts_before_plus_variants() {
        let base = variant_sort_key("Agfa Scala 200");

        for variant in [
            "Agfa Scala 200 +",
            "Agfa Scala 200 ++",
            "Agfa Scala 200 + grainy",
            "Agfa Scala 200 ++ grainy",
        ] {
            assert!(base < variant_sort_key(variant), "{variant}");
        }
    }

    #[test]
    fn film_speeds_render_as_separate_branches() {
        let mut trie = ProfileTrie::default();
        for name in [
            "Kodak Portra 200",
            "Kodak Portra 200 grainy",
            "Kodak Portra 800",
            "Kodak Portra 800 grainy",
            "Kodak Portra 100",
            "Kodak Portra 100 grainy",
        ] {
            trie.insert(sample_thumb(name, 1024, 683));
        }

        let layout = build_sheet_layout(&trie, 256, 8, SheetOutputKind::Jpeg);
        let svg = render_sheet_svg(&layout);

        assert!(svg.contains(">Kodak Portra 100<"));
        assert!(svg.contains(">Kodak Portra 200<"));
        assert!(svg.contains(">Kodak Portra 800<"));
        assert!(svg.find(">Kodak Portra 100<") < svg.find(">Kodak Portra 200<"));
        assert!(svg.find(">Kodak Portra 200<") < svg.find(">Kodak Portra 800<"));
        assert!(svg.find(">100<") < svg.find(">100 grainy<"));
        assert!(svg.find(">200<") < svg.find(">200 grainy<"));
        assert!(svg.find(">800<") < svg.find(">800 grainy<"));
    }

    #[test]
    fn large_sampler_layout_stays_below_jpeg_dimension_limit() {
        let mut trie = ProfileTrie::default();
        for film in 0..104 {
            for version in 1..=3 {
                for grainy in [false, true] {
                    let name = if grainy {
                        format!("Fuji Superia {film} variant {version} grainy")
                    } else {
                        format!("Fuji Superia {film} variant {version}")
                    };
                    trie.insert(sample_thumb(&name, 1024, 683));
                }
            }
        }

        let layout = build_sheet_layout(&trie, 1024, 8, SheetOutputKind::Jpeg);

        assert_eq!(trie_thumb_count(&trie), 624);
        assert!(layout.width < 65_000);
        assert!(layout.height < 65_000);
    }

    #[test]
    fn sampler_columns_use_requested_cap() {
        assert_eq!(sampler_sheet_columns(414, 8), 8);
        assert_eq!(sampler_sheet_columns(24, 4), 4);
        assert_eq!(sampler_sheet_columns(3, 4), 3);
    }

    #[test]
    fn pdf_sampler_layout_preserves_requested_thumbnail_size() {
        let mut trie = ProfileTrie::default();
        for index in 0..64 {
            trie.insert(sample_thumb(&format!("Kodak Portra {index}"), 1024, 683));
        }

        let layout = build_sheet_layout(&trie, 1024, 8, SheetOutputKind::Pdf);
        let svg = render_sheet_svg(&layout);

        assert!(svg.contains(r#"width="996" height="664""#));
    }

    #[test]
    fn tiff_sampler_layout_preserves_requested_thumbnail_size() {
        let mut trie = ProfileTrie::default();
        for index in 0..64 {
            trie.insert(sample_thumb(&format!("Kodak Portra {index}"), 1024, 683));
        }

        let layout = build_sheet_layout(&trie, 1024, 8, SheetOutputKind::Tiff);
        let svg = render_sheet_svg(&layout);

        assert!(svg.contains(r#"width="996" height="664""#));
    }

    #[test]
    fn pdf_sampler_can_render_four_requested_columns_without_shrinking_thumbnails() {
        let mut trie = ProfileTrie::default();
        for index in 0..16 {
            trie.insert(sample_thumb(&format!("Kodak Portra {index}"), 1024, 683));
        }

        let layout = build_sheet_layout(&trie, 1024, 4, SheetOutputKind::Pdf);
        let svg = render_sheet_svg(&layout);

        assert!(layout.width < 5000);
        assert!(svg.contains(r#"width="996" height="664""#));
    }

    #[test]
    fn sampler_accepts_jpeg_tiff_and_pdf_outputs() {
        assert_eq!(
            sampler_output_kind(Path::new("sheet.jpg")).unwrap(),
            Some(SheetOutputKind::Jpeg)
        );
        assert_eq!(
            sampler_output_kind(Path::new("sheet.tiff")).unwrap(),
            Some(SheetOutputKind::Tiff)
        );
        assert_eq!(
            sampler_output_kind(Path::new("sheet.pdf")).unwrap(),
            Some(SheetOutputKind::Pdf)
        );
        assert_eq!(
            sampler_output_kind(Path::new("sheet.html")).unwrap(),
            Some(SheetOutputKind::Html)
        );
        assert_eq!(sampler_output_kind(Path::new("sheet.png")).unwrap(), None);
    }

    #[test]
    fn html_sampler_renders_grouped_thumbnail_references() {
        let mut trie = ProfileTrie::default();
        let mut thumb = sample_thumb("Kodak Portra 400 Grainy", 1024, 683);
        thumb.image = PathBuf::from("thumbnails/kodak.jpg");
        thumb.pp3 = Some(PathBuf::from("pp3/Kodak Portra 400 Grainy.pp3"));
        thumb.hald = Some(PathBuf::from("/tmp/Kodak Portra 400 Grainy.hald.png"));
        trie.insert(thumb);

        let html = render_sheet_html(&trie, 4).unwrap();

        assert!(html.contains("<!doctype html>"));
        assert!(html.contains("--columns: 4"));
        assert!(
            html.contains(
                "font-family: \"PragmataPro Mono Liga\", \"PragmataProMonoLiga\", \"Pragmata Pro\", ui-monospace,"
            )
        );
        assert!(!html.contains("@font-face"));
        assert!(html.contains("repeat(var(--columns), minmax(0, 1fr))"));
        assert!(html.contains("grid-auto-rows: 1fr"));
        assert!(html.contains("<h1>mini-film sampler</h1>"));
        assert!(html.contains(r#"<h2 class="branch-title"><button class="branch-toggle" type="button" aria-expanded="true">Kodak</button></h2>"#));
        assert!(html.contains(r#"<h3 class="branch-title"><button class="branch-toggle" type="button" aria-expanded="true">Kodak Portra</button></h3>"#));
        assert!(html.contains(">400 Grainy<"));
        assert!(html.contains("<span>Kodak Portra 400 Grainy</span>"));
        assert!(!html.contains(">Kodak Portra 400 Grainy.xmp<"));
        assert!(html.contains("class=\"branch-toggle\""));
        assert!(html.contains("class=\"branch-body\""));
        assert!(html.contains("data-branch-key=\"Kodak/Portra\""));
        assert!(html.contains("mini-film-collapsed-branches"));
        assert!(html.contains("localStorage.setItem"));
        assert!(html.contains("src=\"thumbnails/kodak.jpg\""));
        assert!(html.contains("href=\"file:///tmp/Kodak%20Portra%20400%20Grainy.xmp\""));
        assert!(html.contains("href=\"pp3/Kodak%20Portra%20400%20Grainy.pp3\""));
        assert!(html.contains("href=\"file:///tmp/Kodak%20Portra%20400%20Grainy.hald.png\""));
        assert!(html.contains(">XMP</a>"));
        assert!(html.contains(">PP3</a>"));
        assert!(html.contains(">HALD</a>"));
        assert!(html.contains("href=\"file:///tmp/Kodak%20Portra%20400%20Grainy.xmp\" download"));
        assert!(html.contains("href=\"pp3/Kodak%20Portra%20400%20Grainy.pp3\" download"));
        assert!(
            html.contains("href=\"file:///tmp/Kodak%20Portra%20400%20Grainy.hald.png\" download")
        );
        assert!(html.contains("class=\"thumb-button\""));
        assert!(html.contains("id=\"overlay\""));
        assert!(html.contains("max-width: calc(100vw - 96px)"));
        assert!(html.contains("max-height: calc(100vh - 128px)"));
        assert!(html.contains("https://github.com/alfanick/mini-film"));
        let version = env!("CARGO_PKG_VERSION");
        assert!(html.contains(&format!("mini-film</a> {version}")));
        assert!(html.contains("Picture by Amadeus Juskowiak"));
        assert!(html.contains(
            "https://reallyniceimages.com/products/rni-all-films-5-pro-for-adobe-lightroom.html"
        ));
        assert!(html.contains("loading=\"lazy\""));
    }

    #[test]
    fn html_section_heading_levels_follow_document_depth() {
        assert_eq!(html_heading_tag(0), "h2");
        assert_eq!(html_heading_tag(1), "h3");
        assert_eq!(html_heading_tag(2), "h4");
        assert_eq!(html_heading_tag(3), "h5");
        assert_eq!(html_heading_tag(4), "h6");
        assert_eq!(html_heading_tag(8), "h6");
    }

    #[test]
    fn html_pp3_names_strip_xmp_extension_and_disambiguate_duplicates() {
        let first = sample_thumb("Ilford FP4", 1024, 683);
        let second = sample_thumb("Ilford FP4", 1024, 683);
        let mut names = BTreeMap::new();

        assert_eq!(unique_html_sidecar_stem(&mut names, &first), "Ilford FP4");
        assert_eq!(
            unique_html_sidecar_stem(&mut names, &second),
            "Ilford FP4-2"
        );
    }

    #[test]
    fn profile_filenames_strip_only_xmp_extension_for_display() {
        assert_eq!(
            profile_filename_without_xmp("RNI/Kodak Portra 400.xmp"),
            "RNI/Kodak Portra 400"
        );
        assert_eq!(
            profile_filename_without_xmp("RNI/Kodak Portra 400.XMP"),
            "RNI/Kodak Portra 400"
        );
        assert_eq!(
            profile_filename_without_xmp("RNI/Kodak Portra 400"),
            "RNI/Kodak Portra 400"
        );
    }

    #[test]
    fn html_links_escape_spaces_for_file_and_relative_urls() {
        assert_eq!(
            file_url(Path::new("/tmp/RNI Films/Ilford FP4.xmp")),
            "file:///tmp/RNI%20Films/Ilford%20FP4.xmp"
        );
        assert_eq!(
            relative_url(&PathBuf::from("pp3/Ilford FP4.pp3")),
            "pp3/Ilford%20FP4.pp3"
        );
    }

    #[test]
    fn thumbnail_cache_path_uses_raw_and_xmp_sha1_plus_render_options() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("input.dng");
        let xmp = dir.path().join("profile.xmp");
        fs::write(&raw, b"raw bytes").unwrap();
        fs::write(&xmp, b"xmp bytes").unwrap();

        let mut args = sampler_args_for_cache(
            raw.clone(),
            dir.path().to_path_buf(),
            dir.path().join("hald"),
        );
        let cache = ThumbnailCache::new(&raw).unwrap();
        let cached = cache.path_for(&xmp, &args).unwrap();
        let name = cached.file_name().unwrap().to_string_lossy();

        assert!(cached.starts_with(env::temp_dir().join("mini-film-sampler-cache")));
        assert!(name.contains(&sha1_file(&raw).unwrap()));
        assert!(name.contains(&sha1_file(&xmp).unwrap()));
        assert!(name.contains("512px"));

        args.thumbnail_long_edge = 1024;
        let larger = cache.path_for(&xmp, &args).unwrap();
        assert_ne!(cached, larger);
    }

    #[test]
    fn sampler_grain_is_attenuated_for_small_thumbnails() {
        let grain = GrainSettings {
            amount: 50,
            size: 50,
            frequency: 50,
        };

        let small = scale_sampler_grain(grain, 512);
        let medium = scale_sampler_grain(grain, 1024);
        let full = scale_sampler_grain(grain, 4000);

        assert!(small.amount < grain.amount);
        assert!(small.size < grain.size);
        assert!(medium.amount > small.amount);
        assert!(medium.amount < grain.amount);
        assert_eq!(small.frequency, grain.frequency);
        assert_eq!(full.amount, grain.amount);
        assert_eq!(full.size, grain.size);
    }

    #[test]
    fn thumbnail_display_size_preserves_aspect_ratio() {
        let landscape = sample_thumb("Landscape", 6000, 4000);
        let portrait = sample_thumb("Portrait", 3000, 4500);

        assert_eq!(thumb_display_size(&landscape, 512), (512, 341));
        assert_eq!(thumb_display_size(&portrait, 512), (341, 512));
    }

    #[test]
    fn sampler_font_css_prefers_pragmata_when_available() {
        let css = sampler_font_css();
        assert!(css.contains("PragmataPro Mono Liga"));
        assert!(css.contains("ui-monospace"));
    }

    #[test]
    fn sample_seed_changes_with_index_path_or_base_seed() {
        let path = Path::new("emulations/Fuji.xmp");
        let seed = sample_seed(10, 0, path);
        assert_eq!(seed, sample_seed(10, 0, path));
        assert_ne!(seed, sample_seed(11, 0, path));
        assert_ne!(seed, sample_seed(10, 1, path));
        assert_ne!(seed, sample_seed(10, 0, Path::new("emulations/Kodak.xmp")));
    }

    #[test]
    fn sampler_duration_estimates_are_clamped_and_monotonic() {
        assert_eq!(
            estimate_sampler_raw_duration(1),
            estimate_sampler_raw_duration(128)
        );
        assert!(estimate_sampler_raw_duration(1024) > estimate_sampler_raw_duration(512));
        assert!(estimate_sampler_grain_duration(4096) <= Duration::from_secs_f64(1.5));
        assert!(estimate_sheet_duration(10_000) <= Duration::from_secs(20));
    }

    #[test]
    fn resolve_sampler_jobs_defaults_and_rejects_zero() {
        assert!(resolve_sampler_jobs(None).unwrap() >= 1);
        assert_eq!(resolve_sampler_jobs(Some(4)).unwrap(), 4);
        assert!(resolve_sampler_jobs(Some(0)).is_err());
    }
}
