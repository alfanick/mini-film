use std::{
    collections::BTreeMap,
    env,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, bail};
use handlebars::Handlebars;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use mini_film::{
    DiffusionMethod, DiffusionPreset, DiffusionSettings, GrainEngine, GrainRenderOptions,
    GrainSettings, apply_diffusion, apply_grain_8bit_with_options,
    write_rawtherapee_resize_profile,
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha1::{Digest, Sha1};
use tempfile::Builder;
use walkdir::WalkDir;

use crate::app::dcp::{DcpProfile, resolve_dcp_profile};
use crate::app::dng::DngFallbackConfig;
use crate::app::export::{add_convert_thread_limit, finalize_output, output_ext};
use crate::app::pp3::{
    RAW_RENDER_PIPELINE_KEY, write_rawtherapee_auto_matched_curve_profile,
    write_rawtherapee_color_noise_profile, write_rawtherapee_dcp_profile,
    write_rawtherapee_lens_corrections_profile, write_rawtherapee_srgb_output_profile,
};
use crate::app::profile::{profile_from_xmp_quiet, rawtherapee_profiles_with_hald};
use crate::app::progress::{
    ApplyProgress, StageEstimates, format_duration, progress_length, progress_position,
    progress_stage, progress_stage_adaptive, set_progress,
};
use crate::app::raw::{run_raw_develop, run_raw_develop_jpeg};
use crate::app::sampler_assets::{
    html_children_template, html_grid_template, html_page_template, html_script,
    html_section_template, html_styles, html_tile_template,
};
use crate::app::sampler_detail::{
    ANALYSIS_LONG_EDGE, SAMPLER_DETAIL_ANALYSIS_VERSION, SamplerDetailAnalysis, SamplerDetailArea,
    SamplerDetailKind, analyze_sampler_detail_areas,
};
use crate::app::timestamps::{GalleryFocusRegion, extract_gallery_exif};
use crate::app::util::{
    OutputEditMetadata, extract_capture_iso, half_cpu_thread_count, is_raw_input_file,
    remove_temp_file, restore_output_color_profile,
    sync_output_metadata_from_raw_with_color_profile, sync_output_timestamps_from_exif,
    time_of_day_seed,
};
use crate::cli::{ExportOptions, JpegSubsampling, LensCorrections};

pub(crate) struct SamplerArgs {
    pub(crate) raw: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_dir: PathBuf,
    pub(crate) hald_level: u32,
    pub(crate) rawtherapee: PathBuf,
    pub(crate) dng_fallback: DngFallbackConfig,
    pub(crate) convert: PathBuf,
    pub(crate) lcp_root: Option<PathBuf>,
    pub(crate) no_grain: bool,
    pub(crate) normalize_grain_mpix: Option<f64>,
    pub(crate) color_noise_iso_threshold: u32,
    pub(crate) lens_corrections: LensCorrections,
    pub(crate) grain_seed: Option<u64>,
    pub(crate) grain_engine: GrainEngine,
    pub(crate) diffusion: DiffusionSettings,
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
    diffusion_image: Option<PathBuf>,
    original_image: Option<PathBuf>,
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
    Html,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SamplerIntermediateKind {
    Jpeg8,
    Tiff16,
}

impl SamplerIntermediateKind {
    fn for_diffusion(settings: DiffusionSettings) -> Self {
        if settings.is_enabled() {
            Self::Tiff16
        } else {
            Self::Jpeg8
        }
    }

    const fn filename(self) -> &'static str {
        match self {
            Self::Jpeg8 => "rawtherapee.jpg",
            Self::Tiff16 => "rawtherapee.tif",
        }
    }
}

fn sampler_intermediate_kind(
    output_kind: SheetOutputKind,
    diffusion: DiffusionSettings,
) -> SamplerIntermediateKind {
    match output_kind {
        SheetOutputKind::Jpeg => SamplerIntermediateKind::for_diffusion(diffusion),
        SheetOutputKind::Html => SamplerIntermediateKind::Tiff16,
    }
}

fn html_sampler_diffusion_settings(configured: DiffusionSettings) -> DiffusionSettings {
    if configured.is_enabled() {
        configured.canonical_render_settings()
    } else {
        DiffusionPreset::Medium.settings(DiffusionMethod::MultiScaleMist)
    }
}

struct SamplerProgress {
    profile: ProgressBar,
    started: Instant,
    estimates: Arc<StageEstimates>,
}

struct ThumbnailCache {
    dir: PathBuf,
    raw_sha1: String,
    dcp_identity: String,
}

const SAMPLER_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const DEFAULT_COLOR_NOISE_ISO_THRESHOLD: u32 = 1600;

#[derive(Debug, Serialize, Deserialize)]
struct CachedSamplerDetailAnalysis {
    analysis_version: String,
    source_sha1: String,
    focus_signature: String,
    source_width: u32,
    source_height: u32,
    analysis: SamplerDetailAnalysis,
}

struct HtmlPairCachePaths {
    profile_image: PathBuf,
    diffusion_image: PathBuf,
    manifest: PathBuf,
}

impl HtmlPairCachePaths {
    fn is_valid(&self) -> bool {
        self.is_valid_at(SystemTime::now())
    }

    fn is_valid_at(&self, now: SystemTime) -> bool {
        if !self.manifest.is_file() {
            return false;
        }
        if !cache_file_is_fresh_at(&self.manifest, now)
            || !cache_file_is_fresh_at(&self.profile_image, now)
            || !cache_file_is_fresh_at(&self.diffusion_image, now)
        {
            return false;
        }
        match (
            decoded_sampler_image_dimensions(&self.profile_image),
            decoded_sampler_image_dimensions(&self.diffusion_image),
        ) {
            (Some(profile), Some(diffusion)) if profile == diffusion => {
                let Ok(profile_sha1) = sha1_file(&self.profile_image) else {
                    return false;
                };
                let Ok(diffusion_sha1) = sha1_file(&self.diffusion_image) else {
                    return false;
                };
                let expected = html_pair_manifest(profile, &profile_sha1, &diffusion_sha1);
                fs::read_to_string(&self.manifest).is_ok_and(|manifest| manifest == expected)
            }
            _ => false,
        }
    }
}

struct ProfileRenderContext<'a> {
    args: &'a SamplerArgs,
    temp_root: &'a Path,
    emulation_root: &'a Path,
    index: usize,
    base_seed: u64,
    export: &'a ExportOptions,
    cache: Option<&'a ThumbnailCache>,
    dcp_profile: Option<&'a DcpProfile>,
    progress: &'a SamplerProgress,
}

struct StructuredSheetContext<'a> {
    rawtherapee: &'a Path,
    dng_fallback: &'a DngFallbackConfig,
    convert: &'a Path,
    output: &'a Path,
    raw: &'a Path,
    thumbs: &'a [SampleThumb],
    profiles_root: &'a Path,
    hald_dir: &'a Path,
    hald_level: u32,
    color_noise_iso_threshold: u32,
    lens_corrections: LensCorrections,
    thumbnail_long_edge: u32,
    columns: u32,
    jpg_quality: u8,
    jpeg_subsampling: JpegSubsampling,
    progressive_jpeg: bool,
    dcp_profile: Option<&'a DcpProfile>,
    cache: Option<&'a ThumbnailCache>,
    focus_regions: &'a [GalleryFocusRegion],
}

struct SamplerSidecarContext<'a> {
    raw: &'a Path,
    hald_level: u32,
    profiles_root: &'a Path,
    hald_dir: &'a Path,
    color_noise_iso_threshold: u32,
    lens_corrections: LensCorrections,
    dcp_profile: Option<&'a DcpProfile>,
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
    let mut args = args;
    validate_sampler_args(&args)?;
    let focus_regions = extract_gallery_exif(&args.raw)
        .map(|exif| exif.focus_regions)
        .unwrap_or_default();
    let prepared_source = args.dng_fallback.prepare_known(&args.raw)?;
    args.raw = prepared_source.active().to_path_buf();
    let dcp_profile = is_raw_input_file(&args.raw)
        .then(|| resolve_dcp_profile(&args.raw, &args.dng_fallback))
        .flatten();
    let jobs = resolve_sampler_jobs(args.jobs)?;

    let emulation_root = emulation_root(&args.profiles_root);
    let profiles = collect_xmp_profiles(&emulation_root)?;
    if profiles.is_empty() {
        bail!("no XMP files found under {}", emulation_root.display());
    }

    let temp_dir = Builder::new().prefix("mini-film-sampler-").tempdir()?;
    let base_seed = args.grain_seed.unwrap_or_else(time_of_day_seed);
    let cache = sampler_cache(&args.raw, dcp_profile.as_ref(), args.no_cache)?.map(Arc::new);
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
                    let render_context = ProfileRenderContext {
                        args: &args,
                        temp_root: temp_dir.path(),
                        emulation_root: &emulation_root,
                        index,
                        base_seed,
                        export: &export,
                        cache: cache.as_deref(),
                        dcp_profile: dcp_profile.as_ref(),
                        progress: &progress,
                    };
                    let result = render_profile_thumbnail(&render_context, profile);
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
    let sheet_context = StructuredSheetContext {
        rawtherapee: &args.rawtherapee,
        dng_fallback: &args.dng_fallback,
        convert: &args.convert,
        output: &args.output,
        raw: &args.raw,
        thumbs: &thumbs,
        profiles_root: &args.profiles_root,
        hald_dir: &args.hald_dir,
        hald_level: args.hald_level,
        color_noise_iso_threshold: args.color_noise_iso_threshold,
        lens_corrections: args.lens_corrections,
        thumbnail_long_edge: args.thumbnail_long_edge,
        columns: args.columns,
        jpg_quality: args.jpg_quality,
        jpeg_subsampling: args.jpeg_subsampling,
        progressive_jpeg: args.progressive_jpeg,
        dcp_profile: dcp_profile.as_ref(),
        cache: cache.as_deref(),
        focus_regions: &focus_regions,
    };
    run_structured_sheet(&sheet_context)?;
    args.dng_fallback
        .finish_successful_development(&prepared_source)?;
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
        bail!("sampler output must be .jpg, .jpeg, or .html");
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
    fn new(raw: &Path, dcp_profile: Option<&DcpProfile>) -> Result<Self> {
        let raw_sha1 = sha1_file(raw).with_context(|| format!("hashing RAW {}", raw.display()))?;
        let dir = env::temp_dir().join("mini-film-sampler-cache");
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let dcp_identity = dcp_profile
            .map(DcpProfile::cache_identity)
            .unwrap_or_else(|| "dcp-none".to_string());
        Ok(Self {
            dir,
            raw_sha1,
            dcp_identity,
        })
    }

    fn path_for(&self, profile: &Path, args: &SamplerArgs) -> Result<PathBuf> {
        self.path_for_with_seed(profile, args, args.grain_seed)
    }

    fn html_pair_base_path(&self, profile: &Path, args: &SamplerArgs) -> Result<PathBuf> {
        self.path_for_with_seed(profile, args, None)
    }

    fn path_for_with_seed(
        &self,
        profile: &Path,
        args: &SamplerArgs,
        grain_seed: Option<u64>,
    ) -> Result<PathBuf> {
        let xmp_sha1 =
            sha1_file(profile).with_context(|| format!("hashing XMP {}", profile.display()))?;
        let grain_mode = if args.no_grain {
            "nograin".to_string()
        } else {
            let normalization = args
                .normalize_grain_mpix
                .map(|mpix| format!("norm-{:016x}", mpix.to_bits()))
                .unwrap_or_else(|| "norm-off".to_string());
            let seed = grain_seed
                .map(|seed| format!("-seed-{seed}"))
                .unwrap_or_default();
            format!("grain-{}-{normalization}{seed}", args.grain_engine)
        };
        let diffusion = args.diffusion.canonical_render_settings();
        let advanced_diffusion = !diffusion.has_neutral_advanced_controls();
        let diffusion_mode = sampler_diffusion_cache_mode(diffusion);
        let subsampling = format!("{:?}", args.jpeg_subsampling).to_ascii_lowercase();
        let lens_corrections = if args.lens_corrections == LensCorrections::default() {
            "none".to_string()
        } else {
            format!(
                "{}{}{}",
                if args.lens_corrections.distortion {
                    "d"
                } else {
                    ""
                },
                if args.lens_corrections.ca { ".ca" } else { "" },
                if args.lens_corrections.vignetting {
                    ".v"
                } else {
                    ""
                }
            )
        };
        let mut render_options = format!(
            "{}-{}-l{}-{}px-lc{}-q{}-{}-{}-{}-sg3-strip{}-prog{}",
            RAW_RENDER_PIPELINE_KEY,
            self.dcp_identity,
            args.hald_level,
            args.thumbnail_long_edge,
            lens_corrections,
            args.jpg_quality,
            subsampling,
            grain_mode,
            diffusion_mode,
            args.strip_metadata as u8,
            args.progressive_jpeg as u8,
        );
        if args.color_noise_iso_threshold != DEFAULT_COLOR_NOISE_ISO_THRESHOLD {
            render_options.push_str(&format!("-noise{}", args.color_noise_iso_threshold));
        }
        let legacy_file_name = format!("{}-{xmp_sha1}-{render_options}.jpg", self.raw_sha1);
        let file_name = if !advanced_diffusion && legacy_file_name.len() <= 255 {
            legacy_file_name
        } else {
            let mut hasher = Sha1::new();
            hasher.update(render_options);
            let digest = hasher
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let cache_version = if advanced_diffusion {
                "diffusion-v2"
            } else {
                "cache-v1"
            };
            format!("{}-{xmp_sha1}-{cache_version}-{digest}.jpg", self.raw_sha1)
        };
        Ok(self.dir.join(file_name))
    }

    fn html_pair_paths(
        &self,
        profile: &Path,
        profile_index: usize,
        args: &SamplerArgs,
    ) -> Result<HtmlPairCachePaths> {
        let xmp_sha1 =
            sha1_file(profile).with_context(|| format!("hashing XMP {}", profile.display()))?;
        let single_output_key = self.html_pair_base_path(profile, args)?;
        let diffusion = html_sampler_diffusion_settings(args.diffusion)
            .render_identity()
            .expect("the HTML sampler diffusion variant is enabled");
        let mut hasher = Sha1::new();
        hasher.update(b"html-sampler-diffusion-pair-v1\0");
        hasher.update(single_output_key.as_os_str().as_encoded_bytes());
        hasher.update(b"\0");
        hasher.update(diffusion);
        hasher.update(b"\0");
        hasher.update(profile.as_os_str().as_encoded_bytes());
        hasher.update(b"\0");
        hasher.update(profile_index.to_le_bytes());
        if let Some(grain_seed) = args.grain_seed {
            hasher.update(b"\0seed=");
            hasher.update(grain_seed.to_le_bytes());
        }
        let digest = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let stem = format!(
            "{}-{xmp_sha1}-html-diffusion-pair-v1-{digest}",
            self.raw_sha1
        );
        Ok(HtmlPairCachePaths {
            profile_image: self.dir.join(format!("{stem}-profile.jpg")),
            diffusion_image: self.dir.join(format!("{stem}-diffusion.jpg")),
            manifest: self.dir.join(format!("{stem}.pair")),
        })
    }

    fn html_original_path(&self, jpg_quality: u8, jpeg_subsampling: JpegSubsampling) -> PathBuf {
        let mut hasher = Sha1::new();
        hasher.update(b"html-sampler-original-v1\0");
        hasher.update(RAW_RENDER_PIPELINE_KEY);
        hasher.update(b"\0");
        hasher.update(self.dcp_identity.as_bytes());
        hasher.update(b"\0quality=");
        hasher.update(jpg_quality.clamp(1, 100).to_le_bytes());
        hasher.update(b"\0subsampling=");
        hasher.update(format!("{jpeg_subsampling:?}").as_bytes());
        let digest = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        self.dir
            .join(format!("{}-html-original-v1-{digest}.jpg", self.raw_sha1))
    }

    fn progressive_html_path(
        &self,
        source: &Path,
        jpg_quality: u8,
        jpeg_subsampling: JpegSubsampling,
    ) -> Result<PathBuf> {
        progressive_html_cache_path(&self.dir, source, jpg_quality, jpeg_subsampling)
    }

    fn html_detail_analysis_path(&self, source_sha1: &str, focus_signature: &str) -> PathBuf {
        let mut hasher = Sha1::new();
        hasher.update(SAMPLER_DETAIL_ANALYSIS_VERSION.as_bytes());
        hasher.update(b"\0");
        hasher.update(source_sha1.as_bytes());
        hasher.update(b"\0");
        hasher.update(focus_signature.as_bytes());
        let digest = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        self.dir
            .join("html-detail-areas")
            .join(format!("{digest}.json"))
    }
}

fn sampler_cache(
    raw: &Path,
    dcp_profile: Option<&DcpProfile>,
    no_cache: bool,
) -> Result<Option<ThumbnailCache>> {
    if no_cache {
        return Ok(None);
    }
    ThumbnailCache::new(raw, dcp_profile).map(Some)
}

fn cache_file_is_fresh_at(path: &Path, now: SystemTime) -> bool {
    let Ok(modified) = fs::metadata(path).and_then(|metadata| metadata.modified()) else {
        return false;
    };
    now.duration_since(modified)
        .is_ok_and(|age| age <= SAMPLER_CACHE_TTL)
}

fn fresh_decodable_sampler_image(path: &Path) -> bool {
    fresh_decodable_sampler_image_at(path, SystemTime::now())
}

fn fresh_decodable_sampler_image_at(path: &Path, now: SystemTime) -> bool {
    cache_file_is_fresh_at(path, now) && decoded_sampler_image_dimensions(path).is_some()
}

fn sampler_focus_signature(focus_regions: &[GalleryFocusRegion]) -> Result<String> {
    let serialized =
        serde_json::to_vec(focus_regions).context("serializing sampler focus regions")?;
    let mut hasher = Sha1::new();
    hasher.update(serialized);
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn load_or_analyze_sampler_details(
    convert: &Path,
    source: &Path,
    focus_regions: &[GalleryFocusRegion],
    cache: Option<&ThumbnailCache>,
) -> Result<SamplerDetailAnalysis> {
    let source_sha1 = sha1_file(source)
        .with_context(|| format!("hashing sampler original {}", source.display()))?;
    let focus_signature = sampler_focus_signature(focus_regions)?;
    let (source_width, source_height) = image::image_dimensions(source)
        .with_context(|| format!("decoding sampler original {}", source.display()))?;
    let cache_path =
        cache.map(|cache| cache.html_detail_analysis_path(&source_sha1, &focus_signature));

    if let Some(cache_path) = cache_path.as_ref()
        && cache_file_is_fresh_at(cache_path, SystemTime::now())
        && let Ok(bytes) = fs::read(cache_path)
        && let Ok(cached) = serde_json::from_slice::<CachedSamplerDetailAnalysis>(&bytes)
        && cached.analysis_version == SAMPLER_DETAIL_ANALYSIS_VERSION
        && cached.source_sha1 == source_sha1
        && cached.focus_signature == focus_signature
        && cached.source_width == source_width
        && cached.source_height == source_height
        && cached.analysis.is_valid()
    {
        return Ok(cached.analysis);
    }

    let image = load_sampler_detail_proxy(convert, source, source_width, source_height)?;
    let analysis = analyze_sampler_detail_areas(&image, focus_regions);
    if !analysis.is_valid() {
        bail!("sampler detail analysis produced invalid areas");
    }

    if let Some(cache_path) = cache_path {
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let cached = CachedSamplerDetailAnalysis {
            analysis_version: SAMPLER_DETAIL_ANALYSIS_VERSION.to_string(),
            source_sha1,
            focus_signature,
            source_width,
            source_height,
            analysis: analysis.clone(),
        };
        let serialized =
            serde_json::to_vec(&cached).context("serializing sampler detail analysis")?;
        write_cache_file_atomically(&serialized, &cache_path)?;
    }

    Ok(analysis)
}

fn load_sampler_detail_proxy(
    convert: &Path,
    source: &Path,
    source_width: u32,
    source_height: u32,
) -> Result<image::RgbImage> {
    if source_width.max(source_height) <= ANALYSIS_LONG_EDGE {
        return Ok(image::open(source)
            .with_context(|| format!("decoding sampler original {}", source.display()))?
            .into_rgb8());
    }

    let temp_dir = Builder::new()
        .prefix("mini-film-sampler-analysis-")
        .tempdir()?;
    let proxy = temp_dir.path().join("analysis.ppm");
    let mut command = Command::new(convert);
    add_convert_thread_limit(&mut command, convert);
    command
        .arg(source)
        .arg("-resize")
        .arg(format!("{ANALYSIS_LONG_EDGE}x{ANALYSIS_LONG_EDGE}>"))
        .arg("-depth")
        .arg("8")
        .arg(&proxy)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = command
        .output()
        .with_context(|| format!("running {} for sampler detail analysis", convert.display()))?;
    if !output.status.success() {
        bail!(
            "sampler detail proxy failed with status {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(image::open(&proxy)
        .with_context(|| format!("decoding sampler detail proxy {}", proxy.display()))?
        .into_rgb8())
}

fn decoded_sampler_image_dimensions(path: &Path) -> Option<(u32, u32)> {
    image::open(path)
        .ok()
        .map(|image| (image.width(), image.height()))
        .filter(|(width, height)| *width > 0 && *height > 0)
}

fn html_pair_manifest(dimensions: (u32, u32), profile_sha1: &str, diffusion_sha1: &str) -> String {
    format!(
        "html-sampler-pair-v1\n{}x{}\n{profile_sha1}\n{diffusion_sha1}\n",
        dimensions.0, dimensions.1
    )
}

fn sampler_diffusion_cache_mode(diffusion: DiffusionSettings) -> String {
    let diffusion = diffusion.canonical_render_settings();
    if diffusion.has_neutral_advanced_controls() {
        return format!(
            "diffusion-v1-{:?}-{}-{}",
            diffusion.method, diffusion.softness, diffusion.highlight_glow
        );
    }

    diffusion
        .render_identity()
        .expect("enabled advanced diffusion has a render identity")
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
        diffusion_image: None,
        original_image: None,
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

fn sample_thumb_from_pair(
    profile_image: PathBuf,
    diffusion_image: PathBuf,
    profile: &Path,
    emulation_root: &Path,
) -> Result<SampleThumb> {
    let profile_dimensions = decoded_sampler_image_dimensions(&profile_image)
        .with_context(|| format!("reading {}", profile_image.display()))?;
    let diffusion_dimensions = decoded_sampler_image_dimensions(&diffusion_image)
        .with_context(|| format!("reading {}", diffusion_image.display()))?;
    if profile_dimensions != diffusion_dimensions {
        bail!(
            "HTML sampler pair dimensions differ: {:?} and {:?}",
            profile_dimensions,
            diffusion_dimensions
        );
    }
    let mut thumb = sample_thumb_from_image(profile_image, profile, emulation_root)?;
    thumb.diffusion_image = Some(diffusion_image);
    Ok(thumb)
}

fn sampler_output_kind(output: &Path) -> Result<Option<SheetOutputKind>> {
    Ok(match output_ext(output)?.as_str() {
        "jpg" | "jpeg" => Some(SheetOutputKind::Jpeg),
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

pub(crate) fn collect_xmp_profiles(root: &Path) -> Result<Vec<PathBuf>> {
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

pub(crate) fn emulation_root(root: &Path) -> PathBuf {
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
    context: &ProfileRenderContext<'_>,
    profile: &Path,
) -> Result<SampleThumb> {
    let output_kind =
        sampler_output_kind(&context.args.output)?.context("unsupported sampler output format")?;
    let html_pair_cache = if output_kind == SheetOutputKind::Html {
        context
            .cache
            .map(|cache| cache.html_pair_paths(profile, context.index, context.args))
            .transpose()?
    } else {
        None
    };
    if let Some(cached) = html_pair_cache.as_ref() {
        if cached.is_valid() {
            sampler_step(context.progress, 5, "cache hit");
            return sample_thumb_from_pair(
                cached.profile_image.clone(),
                cached.diffusion_image.clone(),
                profile,
                context.emulation_root,
            );
        }
    } else if let Some(cache) = context.cache {
        let cached = cache.path_for(profile, context.args)?;
        if fresh_decodable_sampler_image(&cached) {
            sampler_step(context.progress, 5, "cache hit");
            return sample_thumb_from_image(cached, profile, context.emulation_root);
        }
    }

    sampler_step(context.progress, 1, "resolve");
    let profile_temp = context.temp_root.join(format!("profile-{}", context.index));
    fs::create_dir_all(&profile_temp)
        .with_context(|| format!("creating {}", profile_temp.display()))?;

    let resolved = profile_from_xmp_quiet(
        profile,
        context.args.hald_level,
        &context.args.profiles_root,
        &context.args.hald_dir,
        &profile_temp,
    )
    .with_context(|| format!("resolving profile {}", profile.display()))?;
    let prepared_source = context.args.dng_fallback.prepare_known(&context.args.raw)?;
    let active_source = prepared_source.active().to_path_buf();
    let intermediate_kind = sampler_intermediate_kind(output_kind, context.args.diffusion);
    let diffusion_enabled = intermediate_kind == SamplerIntermediateKind::Tiff16;
    let developed = profile_temp.join(intermediate_kind.filename());
    let mut rawtherapee_profiles = rawtherapee_profiles_with_hald(&resolved, &profile_temp)?;
    append_color_noise_if_qualified(
        &active_source,
        context.args.color_noise_iso_threshold,
        &mut rawtherapee_profiles,
        &profile_temp,
    )?;
    append_lens_corrections_if_requested(
        context.args.lens_corrections,
        &mut rawtherapee_profiles,
        &profile_temp,
    )?;
    append_dcp_or_auto_matched_curve(
        &mut rawtherapee_profiles,
        &profile_temp,
        context.dcp_profile,
    )?;
    if diffusion_enabled {
        rawtherapee_profiles.push(write_rawtherapee_srgb_output_profile(
            &profile_temp.join("diffusion-srgb-output.pp3"),
        )?);
    }
    rawtherapee_profiles.push(write_rawtherapee_resize_profile(
        &profile_temp.join("resize.pp3"),
        context.args.thumbnail_long_edge,
    )?);
    let apply_progress = ApplyProgress {
        file: &context.progress.profile,
        started: context.progress.started,
        estimates: Some(Arc::clone(&context.progress.estimates)),
    };
    let raw_stage = progress_stage_adaptive(
        Some(&apply_progress),
        2,
        3,
        "sampler-rawtherapee",
        "rawtherapee",
        estimate_sampler_raw_duration(context.args.thumbnail_long_edge),
    );
    let raw_outcome = match intermediate_kind {
        SamplerIntermediateKind::Tiff16 => run_raw_develop(
            &context.args.rawtherapee,
            &rawtherapee_profiles,
            prepared_source,
            &developed,
            context.args.lcp_root.as_deref(),
            true,
            &context.args.dng_fallback,
        )?,
        SamplerIntermediateKind::Jpeg8 => run_raw_develop_jpeg(
            &context.args.rawtherapee,
            &rawtherapee_profiles,
            prepared_source,
            &developed,
            context.args.jpg_quality,
            context.args.jpeg_subsampling,
            context.args.lcp_root.as_deref(),
            true,
            &context.args.dng_fallback,
        )?,
    };
    raw_stage.finish();

    if output_kind == SheetOutputKind::Html {
        let diffusion = html_sampler_diffusion_settings(context.args.diffusion);
        sampler_step(context.progress, 3, "diffusion pair");
        let diffused = profile_temp.join("diffused.tif");
        apply_diffusion(&developed, &diffused, diffusion)?;

        let grain_enabled = !context.args.no_grain && resolved.grain.is_enabled();
        let metadata_grain = if grain_enabled {
            attenuate_sampler_grain_amount(resolved.grain, context.args.thumbnail_long_edge)
        } else {
            GrainSettings::default()
        };
        let metadata_grain_seed =
            grain_enabled.then(|| sample_seed(context.base_seed, context.index, profile));
        let (profile_source, diffusion_source) = if grain_enabled {
            let grain_stage = progress_stage_adaptive(
                Some(&apply_progress),
                3,
                4,
                "sampler-grain-pair",
                "grain pair",
                estimate_sampler_grain_duration(context.args.thumbnail_long_edge) * 2,
            );
            let profile_grained = profile_temp.join("profile-grained-8.ppm");
            let diffusion_grained = profile_temp.join("diffusion-grained-8.ppm");
            let grain_options = GrainRenderOptions {
                engine: context.args.grain_engine,
                normalize_grain_mpix: context.args.normalize_grain_mpix,
            };
            let grain_seed = metadata_grain_seed.unwrap_or_default();
            apply_grain_8bit_with_options(
                &developed,
                &profile_grained,
                metadata_grain,
                grain_seed,
                grain_options,
            )?;
            apply_grain_8bit_with_options(
                &diffused,
                &diffusion_grained,
                metadata_grain,
                grain_seed,
                grain_options,
            )?;
            grain_stage.finish();
            (profile_grained, diffusion_grained)
        } else {
            sampler_step(context.progress, 3, "grain skipped");
            (developed.clone(), diffused.clone())
        };

        let thumbnail_stage = progress_stage_adaptive(
            Some(&apply_progress),
            4,
            5,
            "sampler-thumbnail-pair",
            "thumbnail pair",
            estimate_sampler_thumbnail_duration(context.args.thumbnail_long_edge) * 2,
        );
        let profile_thumb = profile_temp.join("thumb.jpg");
        let diffusion_thumb = profile_temp.join("diffusion-thumb.jpg");
        finalize_output(
            &context.args.convert,
            &profile_source,
            &profile_thumb,
            context.export,
        )?;
        finalize_output(
            &context.args.convert,
            &diffusion_source,
            &diffusion_thumb,
            context.export,
        )?;
        thumbnail_stage.finish();

        if context.args.strip_metadata {
            restore_output_color_profile(Some(&developed), &profile_thumb)?;
            restore_output_color_profile(Some(&developed), &diffusion_thumb)?;
            let metadata_stage = progress_stage_adaptive(
                Some(&apply_progress),
                5,
                6,
                "sampler-timestamps-pair",
                "timestamps pair",
                estimate_sampler_metadata_duration() * 2,
            );
            sync_output_timestamps_from_exif(raw_outcome.source.active(), &profile_thumb)?;
            sync_output_timestamps_from_exif(raw_outcome.source.active(), &diffusion_thumb)?;
            metadata_stage.finish();
        } else {
            let metadata_stage = progress_stage_adaptive(
                Some(&apply_progress),
                5,
                6,
                "sampler-exif-pair",
                "exif pair",
                estimate_sampler_exif_duration() * 2,
            );
            let comment = format!(
                "mini-film {} usage=sampler profile={}",
                env!("CARGO_PKG_VERSION"),
                resolved.resolved_stem
            );
            let metadata = |diffusion| OutputEditMetadata {
                comment: Some(comment.as_str()),
                profile: &resolved.metadata,
                profile_sharpening_applied: is_raw_input_file(raw_outcome.source.active()),
                grain: metadata_grain,
                grain_seed: metadata_grain_seed,
                grain_engine: grain_enabled.then_some(context.args.grain_engine),
                normalize_grain_mpix: context.args.normalize_grain_mpix,
                diffusion,
            };
            sync_output_metadata_from_raw_with_color_profile(
                raw_outcome.source.active(),
                &profile_thumb,
                metadata(DiffusionSettings::default()),
                Some(&developed),
            )?;
            sync_output_timestamps_from_exif(raw_outcome.source.active(), &profile_thumb)?;
            sync_output_metadata_from_raw_with_color_profile(
                raw_outcome.source.active(),
                &diffusion_thumb,
                metadata(diffusion),
                Some(&developed),
            )?;
            sync_output_timestamps_from_exif(raw_outcome.source.active(), &diffusion_thumb)?;
            metadata_stage.finish();
        }

        if grain_enabled {
            remove_temp_file(&profile_source)?;
            remove_temp_file(&diffusion_source)?;
        }
        remove_temp_file(&developed)?;
        remove_temp_file(&diffused)?;
        let (profile_image, diffusion_image) = if let Some(cached) = html_pair_cache {
            copy_html_pair_to_cache(&profile_thumb, &diffusion_thumb, &cached)?;
            (cached.profile_image, cached.diffusion_image)
        } else {
            (profile_thumb, diffusion_thumb)
        };
        context
            .args
            .dng_fallback
            .finish_successful_development(&raw_outcome.source)?;
        sampler_step(context.progress, 6, "done");
        return sample_thumb_from_pair(
            profile_image,
            diffusion_image,
            profile,
            context.emulation_root,
        );
    }

    let diffused = if diffusion_enabled {
        sampler_step(context.progress, 3, "diffusion");
        let output = profile_temp.join("diffused.tif");
        apply_diffusion(&developed, &output, context.args.diffusion)?;
        Some(output)
    } else {
        None
    };
    let developed_for_grain = diffused.as_deref().unwrap_or(&developed);

    let grain_enabled = !context.args.no_grain && resolved.grain.is_enabled();
    let metadata_grain = if grain_enabled {
        attenuate_sampler_grain_amount(resolved.grain, context.args.thumbnail_long_edge)
    } else {
        GrainSettings::default()
    };
    let metadata_grain_seed =
        grain_enabled.then(|| sample_seed(context.base_seed, context.index, profile));

    let color_profile_source = developed.clone();
    let source = if grain_enabled {
        let grain_stage = progress_stage_adaptive(
            Some(&apply_progress),
            3,
            4,
            "sampler-grain",
            "grain",
            estimate_sampler_grain_duration(context.args.thumbnail_long_edge),
        );
        let grained = profile_temp.join("grained-8.ppm");
        apply_grain_8bit_with_options(
            developed_for_grain,
            &grained,
            metadata_grain,
            metadata_grain_seed.unwrap_or_default(),
            GrainRenderOptions {
                engine: context.args.grain_engine,
                normalize_grain_mpix: context.args.normalize_grain_mpix,
            },
        )?;
        grain_stage.finish();
        grained
    } else {
        sampler_step(context.progress, 3, "grain skipped");
        developed_for_grain.to_path_buf()
    };

    let thumbnail_stage = progress_stage_adaptive(
        Some(&apply_progress),
        4,
        5,
        "sampler-thumbnail",
        "thumbnail",
        estimate_sampler_thumbnail_duration(context.args.thumbnail_long_edge),
    );
    let thumb = profile_temp.join("thumb.jpg");
    finalize_output(&context.args.convert, &source, &thumb, context.export)?;
    thumbnail_stage.finish();
    if context.args.strip_metadata {
        if diffusion_enabled {
            restore_output_color_profile(Some(&color_profile_source), &thumb)?;
        }
        let metadata_stage = progress_stage_adaptive(
            Some(&apply_progress),
            5,
            6,
            "sampler-timestamps",
            "timestamps",
            estimate_sampler_metadata_duration(),
        );
        sync_output_timestamps_from_exif(raw_outcome.source.active(), &thumb)?;
        metadata_stage.finish();
    } else {
        let metadata_stage = progress_stage_adaptive(
            Some(&apply_progress),
            5,
            6,
            "sampler-exif",
            "exif",
            estimate_sampler_exif_duration(),
        );
        sync_output_metadata_from_raw_with_color_profile(
            raw_outcome.source.active(),
            &thumb,
            OutputEditMetadata {
                comment: Some(&format!(
                    "mini-film {} usage=sampler profile={}",
                    env!("CARGO_PKG_VERSION"),
                    resolved.resolved_stem
                )),
                profile: &resolved.metadata,
                profile_sharpening_applied: is_raw_input_file(raw_outcome.source.active()),
                grain: metadata_grain,
                grain_seed: metadata_grain_seed,
                grain_engine: grain_enabled.then_some(context.args.grain_engine),
                normalize_grain_mpix: context.args.normalize_grain_mpix,
                diffusion: context.args.diffusion,
            },
            Some(&color_profile_source),
        )?;
        sync_output_timestamps_from_exif(raw_outcome.source.active(), &thumb)?;
        metadata_stage.finish();
    }
    remove_temp_file(&source)?;
    if developed != source {
        remove_temp_file(&developed)?;
    }
    if let Some(diffused) = diffused
        && diffused != source
    {
        remove_temp_file(&diffused)?;
    }
    let image = if let Some(cache) = context.cache {
        let cached = cache.path_for(profile, context.args)?;
        copy_thumbnail_to_cache(&thumb, &cached)?;
        cached
    } else {
        thumb
    };
    context
        .args
        .dng_fallback
        .finish_successful_development(&raw_outcome.source)?;
    sampler_step(context.progress, 6, "done");
    sample_thumb_from_image(image, profile, context.emulation_root)
}

fn append_color_noise_if_qualified(
    raw: &Path,
    color_noise_iso_threshold: u32,
    rawtherapee_profiles: &mut Vec<PathBuf>,
    temp_dir: &Path,
) -> Result<()> {
    if color_noise_iso_threshold == 0 {
        return Ok(());
    }

    let iso = match extract_capture_iso(raw)? {
        Some(iso) => iso,
        None => return Ok(()),
    };
    if iso < color_noise_iso_threshold {
        return Ok(());
    }

    if let Some(path) =
        write_rawtherapee_color_noise_profile(&temp_dir.join("color-noise.pp3"), iso)?
    {
        rawtherapee_profiles.push(path);
    }

    Ok(())
}

fn append_lens_corrections_if_requested(
    lens_corrections: LensCorrections,
    rawtherapee_profiles: &mut Vec<PathBuf>,
    temp_dir: &Path,
) -> Result<()> {
    if let Some(path) = write_rawtherapee_lens_corrections_profile(
        &temp_dir.join("lens-corrections.pp3"),
        lens_corrections,
    )? {
        rawtherapee_profiles.push(path);
    }
    Ok(())
}

fn append_auto_matched_curve(
    rawtherapee_profiles: &mut Vec<PathBuf>,
    temp_dir: &Path,
) -> Result<()> {
    rawtherapee_profiles.push(write_rawtherapee_auto_matched_curve_profile(
        &temp_dir.join("auto-matched-curve.pp3"),
    )?);
    Ok(())
}

fn append_dcp_or_auto_matched_curve(
    rawtherapee_profiles: &mut Vec<PathBuf>,
    temp_dir: &Path,
    dcp_profile: Option<&DcpProfile>,
) -> Result<()> {
    if let Some(dcp_profile) = dcp_profile {
        rawtherapee_profiles.push(write_rawtherapee_dcp_profile(
            &temp_dir.join("adobe-dcp.pp3"),
            &dcp_profile.path,
        )?);
        Ok(())
    } else {
        append_auto_matched_curve(rawtherapee_profiles, temp_dir)
    }
}

fn append_color_noise_to_profiles(
    rawtherapee_profiles: Vec<PathBuf>,
    temp_dir: &Path,
    raw: &Path,
    color_noise_iso_threshold: u32,
) -> Result<Vec<PathBuf>> {
    let mut profiles = rawtherapee_profiles;
    append_color_noise_if_qualified(raw, color_noise_iso_threshold, &mut profiles, temp_dir)?;
    Ok(profiles)
}

fn append_lens_corrections_to_profiles(
    rawtherapee_profiles: Vec<PathBuf>,
    temp_dir: &Path,
    lens_corrections: LensCorrections,
) -> Result<Vec<PathBuf>> {
    let mut profiles = rawtherapee_profiles;
    append_lens_corrections_if_requested(lens_corrections, &mut profiles, temp_dir)?;
    Ok(profiles)
}

fn copy_thumbnail_to_cache(source: &Path, destination: &Path) -> Result<()> {
    decoded_sampler_image_dimensions(source)
        .with_context(|| format!("sampler thumbnail does not decode: {}", source.display()))?;
    copy_cache_file_atomically(source, destination)
}

fn copy_html_pair_to_cache(
    profile_source: &Path,
    diffusion_source: &Path,
    destinations: &HtmlPairCachePaths,
) -> Result<()> {
    let parent = destinations
        .manifest
        .parent()
        .context("HTML sampler cache pair has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;

    let profile_dimensions = decoded_sampler_image_dimensions(profile_source)
        .context("HTML sampler profile image does not decode")?;
    let diffusion_dimensions = decoded_sampler_image_dimensions(diffusion_source)
        .context("HTML sampler diffusion image does not decode")?;
    if profile_dimensions != diffusion_dimensions {
        bail!(
            "HTML sampler pair dimensions differ: {:?} and {:?}",
            profile_dimensions,
            diffusion_dimensions
        );
    }
    let profile_sha1 = sha1_file(profile_source)?;
    let diffusion_sha1 = sha1_file(diffusion_source)?;

    // The manifest is the pair's commit marker. Remove it and both old halves
    // before publishing either replacement, then install a manifest tied to
    // the exact hashes only after both images decode and agree in dimensions.
    remove_temp_file(&destinations.manifest)?;
    remove_temp_file(&destinations.profile_image)?;
    remove_temp_file(&destinations.diffusion_image)?;
    copy_cache_file_atomically(profile_source, &destinations.profile_image)?;
    copy_cache_file_atomically(diffusion_source, &destinations.diffusion_image)?;

    let cached_profile_dimensions = decoded_sampler_image_dimensions(&destinations.profile_image)
        .context("cached HTML sampler profile image does not decode")?;
    let cached_diffusion_dimensions =
        decoded_sampler_image_dimensions(&destinations.diffusion_image)
            .context("cached HTML sampler diffusion image does not decode")?;
    if cached_profile_dimensions != profile_dimensions
        || cached_diffusion_dimensions != diffusion_dimensions
        || sha1_file(&destinations.profile_image)? != profile_sha1
        || sha1_file(&destinations.diffusion_image)? != diffusion_sha1
    {
        bail!("cached HTML sampler pair changed while it was being published");
    }
    let manifest = html_pair_manifest(profile_dimensions, &profile_sha1, &diffusion_sha1);
    write_cache_file_atomically(manifest.as_bytes(), &destinations.manifest)
}

fn copy_cache_file_atomically(source: &Path, destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .context("sampler cache file has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let temp = Builder::new()
        .prefix(".mini-film-sampler-cache-")
        .suffix(".tmp")
        .tempfile_in(parent)
        .with_context(|| format!("creating temporary cache file in {}", parent.display()))?;
    fs::copy(source, temp.path())
        .with_context(|| format!("copying {} to {}", source.display(), temp.path().display()))?;
    temp.as_file()
        .sync_all()
        .with_context(|| format!("syncing {}", temp.path().display()))?;
    temp.persist(destination)
        .map(|_| ())
        .map_err(|err| err.error)
        .with_context(|| format!("publishing cache file {}", destination.display()))
}

fn write_cache_file_atomically(contents: &[u8], destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .context("sampler cache file has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let temp = Builder::new()
        .prefix(".mini-film-sampler-cache-")
        .suffix(".tmp")
        .tempfile_in(parent)
        .with_context(|| format!("creating temporary cache file in {}", parent.display()))?;
    fs::write(temp.path(), contents)
        .with_context(|| format!("writing {}", temp.path().display()))?;
    temp.as_file()
        .sync_all()
        .with_context(|| format!("syncing {}", temp.path().display()))?;
    temp.persist(destination)
        .map(|_| ())
        .map_err(|err| err.error)
        .with_context(|| format!("publishing cache file {}", destination.display()))
}

fn run_structured_sheet(context: &StructuredSheetContext<'_>) -> Result<()> {
    let convert = context.convert;
    let output = context.output;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let output_kind = sampler_output_kind(output)?.context("unsupported sampler output format")?;
    if output_kind == SheetOutputKind::Html {
        return run_html_sheet(context);
    }

    let mut trie = ProfileTrie::default();
    for thumb in context.thumbs {
        trie.insert(thumb.clone_for_tree());
    }
    let layout = build_sheet_layout(&trie, context.thumbnail_long_edge, context.columns);
    let svg = render_sheet_svg(&layout);
    let svg_path = output.with_extension("mini-film-sampler.svg");
    fs::write(&svg_path, svg).with_context(|| format!("writing {}", svg_path.display()))?;

    let mut command = Command::new(convert);
    add_convert_thread_limit(&mut command, convert);
    command.arg(&svg_path);
    match output_kind {
        SheetOutputKind::Jpeg => {
            command
                .arg("-quality")
                .arg(context.jpg_quality.clamp(1, 100).to_string());
            if context.progressive_jpeg {
                command.arg("-interlace").arg("Line");
            }
        }
        SheetOutputKind::Html => {}
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

fn run_html_sheet(context: &StructuredSheetContext<'_>) -> Result<()> {
    let thumbs = context.thumbs;
    let output = context.output;
    let output_dir = output.parent().unwrap_or_else(|| Path::new("."));
    let thumbnail_dir = output_dir.join("thumbnails");
    let pp3_dir = output_dir.join("pp3");
    fs::create_dir_all(&thumbnail_dir)
        .with_context(|| format!("creating {}", thumbnail_dir.display()))?;
    fs::create_dir_all(&pp3_dir).with_context(|| format!("creating {}", pp3_dir.display()))?;
    let baseline_original = {
        let baseline_source = thumbnail_dir.join("original.jpg");
        let baseline_relative = PathBuf::from("thumbnails").join("original.jpg");
        write_html_baseline_thumbnail(context, &baseline_source)?;
        baseline_relative
    };
    let detail_analysis = load_or_analyze_sampler_details(
        context.convert,
        &thumbnail_dir.join("original.jpg"),
        context.focus_regions,
        context.cache,
    )?;

    let jobs = half_cpu_thread_count();
    let pool = rayon::ThreadPoolBuilder::new().num_threads(jobs).build()?;
    let mut html_thumbs: Vec<(usize, SampleThumb)> = pool.install(|| {
        thumbs
            .par_iter()
            .enumerate()
            .map(|(index, thumb)| {
                let file_name = html_thumbnail_file_name(index, thumb);
                let destination = thumbnail_dir.join(&file_name);
                let diffusion_source = thumb
                    .diffusion_image
                    .as_ref()
                    .context("HTML sampler thumbnail is missing its diffusion variant")?;
                let diffusion_file_name = html_diffusion_thumbnail_file_name(index, thumb);
                let diffusion_destination = thumbnail_dir.join(&diffusion_file_name);
                write_cached_progressive_html_thumbnail(
                    context.convert,
                    &thumb.image,
                    &destination,
                    context.jpg_quality,
                    context.jpeg_subsampling,
                    context.cache,
                )?;
                write_cached_progressive_html_thumbnail(
                    context.convert,
                    diffusion_source,
                    &diffusion_destination,
                    context.jpg_quality,
                    context.jpeg_subsampling,
                    context.cache,
                )?;
                let mut exported = thumb.clone_for_tree();
                exported.image = PathBuf::from("thumbnails").join(file_name);
                exported.diffusion_image =
                    Some(PathBuf::from("thumbnails").join(diffusion_file_name));
                exported.original_image = Some(baseline_original.clone());
                Ok::<_, anyhow::Error>((index, exported))
            })
            .collect::<Result<Vec<_>>>()
    })?;
    html_thumbs.sort_by_key(|(index, _)| *index);

    let mut sidecar_names = BTreeMap::new();
    let mut sidecar_exports = Vec::with_capacity(html_thumbs.len());
    for (html_index, (_, thumb)) in html_thumbs.iter_mut().enumerate() {
        let stem = unique_html_sidecar_stem(&mut sidecar_names, thumb);
        let pp3_name = format!("{stem}.pp3");
        let pp3_output = pp3_dir.join(&pp3_name);
        sidecar_exports.push((html_index, thumb.profile.clone(), pp3_output));
        thumb.pp3 = Some(PathBuf::from("pp3").join(pp3_name));
    }
    let mut hald_links = pool.install(|| {
        sidecar_exports
            .par_iter()
            .map(|(html_index, profile, pp3_output)| {
                let sidecar_context = SamplerSidecarContext {
                    raw: context.raw,
                    hald_level: context.hald_level,
                    profiles_root: context.profiles_root,
                    hald_dir: context.hald_dir,
                    color_noise_iso_threshold: context.color_noise_iso_threshold,
                    lens_corrections: context.lens_corrections,
                    dcp_profile: context.dcp_profile,
                };
                let hald = write_html_sampler_sidecars(profile, &sidecar_context, pp3_output)
                    .with_context(|| {
                        format!("exporting sampler sidecars for {}", profile.display())
                    })?;
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
    let html = render_sheet_html(&trie, context.columns, &detail_analysis)?;
    fs::write(output, html).with_context(|| format!("writing {}", output.display()))?;
    Ok(())
}

fn write_html_sampler_sidecars(
    profile: &Path,
    context: &SamplerSidecarContext<'_>,
    pp3_output: &Path,
) -> Result<PathBuf> {
    let temp_dir = Builder::new().prefix("mini-film-sampler-pp3-").tempdir()?;
    let resolved = profile_from_xmp_quiet(
        profile,
        context.hald_level,
        context.profiles_root,
        context.hald_dir,
        temp_dir.path(),
    )?;
    let hald_path = resolved
        .hald_path
        .as_ref()
        .context("resolved emulation did not produce a HALD path")?;
    let rawtherapee_profiles = rawtherapee_profiles_with_hald(&resolved, temp_dir.path())?;
    let rawtherapee_profiles = append_color_noise_to_profiles(
        rawtherapee_profiles,
        temp_dir.path(),
        context.raw,
        context.color_noise_iso_threshold,
    )?;
    let rawtherapee_profiles = append_lens_corrections_to_profiles(
        rawtherapee_profiles,
        temp_dir.path(),
        context.lens_corrections,
    )?;
    let mut rawtherapee_profiles = rawtherapee_profiles;
    append_dcp_or_auto_matched_curve(
        &mut rawtherapee_profiles,
        temp_dir.path(),
        context.dcp_profile,
    )?;
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
    cache: Option<&ThumbnailCache>,
) -> Result<()> {
    let Some(cache) = cache else {
        return write_progressive_html_thumbnail(
            convert,
            source,
            destination,
            jpg_quality,
            jpeg_subsampling,
        );
    };
    let cached = cache.progressive_html_path(source, jpg_quality, jpeg_subsampling)?;
    if fresh_decodable_sampler_image(&cached) {
        copy_file(&cached, destination)?;
        return Ok(());
    }

    write_progressive_html_thumbnail_to_cache(
        convert,
        source,
        &cached,
        jpg_quality,
        jpeg_subsampling,
    )?;
    copy_file(&cached, destination)?;
    Ok(())
}

fn write_html_baseline_thumbnail(
    context: &StructuredSheetContext<'_>,
    destination: &Path,
) -> Result<PathBuf> {
    let cached = context
        .cache
        .map(|cache| cache.html_original_path(context.jpg_quality, context.jpeg_subsampling));
    if let Some(cached) = cached.as_ref()
        && fresh_decodable_sampler_image(cached)
    {
        copy_file(cached, destination)?;
        return Ok(destination.to_path_buf());
    }

    let temp_dir = Builder::new()
        .prefix("mini-film-sampler-baseline-")
        .tempdir()?;
    let raw_source = temp_dir.path().join("raw-baseline.jpg");
    let mut profiles = Vec::new();
    append_dcp_or_auto_matched_curve(&mut profiles, temp_dir.path(), context.dcp_profile)?;
    let prepared_source = context.dng_fallback.prepare_known(context.raw)?;
    let outcome = run_raw_develop_jpeg(
        context.rawtherapee,
        &profiles,
        prepared_source,
        &raw_source,
        context.jpg_quality,
        context.jpeg_subsampling,
        None,
        true,
        context.dng_fallback,
    )?;
    if let Some(cached) = cached {
        write_progressive_html_thumbnail_to_cache(
            context.convert,
            &raw_source,
            &cached,
            context.jpg_quality,
            context.jpeg_subsampling,
        )?;
        copy_file(&cached, destination)?;
    } else {
        write_progressive_html_thumbnail(
            context.convert,
            &raw_source,
            destination,
            context.jpg_quality,
            context.jpeg_subsampling,
        )?;
    }
    context
        .dng_fallback
        .finish_successful_development(&outcome.source)?;
    Ok(destination.to_path_buf())
}

fn progressive_html_cache_path(
    cache_root: &Path,
    source: &Path,
    jpg_quality: u8,
    jpeg_subsampling: JpegSubsampling,
) -> Result<PathBuf> {
    let source_sha1 =
        sha1_file(source).with_context(|| format!("hashing thumbnail {}", source.display()))?;
    let subsampling = format!("{:?}", jpeg_subsampling).to_ascii_lowercase();
    let dir = cache_root.join("html-progressive");
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir.join(format!(
        "{source_sha1}-q{}-{subsampling}-progressive.jpg",
        jpg_quality.clamp(1, 100)
    )))
}

fn write_progressive_html_thumbnail_to_cache(
    convert: &Path,
    source: &Path,
    destination: &Path,
    jpg_quality: u8,
    jpeg_subsampling: JpegSubsampling,
) -> Result<()> {
    let parent = destination
        .parent()
        .context("HTML sampler cache file has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let temp = Builder::new()
        .prefix(".mini-film-sampler-progressive-")
        .suffix(".jpg")
        .tempfile_in(parent)
        .with_context(|| format!("creating temporary cache file in {}", parent.display()))?;
    write_progressive_html_thumbnail(convert, source, temp.path(), jpg_quality, jpeg_subsampling)?;
    decoded_sampler_image_dimensions(temp.path()).with_context(|| {
        format!(
            "generated HTML sampler image does not decode: {}",
            temp.path().display()
        )
    })?;
    temp.as_file()
        .sync_all()
        .with_context(|| format!("syncing {}", temp.path().display()))?;
    temp.persist(destination)
        .map(|_| ())
        .map_err(|err| err.error)
        .with_context(|| format!("publishing cache file {}", destination.display()))
}

fn copy_file(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::copy(source, destination)
        .with_context(|| format!("copying {} to {}", source.display(), destination.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let mut permissions = fs::metadata(destination)
            .with_context(|| format!("reading permissions for {}", destination.display()))?
            .permissions();
        permissions.set_mode(permissions.mode() | 0o044);
        fs::set_permissions(destination, permissions)
            .with_context(|| format!("making {} web-readable", destination.display()))?;
    }
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
    add_convert_thread_limit(&mut command, convert);
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

fn html_diffusion_thumbnail_file_name(index: usize, thumb: &SampleThumb) -> String {
    let profile_file_name = html_thumbnail_file_name(index, thumb);
    let stem = profile_file_name
        .strip_suffix(".jpg")
        .expect("HTML thumbnail names use a JPEG extension");
    format!("{stem}-diffusion.jpg")
}

fn render_sheet_html(
    trie: &ProfileTrie,
    columns: u32,
    detail_analysis: &SamplerDetailAnalysis,
) -> Result<String> {
    let templates = html_templates()?;
    let focus = sampler_detail_area(detail_analysis, SamplerDetailKind::Focus)?;
    let highlights = sampler_detail_area(detail_analysis, SamplerDetailKind::Highlights)?;
    let shadows = sampler_detail_area(detail_analysis, SamplerDetailKind::Shadows)?;
    let mut sections = String::new();
    for (part, child) in &trie.children {
        sections.push_str(&render_html_node(
            &templates,
            child,
            std::slice::from_ref(part),
            0,
        )?);
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
                "focus_x": focus.center_x,
                "focus_y": focus.center_y,
                "highlights_x": highlights.center_x,
                "highlights_y": highlights.center_y,
                "shadows_x": shadows.center_x,
                "shadows_y": shadows.center_y,
            }),
        )
        .context("rendering HTML sampler page")
}

fn sampler_detail_area(
    analysis: &SamplerDetailAnalysis,
    kind: SamplerDetailKind,
) -> Result<SamplerDetailArea> {
    analysis
        .areas
        .iter()
        .copied()
        .find(|area| area.kind == kind)
        .with_context(|| format!("sampler detail analysis is missing {kind:?}"))
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

fn relative_url(path: &Path) -> String {
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
        let profile_image = relative_url(&entry.thumb.image);
        tiles.push_str(
            &templates
                .render(
                    "tile",
                    &json!({
                        "label": entry.label,
                        "filename": entry.full_name,
                        "image": &profile_image,
                        "profile_image": &profile_image,
                        "diffusion_image": entry
                            .thumb
                            .diffusion_image
                            .as_ref()
                            .map(|path| relative_url(path))
                            .unwrap_or_else(String::new),
                        "original_image": entry
                            .thumb
                            .original_image
                            .as_ref()
                            .map(|path| relative_url(path))
                            .unwrap_or_else(String::new),
                        "xmp_href": file_url(&entry.thumb.profile),
                        "pp3_href": entry.thumb.pp3.as_ref().map(|path| relative_url(path)).unwrap_or_default(),
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

fn estimate_sampler_exif_duration() -> Duration {
    Duration::from_millis(700)
}

fn estimate_sampler_metadata_duration() -> Duration {
    Duration::from_millis(180)
}

fn estimate_sheet_duration(thumbs: usize) -> Duration {
    Duration::from_secs_f64((0.5 + thumbs as f64 * 0.01).clamp(1.0, 20.0))
}

fn attenuate_sampler_grain_amount(grain: GrainSettings, thumbnail_long_edge: u32) -> GrainSettings {
    if !grain.is_enabled() || thumbnail_long_edge >= 3000 {
        return grain;
    }

    let linear = (thumbnail_long_edge.max(1) as f32 / 3600.0).clamp(0.25, 1.0);
    let amount_scale = linear.sqrt().clamp(0.45, 1.0);

    GrainSettings {
        amount: scale_grain_byte(grain.amount, amount_scale),
        size: grain.size,
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
            diffusion_image: self.diffusion_image.clone(),
            original_image: self.original_image.clone(),
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

pub(crate) fn profile_display_name_from_relative(relative: &str) -> String {
    let stem = Path::new(relative)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(relative);
    stem.trim().to_string()
}

pub(crate) fn profile_name_parts(name: &str) -> Vec<String> {
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

fn build_sheet_layout(trie: &ProfileTrie, thumbnail_long_edge: u32, columns: u32) -> SheetLayout {
    let mut thumb = thumbnail_long_edge.max(64);
    let columns = sampler_sheet_columns(trie_thumb_count(trie), columns);
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
        ctx.render_node(child, std::slice::from_ref(part), 0);
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
            if index > 0 && (index as u32).is_multiple_of(columns) {
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

pub(crate) fn variant_sort_key(label: &str) -> String {
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
        let grainy_position = if marker_parts.contains(&"grainy") {
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
    if let Some(version) = part
        .strip_prefix('v')
        .or_else(|| part.strip_prefix('V'))
        .and_then(|version| version.parse::<u32>().ok())
    {
        return format!("v{version:06}");
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
    use filetime::{FileTime, set_file_mtime};

    fn sample_thumb(name: &str, width: u32, height: u32) -> SampleThumb {
        SampleThumb {
            image: PathBuf::from(format!("/tmp/{name}.jpg")),
            diffusion_image: None,
            original_image: None,
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

    fn sample_detail_analysis() -> SamplerDetailAnalysis {
        SamplerDetailAnalysis {
            areas: vec![
                SamplerDetailArea {
                    kind: SamplerDetailKind::Focus,
                    center_x: 0.25,
                    center_y: 0.35,
                },
                SamplerDetailArea {
                    kind: SamplerDetailKind::Highlights,
                    center_x: 0.65,
                    center_y: 0.2,
                },
                SamplerDetailArea {
                    kind: SamplerDetailKind::Shadows,
                    center_x: 0.75,
                    center_y: 0.8,
                },
            ],
        }
    }

    fn html_detail_coordinates(html: &str, region: &str) -> (f32, f32) {
        let region = html
            .split_once(&format!("data-region=\"{region}\""))
            .unwrap()
            .1;
        let coordinate = |name: &str| {
            region
                .split_once(&format!("data-center-{name}=\""))
                .unwrap()
                .1
                .split_once('"')
                .unwrap()
                .0
                .parse::<f32>()
                .unwrap()
        };
        (coordinate("x"), coordinate("y"))
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
            dng_fallback: DngFallbackConfig::default(),
            convert: PathBuf::from("convert"),
            lcp_root: None,
            no_grain: false,
            normalize_grain_mpix: Some(12.0),
            grain_seed: Some(123),
            grain_engine: GrainEngine::default(),
            diffusion: DiffusionSettings::default(),
            lens_corrections: crate::cli::LensCorrections::default(),
            color_noise_iso_threshold: 1600,
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
    fn enabled_diffusion_selects_the_sampler_tiff16_intermediate() {
        assert_eq!(
            SamplerIntermediateKind::for_diffusion(DiffusionSettings::default()),
            SamplerIntermediateKind::Jpeg8
        );
        assert_eq!(SamplerIntermediateKind::Jpeg8.filename(), "rawtherapee.jpg");

        let enabled = DiffusionSettings {
            method: mini_film::DiffusionMethod::MultiScaleMist,
            softness: 25,
            highlight_glow: 0,
            ..DiffusionSettings::default()
        };
        assert_eq!(
            SamplerIntermediateKind::for_diffusion(enabled),
            SamplerIntermediateKind::Tiff16
        );
        assert_eq!(
            SamplerIntermediateKind::Tiff16.filename(),
            "rawtherapee.tif"
        );
        assert_eq!(
            sampler_intermediate_kind(SheetOutputKind::Html, DiffusionSettings::default()),
            SamplerIntermediateKind::Tiff16,
            "HTML comparisons always branch from one 16-bit intermediate"
        );
        assert_eq!(
            sampler_intermediate_kind(SheetOutputKind::Jpeg, DiffusionSettings::default()),
            SamplerIntermediateKind::Jpeg8,
            "the static JPEG sampler keeps its existing fast path"
        );
    }

    #[test]
    fn html_diffusion_uses_medium_mist_by_default_and_honors_enabled_settings() {
        assert_eq!(
            html_sampler_diffusion_settings(DiffusionSettings::default()),
            DiffusionPreset::Medium.settings(DiffusionMethod::MultiScaleMist)
        );

        let configured = DiffusionSettings {
            method: DiffusionMethod::EdgeAwareGlow,
            softness: 31,
            highlight_glow: 62,
            softness_radius_percent: 175,
            glow_radius_percent: 240,
            intensity_percent: 190,
            highlight_reach: 72,
        };
        assert_eq!(
            html_sampler_diffusion_settings(configured),
            configured.canonical_render_settings()
        );
    }

    #[test]
    fn profile_names_strip_extension_and_split_into_visible_levels() {
        assert_eq!(
            profile_display_name_from_relative("Kodak Portra 400 Grainy.xmp"),
            "Kodak Portra 400 Grainy"
        );
        assert_eq!(
            profile_display_name_from_relative("Provider/Kodak Portra 400.xmp"),
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

        let layout = build_sheet_layout(&trie, 128, 8);
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

        let layout = build_sheet_layout(&trie, 256, 8);
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

        let layout = build_sheet_layout(&trie, 256, 8);
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

        let layout = build_sheet_layout(&trie, 256, 8);
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

        let layout = build_sheet_layout(&trie, 256, 8);
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

        let layout = build_sheet_layout(&trie, 1024, 8);

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
    fn jpeg_sampler_layout_preserves_thumbnail_size_for_small_output() {
        let mut trie = ProfileTrie::default();
        for index in 0..16 {
            trie.insert(sample_thumb(&format!("Kodak Portra {index}"), 1024, 683));
        }

        let layout = build_sheet_layout(&trie, 1024, 4);
        let svg = render_sheet_svg(&layout);

        assert!(layout.width < 5000);
        assert!(svg.contains(r#"width="996" height="664""#));
    }

    #[test]
    fn sampler_accepts_jpeg_and_html_outputs() {
        assert_eq!(
            sampler_output_kind(Path::new("sheet.jpg")).unwrap(),
            Some(SheetOutputKind::Jpeg)
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
        thumb.diffusion_image = Some(PathBuf::from("thumbnails/kodak-diffusion.jpg"));
        thumb.original_image = Some(PathBuf::from("thumbnails/original.jpg"));
        thumb.pp3 = Some(PathBuf::from("pp3/Kodak Portra 400 Grainy.pp3"));
        thumb.hald = Some(PathBuf::from("/tmp/Kodak Portra 400 Grainy.hald.png"));
        trie.insert(thumb);

        let html = render_sheet_html(&trie, 4, &sample_detail_analysis()).unwrap();

        assert!(html.contains("<!doctype html>"));
        assert!(html.contains("--columns: 4"));
        let normalized_html = html.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            normalized_html.contains(
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
        assert!(html.contains("data-profile=\"thumbnails/kodak.jpg\""));
        assert!(html.contains("data-diffusion=\"thumbnails/kodak-diffusion.jpg\""));
        assert!(html.contains("data-original=\"thumbnails/original.jpg\""));
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
        assert!(html.contains("class=\"detail-rail\""));
        assert!(html.contains("data-region=\"focus\""));
        assert!(html.contains("data-region=\"highlights\""));
        assert!(html.contains("data-region=\"shadows\""));
        assert_eq!(html_detail_coordinates(&html, "focus"), (0.25, 0.35));
        assert_eq!(html_detail_coordinates(&html, "highlights"), (0.65, 0.2));
        assert_eq!(html_detail_coordinates(&html, "shadows"), (0.75, 0.8));
        assert!(html.contains("--detail-size: clamp(144px, 16vw, 192px)"));
        assert!(html.contains("max-height: calc(100vh - 128px)"));
        assert!(html.contains("https://github.com/alfanick/mini-film"));
        let version = env!("CARGO_PKG_VERSION");
        assert!(html.contains(&format!("mini-film</a> {version}")));
        assert!(html.contains("Picture by Amadeus Juskowiak"));
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
    fn html_profile_and_diffusion_thumbnails_have_distinct_names() {
        let thumb = sample_thumb("Ilford FP4", 1024, 683);

        assert_eq!(html_thumbnail_file_name(7, &thumb), "0007-Ilford FP4.jpg");
        assert_eq!(
            html_diffusion_thumbnail_file_name(7, &thumb),
            "0007-Ilford FP4-diffusion.jpg"
        );
    }

    #[test]
    fn profile_filenames_strip_only_xmp_extension_for_display() {
        assert_eq!(
            profile_filename_without_xmp("Provider/Kodak Portra 400.xmp"),
            "Provider/Kodak Portra 400"
        );
        assert_eq!(
            profile_filename_without_xmp("Provider/Kodak Portra 400.XMP"),
            "Provider/Kodak Portra 400"
        );
        assert_eq!(
            profile_filename_without_xmp("Provider/Kodak Portra 400"),
            "Provider/Kodak Portra 400"
        );
    }

    #[test]
    fn html_links_escape_spaces_for_file_and_relative_urls() {
        assert_eq!(
            file_url(Path::new("/tmp/Film Profiles/Ilford FP4.xmp")),
            "file:///tmp/Film%20Profiles/Ilford%20FP4.xmp"
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
        args.grain_seed = None;
        let cache = ThumbnailCache::new(&raw, None).unwrap();
        let cached = cache.path_for(&xmp, &args).unwrap();
        let name = cached.file_name().unwrap().to_string_lossy();

        assert!(cached.starts_with(env::temp_dir().join("mini-film-sampler-cache")));
        assert!(name.contains(&sha1_file(&raw).unwrap()));
        assert!(name.contains(&sha1_file(&xmp).unwrap()));
        assert!(name.contains(RAW_RENDER_PIPELINE_KEY));
        assert!(name.contains("512px"));
        assert!(!name.contains("seed-"));
        assert!(!name.contains("noise1600"));

        args.thumbnail_long_edge = 1024;
        let larger = cache.path_for(&xmp, &args).unwrap();
        assert_ne!(cached, larger);

        args.thumbnail_long_edge = 512;
        args.normalize_grain_mpix = Some(24.0);
        let custom_normalization = cache.path_for(&xmp, &args).unwrap();
        args.normalize_grain_mpix = None;
        let disabled_normalization = cache.path_for(&xmp, &args).unwrap();
        assert_ne!(cached, custom_normalization);
        assert_ne!(cached, disabled_normalization);
        assert_ne!(custom_normalization, disabled_normalization);

        args.color_noise_iso_threshold = 0;
        let no_noise = cache.path_for(&xmp, &args).unwrap();
        args.color_noise_iso_threshold = 6400;
        let with_noise = cache.path_for(&xmp, &args).unwrap();
        assert_ne!(no_noise, with_noise);

        args.grain_seed = Some(456);
        let other_seed = cache.path_for(&xmp, &args).unwrap();
        assert_ne!(with_noise, other_seed);
        let explicit_seed_pair_base = cache.html_pair_base_path(&xmp, &args).unwrap();
        args.grain_seed = None;
        let automatic_seed = cache.path_for(&xmp, &args).unwrap();
        assert_ne!(other_seed, automatic_seed);
        assert_eq!(
            explicit_seed_pair_base,
            cache.html_pair_base_path(&xmp, &args).unwrap(),
            "the pair identity keeps its existing separate grain-seed suffix"
        );

        args.diffusion = DiffusionSettings {
            method: mini_film::DiffusionMethod::MultiScaleMist,
            softness: 50,
            highlight_glow: 50,
            ..DiffusionSettings::default()
        };
        let mist_diffusion = cache.path_for(&xmp, &args).unwrap();
        assert_ne!(with_noise, mist_diffusion);
        assert!(
            mist_diffusion
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("diffusion-v1-MultiScaleMist-50-50")
        );

        args.diffusion.intensity_percent = 150;
        let advanced_mist_diffusion = cache.path_for(&xmp, &args).unwrap();
        assert_ne!(mist_diffusion, advanced_mist_diffusion);
        assert!(
            advanced_mist_diffusion
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("diffusion-v2-")
        );

        args.diffusion.highlight_reach = 100;
        assert_eq!(
            advanced_mist_diffusion,
            cache.path_for(&xmp, &args).unwrap(),
            "highlight reach cannot affect the multi-scale mist cache identity"
        );
        args.diffusion.glow_radius_percent = 150;
        assert_ne!(
            advanced_mist_diffusion,
            cache.path_for(&xmp, &args).unwrap(),
            "effective advanced controls must change the sampler cache identity"
        );
        args.diffusion.glow_radius_percent = 100;

        let dcp_cache = ThumbnailCache {
            dir: cache.dir.clone(),
            raw_sha1: cache.raw_sha1.clone(),
            dcp_identity: format!("dcp-{}", "f".repeat(40)),
        };
        args.normalize_grain_mpix = Some(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX);
        let dcp_advanced = dcp_cache.path_for(&xmp, &args).unwrap();
        assert!(
            dcp_advanced.file_name().unwrap().to_string_lossy().len() <= 255,
            "advanced diffusion cache names must fit Linux NAME_MAX"
        );

        args.diffusion = DiffusionSettings {
            method: mini_film::DiffusionMethod::MultiScaleMist,
            softness: 50,
            highlight_glow: 50,
            ..DiffusionSettings::default()
        };
        let dcp_neutral = dcp_cache.path_for(&xmp, &args).unwrap();
        let dcp_neutral_name = dcp_neutral.file_name().unwrap().to_string_lossy();
        assert!(dcp_neutral_name.len() <= 255);
        assert!(dcp_neutral_name.contains("cache-v1-"));

        args.diffusion.intensity_percent = 100;
        args.diffusion.method = mini_film::DiffusionMethod::EdgeAwareGlow;
        let edge_aware_diffusion = cache.path_for(&xmp, &args).unwrap();
        assert_ne!(mist_diffusion, edge_aware_diffusion);
    }

    #[test]
    fn sampler_cache_freshness_rejects_expired_future_and_corrupt_images() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("thumbnail.jpg");
        image::RgbImage::from_pixel(8, 6, image::Rgb([10, 20, 30]))
            .save(&image)
            .unwrap();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(2 * 24 * 60 * 60);

        set_file_mtime(&image, FileTime::from_system_time(now - SAMPLER_CACHE_TTL)).unwrap();
        assert!(fresh_decodable_sampler_image_at(&image, now));

        set_file_mtime(
            &image,
            FileTime::from_system_time(now - SAMPLER_CACHE_TTL - Duration::from_secs(1)),
        )
        .unwrap();
        assert!(!fresh_decodable_sampler_image_at(&image, now));

        set_file_mtime(
            &image,
            FileTime::from_system_time(now + Duration::from_secs(1)),
        )
        .unwrap();
        assert!(!fresh_decodable_sampler_image_at(&image, now));

        fs::write(&image, b"not a JPEG").unwrap();
        set_file_mtime(&image, FileTime::from_system_time(now)).unwrap();
        assert!(!fresh_decodable_sampler_image_at(&image, now));
    }

    #[cfg(unix)]
    #[test]
    fn copied_sampler_images_are_web_readable() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("cached.jpg");
        let destination = dir.path().join("published.jpg");
        fs::write(&source, b"cached sampler image").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();

        copy_file(&source, &destination).unwrap();

        assert_eq!(
            fs::metadata(destination).unwrap().permissions().mode() & 0o444,
            0o444
        );
    }

    #[test]
    fn no_cache_bypasses_sampler_cache_initialization() {
        let missing = Path::new("/definitely/missing/mini-film-cache-input.dng");

        assert!(sampler_cache(missing, None, true).unwrap().is_none());
        assert!(sampler_cache(missing, None, false).is_err());
    }

    #[test]
    fn html_original_and_progressive_cache_paths_track_render_identity() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("profile.jpg");
        fs::write(&source, b"profile pixels").unwrap();
        let cache = ThumbnailCache {
            dir: dir.path().join("cache"),
            raw_sha1: "raw-sha1".to_string(),
            dcp_identity: "dcp-one".to_string(),
        };

        let original = cache.html_original_path(95, JpegSubsampling::S444);
        assert!(original.starts_with(&cache.dir));
        assert!(
            original
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("html-original-v1")
        );
        assert_ne!(
            original,
            cache.html_original_path(90, JpegSubsampling::S444)
        );
        assert_ne!(
            original,
            cache.html_original_path(95, JpegSubsampling::S420)
        );
        let other_dcp = ThumbnailCache {
            dir: cache.dir.clone(),
            raw_sha1: cache.raw_sha1.clone(),
            dcp_identity: "dcp-two".to_string(),
        };
        assert_ne!(
            original,
            other_dcp.html_original_path(95, JpegSubsampling::S444)
        );

        let progressive = cache
            .progressive_html_path(&source, 95, JpegSubsampling::S444)
            .unwrap();
        assert!(progressive.starts_with(cache.dir.join("html-progressive")));
        assert_ne!(
            progressive,
            cache
                .progressive_html_path(&source, 90, JpegSubsampling::S444)
                .unwrap()
        );
        fs::write(&source, b"different profile pixels").unwrap();
        assert_ne!(
            progressive,
            cache
                .progressive_html_path(&source, 95, JpegSubsampling::S444)
                .unwrap()
        );
    }

    #[test]
    fn sampler_detail_analysis_cache_reuses_valid_data_and_recovers_from_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("original.jpg");
        let mut image = image::RgbImage::new(320, 200);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let value = ((x * 3 + y * 5) % 256) as u8;
            *pixel = image::Rgb([value, value, value]);
        }
        image.save(&source).unwrap();
        let focus_regions = vec![GalleryFocusRegion {
            x: 0.1,
            y: 0.2,
            width: 0.1,
            height: 0.1,
            primary: true,
        }];
        let cache = ThumbnailCache {
            dir: dir.path().join("cache"),
            raw_sha1: "raw-sha1".to_string(),
            dcp_identity: "dcp-none".to_string(),
        };

        let computed = load_or_analyze_sampler_details(
            Path::new("convert"),
            &source,
            &focus_regions,
            Some(&cache),
        )
        .unwrap();
        assert!(computed.is_valid());
        let source_sha1 = sha1_file(&source).unwrap();
        let focus_signature = sampler_focus_signature(&focus_regions).unwrap();
        let cache_path = cache.html_detail_analysis_path(&source_sha1, &focus_signature);
        assert!(cache_path.is_file());

        let replacement = sample_detail_analysis();
        let cached = CachedSamplerDetailAnalysis {
            analysis_version: SAMPLER_DETAIL_ANALYSIS_VERSION.to_string(),
            source_sha1,
            focus_signature,
            source_width: 320,
            source_height: 200,
            analysis: replacement.clone(),
        };
        write_cache_file_atomically(&serde_json::to_vec(&cached).unwrap(), &cache_path).unwrap();
        assert_eq!(
            load_or_analyze_sampler_details(
                Path::new("convert"),
                &source,
                &focus_regions,
                Some(&cache),
            )
            .unwrap(),
            replacement
        );

        fs::write(&cache_path, b"not json").unwrap();
        assert_eq!(
            load_or_analyze_sampler_details(
                Path::new("convert"),
                &source,
                &focus_regions,
                Some(&cache),
            )
            .unwrap(),
            computed
        );

        fs::write(&cache_path, b"leave untouched").unwrap();
        assert_eq!(
            load_or_analyze_sampler_details(Path::new("convert"), &source, &focus_regions, None,)
                .unwrap(),
            computed
        );
        assert_eq!(fs::read(&cache_path).unwrap(), b"leave untouched");
    }

    #[test]
    fn html_pair_cache_identity_tracks_effective_diffusion_profile_and_seed() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("input.dng");
        let xmp = dir.path().join("profile.xmp");
        let same_xmp_elsewhere = dir.path().join("same-profile.xmp");
        fs::write(&raw, b"raw bytes").unwrap();
        fs::write(&xmp, b"xmp bytes").unwrap();
        fs::write(&same_xmp_elsewhere, b"xmp bytes").unwrap();
        let mut args = sampler_args_for_cache(
            raw.clone(),
            dir.path().to_path_buf(),
            dir.path().join("hald"),
        );
        let cache = ThumbnailCache {
            dir: dir.path().join("cache"),
            raw_sha1: sha1_file(&raw).unwrap(),
            dcp_identity: "dcp-none".to_string(),
        };

        let default_pair = cache.html_pair_paths(&xmp, 4, &args).unwrap();
        assert_ne!(default_pair.profile_image, default_pair.diffusion_image);
        assert_ne!(default_pair.profile_image, default_pair.manifest);
        assert!(
            default_pair
                .profile_image
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("-profile.jpg")
        );

        args.diffusion = DiffusionPreset::Strong.settings(DiffusionMethod::MultiScaleMist);
        let strong_pair = cache.html_pair_paths(&xmp, 4, &args).unwrap();
        assert_ne!(default_pair.profile_image, strong_pair.profile_image);

        args.diffusion.highlight_reach = 100;
        assert_eq!(
            strong_pair.profile_image,
            cache.html_pair_paths(&xmp, 4, &args).unwrap().profile_image,
            "mist highlight reach is render-inert and must not invalidate the pair"
        );
        assert_ne!(
            strong_pair.profile_image,
            cache
                .html_pair_paths(&same_xmp_elsewhere, 4, &args)
                .unwrap()
                .profile_image,
            "content-identical profiles use different deterministic grain seeds"
        );
        assert_ne!(
            strong_pair.profile_image,
            cache.html_pair_paths(&xmp, 5, &args).unwrap().profile_image,
            "the profile index participates in the deterministic grain seed"
        );

        args.grain_seed = Some(456);
        assert_ne!(
            strong_pair.profile_image,
            cache.html_pair_paths(&xmp, 4, &args).unwrap().profile_image
        );
    }

    #[test]
    fn html_pair_cache_requires_a_fully_decodable_matching_manifest_pair() {
        let dir = tempfile::tempdir().unwrap();
        let sources = dir.path().join("sources");
        let cache_dir = dir.path().join("cache");
        fs::create_dir_all(&sources).unwrap();
        fs::create_dir_all(&cache_dir).unwrap();
        let profile_source = sources.join("profile.jpg");
        let diffusion_source = sources.join("diffusion.jpg");
        let mismatched_source = sources.join("mismatched.jpg");
        image::RgbImage::from_pixel(8, 6, image::Rgb([10, 20, 30]))
            .save(&profile_source)
            .unwrap();
        image::RgbImage::from_pixel(8, 6, image::Rgb([40, 50, 60]))
            .save(&diffusion_source)
            .unwrap();
        image::RgbImage::from_pixel(7, 6, image::Rgb([70, 80, 90]))
            .save(&mismatched_source)
            .unwrap();
        let destinations = HtmlPairCachePaths {
            profile_image: cache_dir.join("pair-profile.jpg"),
            diffusion_image: cache_dir.join("pair-diffusion.jpg"),
            manifest: cache_dir.join("pair.pair"),
        };

        copy_html_pair_to_cache(&profile_source, &diffusion_source, &destinations).unwrap();
        assert!(destinations.is_valid());

        image::RgbImage::from_pixel(8, 6, image::Rgb([90, 80, 70]))
            .save(&destinations.diffusion_image)
            .unwrap();
        assert!(
            !destinations.is_valid(),
            "a valid but stale replacement must not match the committed pair manifest"
        );

        copy_html_pair_to_cache(&profile_source, &diffusion_source, &destinations).unwrap();
        fs::write(&destinations.diffusion_image, [0xff, 0xd8, 0xff, 0xe0]).unwrap();
        assert!(
            !destinations.is_valid(),
            "a JPEG-like header without decodable pixels must be rejected"
        );

        copy_html_pair_to_cache(&profile_source, &diffusion_source, &destinations).unwrap();
        assert!(
            copy_html_pair_to_cache(&profile_source, &mismatched_source, &destinations).is_err()
        );
        assert!(
            destinations.is_valid(),
            "a rejected source pair must not invalidate the last committed cache entry"
        );

        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(2 * 24 * 60 * 60);
        for path in [
            &destinations.profile_image,
            &destinations.diffusion_image,
            &destinations.manifest,
        ] {
            set_file_mtime(path, FileTime::from_system_time(now)).unwrap();
        }
        assert!(destinations.is_valid_at(now));

        set_file_mtime(
            &destinations.manifest,
            FileTime::from_system_time(now - SAMPLER_CACHE_TTL - Duration::from_secs(1)),
        )
        .unwrap();
        assert!(!destinations.is_valid_at(now));

        set_file_mtime(&destinations.manifest, FileTime::from_system_time(now)).unwrap();
        set_file_mtime(
            &destinations.profile_image,
            FileTime::from_system_time(now + Duration::from_secs(1)),
        )
        .unwrap();
        assert!(!destinations.is_valid_at(now));
    }

    #[test]
    fn sampler_grain_is_attenuated_for_small_thumbnails() {
        let grain = GrainSettings {
            amount: 50,
            size: 50,
            frequency: 50,
        };

        let small = attenuate_sampler_grain_amount(grain, 512);
        let medium = attenuate_sampler_grain_amount(grain, 1024);
        let full = attenuate_sampler_grain_amount(grain, 4000);

        assert!(small.amount < grain.amount);
        assert_eq!(small.size, grain.size);
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
