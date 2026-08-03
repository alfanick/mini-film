use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use notify::{
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{AccessKind, ModifyKind},
};
use tempfile::{Builder, TempPath};
use walkdir::WalkDir;

use mini_film::{DiffusionSettings, GrainEngine};

use crate::app::apply::{
    ApplyArgs, ApplyJob, apply_resolved, resolve_apply_effects, resolve_grain_override,
};
use crate::app::auto_import::{AutoImportConfig, AutoImportReceiver, start_auto_import};
use crate::app::cache::DAEMON_PROFILE_OUTPUTS_CACHE_DIR;
use crate::app::dng::DngFallbackConfig;
use crate::app::export::validate_export_options;
use crate::app::info::profile_info_text_for_selector;
use crate::app::managed_symlink::{ensure_directory_symlink, ensure_file_symlink};
use crate::app::nikon_wtu::{NikonWtuConfig, NikonWtuReceiver, start_nikon_wtu_receiver};
use crate::app::profile::{ResolvedProfile, neutral_profile, resolve_profile};
use crate::app::progress::{
    ApplyProgress, StageEstimates, batch_progress_style, file_progress_style, format_duration,
    progress_length,
};
use crate::app::review::{
    ReviewConfig, ReviewGalleryConfig, ReviewHandle, ReviewProfile, ReviewProfileMetadata,
    ReviewRenderPriorityKey, ReviewRenderPrioritySnapshot, SOOC_PROFILE_INDEX, SOOC_PROFILE_STEM,
    review_profile_identity, start_review_server,
};
use crate::app::system_stats::{ResourceUsageSummary, sample_usage_block};
use crate::app::util::{
    InputFileFilter, coalesce_due_input_sidecars, coalesce_input_sidecars, cpu_thread_count,
    half_cpu_thread_count, input_filter_name, is_jpeg_input_file, is_raw_input_file,
    is_rendered_input_file, is_supported_input_file, matching_raw_for_sidecar,
    matching_sidecar_for_raw, time_of_day_seed,
};
use crate::cli::{
    BatchOutputFormat, CodexAnalysisFlags, ExportOptions, GalleryTemplate, LensCorrections,
};
use indicatif::{MultiProgress, ProgressBar};

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(75);
const RESOURCE_USAGE_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

pub(crate) struct BatchDaemonArgs {
    pub(crate) input: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) profile: Vec<String>,
    pub(crate) input_file_filter: InputFileFilter,
    pub(crate) hald_dir: PathBuf,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_level: u32,
    pub(crate) rawtherapee: PathBuf,
    pub(crate) dng_fallback: DngFallbackConfig,
    pub(crate) convert: PathBuf,
    pub(crate) no_grain: bool,
    pub(crate) normalize_grain_mpix: Option<f64>,
    pub(crate) lcp_root: Option<PathBuf>,
    pub(crate) lens_corrections: LensCorrections,
    pub(crate) grain: Option<String>,
    pub(crate) grain_preset: Option<String>,
    pub(crate) grain_seed: Option<u64>,
    pub(crate) grain_engine: GrainEngine,
    pub(crate) diffusion: DiffusionSettings,
    pub(crate) color_noise_iso_threshold: u32,
    pub(crate) jobs: Option<usize>,
    pub(crate) debounce_seconds: u64,
    pub(crate) auto_import: bool,
    pub(crate) nikon_wtu: Option<String>,
    pub(crate) nikon_wtu_port: u16,
    pub(crate) nikon_wtu_name: Option<String>,
    pub(crate) nikon_wtu_guid: Option<String>,
    pub(crate) review_address: Option<String>,
    pub(crate) hugin_bin_dir: Option<PathBuf>,
    pub(crate) codex: Option<CodexAnalysisFlags>,
    pub(crate) codex_binary: PathBuf,
    pub(crate) codex_model: String,
    pub(crate) codex_timeout: u64,
    pub(crate) gallery: Option<GalleryTemplate>,
    pub(crate) gallery_thumbnail_long_edge: u32,
    pub(crate) gallery_columns: u32,
    pub(crate) publish_album: String,
    pub(crate) output_format: BatchOutputFormat,
    pub(crate) export: ExportOptions,
    pub(crate) invocation: Option<String>,
}

struct DaemonProfile {
    selector: String,
    stem: String,
    resolved: ResolvedProfile,
    profile_report: String,
}

struct PendingTask {
    raw: PathBuf,
    kind: DaemonTaskKind,
    key: PendingTaskKey,
    enqueue_sequence: u64,
}

#[derive(Clone, Debug)]
enum DaemonTaskKind {
    RawProfile(usize),
    StandaloneCompressed,
    SoocSidecar { sidecar: PathBuf },
}

impl DaemonTaskKind {
    fn review_profile_index(&self) -> Option<usize> {
        match self {
            Self::RawProfile(profile_index) => Some(*profile_index),
            Self::SoocSidecar { .. } => Some(SOOC_PROFILE_INDEX),
            Self::StandaloneCompressed => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PendingTaskSlot {
    Profile(usize),
    StandaloneCompressed,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PendingImageIdentity {
    Review(u64),
    Path(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PendingTaskKey {
    image: PendingImageIdentity,
    slot: PendingTaskSlot,
}

impl PendingTaskKey {
    fn new(raw: &Path, kind: &DaemonTaskKind, review_image_id: Option<u64>) -> Self {
        let image = review_image_id.map_or_else(
            || PendingImageIdentity::Path(raw.to_path_buf()),
            PendingImageIdentity::Review,
        );
        let slot = match kind {
            DaemonTaskKind::RawProfile(profile_index) => PendingTaskSlot::Profile(*profile_index),
            DaemonTaskKind::SoocSidecar { .. } => PendingTaskSlot::Profile(SOOC_PROFILE_INDEX),
            DaemonTaskKind::StandaloneCompressed => PendingTaskSlot::StandaloneCompressed,
        };
        Self { image, slot }
    }

    fn review_image_id(&self) -> Option<u64> {
        match self.image {
            PendingImageIdentity::Review(image_id) => Some(image_id),
            PendingImageIdentity::Path(_) => None,
        }
    }
}

#[derive(Default)]
struct PendingTasks {
    tasks: Vec<PendingTask>,
    next_sequence: u64,
}

impl PendingTasks {
    fn push(&mut self, raw: PathBuf, kind: DaemonTaskKind, review_image_id: Option<u64>) -> bool {
        let key = PendingTaskKey::new(&raw, &kind, review_image_id);
        if let Some(existing) = self.tasks.iter_mut().find(|task| task.key == key) {
            existing.raw = raw;
            existing.kind = kind;
            return false;
        }

        let enqueue_sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .expect("daemon pending task sequence exhausted");
        self.tasks.push(PendingTask {
            raw,
            kind,
            key,
            enqueue_sequence,
        });
        true
    }

    fn len(&self) -> usize {
        self.tasks.len()
    }

    fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    fn has_unblocked(&self, excluded: &HashSet<PendingTaskKey>) -> bool {
        self.tasks.iter().any(|task| !excluded.contains(&task.key))
    }

    fn contains_key(&self, key: &PendingTaskKey) -> bool {
        self.tasks.iter().any(|task| &task.key == key)
    }

    fn pop_fifo_excluding(&mut self, excluded: &HashSet<PendingTaskKey>) -> Option<PendingTask> {
        let index = self
            .tasks
            .iter()
            .position(|task| !excluded.contains(&task.key))?;
        Some(self.tasks.remove(index))
    }

    fn drop_unschedulable<K>(
        &mut self,
        mut key_for: impl FnMut(&PendingTask) -> Option<K>,
    ) -> usize {
        let before = self.tasks.len();
        self.tasks.retain(|task| key_for(task).is_some());
        before - self.tasks.len()
    }

    fn pop_ranked_excluding<K: Ord>(
        &mut self,
        excluded: &HashSet<PendingTaskKey>,
        mut key_for: impl FnMut(&PendingTask) -> Option<K>,
    ) -> Option<PendingTask> {
        let index = self
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, task)| !excluded.contains(&task.key))
            .filter_map(|(index, task)| key_for(task).map(|key| (index, key)))
            .min_by(|(_, left), (_, right)| left.cmp(right))
            .map(|(index, _)| index)?;
        Some(self.tasks.remove(index))
    }
}

fn review_pending_task_key(
    snapshot: &ReviewRenderPrioritySnapshot,
    task: &PendingTask,
) -> Option<ReviewRenderPriorityKey> {
    snapshot.key_for(
        task.key.review_image_id(),
        task.kind.review_profile_index(),
        task.enqueue_sequence,
    )
}

struct InFlightTask {
    key: PendingTaskKey,
    kind: DaemonTaskKind,
    raw: PathBuf,
    handle: thread::JoinHandle<DaemonFileResult>,
}

struct PendingFile {
    path: PathBuf,
    process_at: Instant,
    size: u64,
    modified: Option<std::time::SystemTime>,
}

struct DaemonProgressState {
    total_processed: u64,
    total_succeeded: u64,
    total_failed: u64,
    total_elapsed_ms: u64,
    started_at: Instant,
    files: Vec<DaemonFileResult>,
    profile_stats: Vec<DaemonProfileStats>,
    profile_output_dirs: Vec<HashSet<PathBuf>>,
    resource_usage: ResourceUsageSummary,
    last_resource_sample: Instant,
}

#[derive(Default, Clone, Copy)]
struct DaemonProfileStats {
    processed: u64,
    succeeded: u64,
    failed: u64,
    elapsed_ms: u64,
}

impl DaemonProfileStats {
    fn avg_ms(&self) -> u64 {
        if self.processed == 0 {
            return 0;
        }
        self.elapsed_ms / self.processed
    }
}

struct DaemonFileResult {
    raw: PathBuf,
    output: PathBuf,
    duration: Duration,
    profile_index: Option<usize>,
    dcp_profile_filename: Option<String>,
    error: Option<String>,
    lens_profile_status: String,
    sharpening_status: String,
    denoise_status: String,
}

fn lens_profile_status(corrections: LensCorrections, applied: bool) -> String {
    if !corrections.is_enabled() {
        return "lens-profile: disabled".to_string();
    }

    let mut parts = Vec::new();
    if corrections.distortion {
        parts.push("distortion");
    }
    if corrections.ca {
        parts.push("chromatic-aberration");
    }
    if corrections.vignetting {
        parts.push("vignetting");
    }

    format!(
        "lens-profile: requested[{}], found=true, applied={}",
        parts.join("+"),
        if applied { "yes" } else { "no" }
    )
}

impl Clone for DaemonFileResult {
    fn clone(&self) -> Self {
        Self {
            raw: self.raw.clone(),
            output: self.output.clone(),
            duration: self.duration,
            profile_index: self.profile_index,
            dcp_profile_filename: self.dcp_profile_filename.clone(),
            error: self.error.clone(),
            lens_profile_status: self.lens_profile_status.clone(),
            sharpening_status: self.sharpening_status.clone(),
            denoise_status: self.denoise_status.clone(),
        }
    }
}

impl DaemonProgressState {
    fn profile_stats_mut(&mut self, profile_index: usize) -> &mut DaemonProfileStats {
        while self.profile_stats.len() <= profile_index {
            self.profile_stats.push(DaemonProfileStats::default());
        }
        &mut self.profile_stats[profile_index]
    }

    fn record(&mut self, result: &DaemonFileResult) {
        self.total_processed += 1;
        self.total_elapsed_ms += result.duration.as_millis() as u64;
        if result.error.is_some() {
            self.total_failed += 1;
        } else {
            self.total_succeeded += 1;
        }
        if let Some(profile_index) = result
            .profile_index
            .filter(|profile_index| *profile_index < self.profile_stats.len())
        {
            let profile = self.profile_stats_mut(profile_index);
            profile.processed += 1;
            profile.elapsed_ms += result.duration.as_millis() as u64;
            if result.error.is_some() {
                profile.failed += 1;
            } else {
                profile.succeeded += 1;
            }
        }
        self.files.push(result.clone());
        if let (Some(profile_index), Some(parent)) = (
            result
                .profile_index
                .filter(|profile_index| *profile_index < self.profile_output_dirs.len()),
            result.output.parent(),
        ) {
            self.profile_output_dirs_mut(profile_index)
                .insert(parent.to_path_buf());
        }
        if self.files.len() > 3000 {
            self.files.remove(0);
        }
    }

    fn profile_output_dirs_mut(&mut self, profile_index: usize) -> &mut HashSet<PathBuf> {
        while self.profile_output_dirs.len() <= profile_index {
            self.profile_output_dirs.push(HashSet::new());
        }
        &mut self.profile_output_dirs[profile_index]
    }

    fn sample_resources(&mut self) {
        if let Some(usage) = sample_usage_block() {
            self.resource_usage.add(&usage);
        }
    }

    fn resource_usage_if_needed(&mut self, now: Instant) {
        if now.duration_since(self.last_resource_sample) >= RESOURCE_USAGE_SAMPLE_INTERVAL {
            self.sample_resources();
            self.last_resource_sample = now;
        }
    }
}

struct ProfileScheduleContext<'a> {
    input_root: &'a Path,
    output_root: &'a Path,
    output_format: BatchOutputFormat,
    input_file_filter: InputFileFilter,
    skip_existing: bool,
    review: Option<&'a ReviewHandle>,
}

/// Run a watcher that applies one or more profiles whenever RAW files appear.
///
/// The input folder is monitored recursively. New/changed RAW files are queued on
/// filesystem notifications and only processed after their size and mtime are
/// observed as stable.
pub(crate) fn run_batch_daemon(mut args: BatchDaemonArgs) -> Result<()> {
    validate_export_options(&args.export)?;
    let jobs = resolve_batch_daemon_jobs(args.jobs)?;
    if args.auto_import && !cfg!(target_os = "linux") {
        bail!("--auto-import is supported only on Linux with mounted GVfs PTP/MTP cameras");
    }
    if !args.input.is_dir() {
        bail!("daemon input is not a directory: {}", args.input.display());
    }
    fs::create_dir_all(&args.output)
        .with_context(|| format!("creating {}", args.output.display()))?;
    args.input = fs::canonicalize(&args.input)
        .with_context(|| format!("canonicalizing {}", args.input.display()))?;
    args.output = fs::canonicalize(&args.output)
        .with_context(|| format!("canonicalizing {}", args.output.display()))?;
    ensure_directory_symlink(&args.input, &args.output.join("originals"))
        .context("creating output originals symlink")?;
    if args.codex.is_some() && args.review_address.is_none() {
        bail!(
            "--codex requires --review-address so generated ratings, tags, and notes can be stored in review state"
        );
    }

    let debounce = Duration::from_secs(args.debounce_seconds);
    let temp_dir = Builder::new().prefix("mini-film-daemon-").tempdir()?;
    let start = Instant::now();
    let nikon_wtu_config = if let Some(camera) = args
        .nikon_wtu
        .clone()
        .filter(|camera| !camera.trim().is_empty())
    {
        Some(NikonWtuConfig {
            camera,
            port: args.nikon_wtu_port,
            output_dir: args.input.clone(),
            computer_name: args.nikon_wtu_name.clone(),
            guid: args.nikon_wtu_guid.clone(),
        })
    } else {
        None
    };

    let profiles = resolve_daemon_profiles(&args, temp_dir.path())?;
    let profiles = profiles.into_iter().map(Arc::new).collect::<Vec<_>>();
    let profiles = Arc::new(profiles);
    let base_seed = args.grain_seed.unwrap_or_else(time_of_day_seed);
    let (trusted_input_tx, trusted_input_rx) = mpsc::channel();
    let review = if let Some(address) = args
        .review_address
        .clone()
        .filter(|address| !address.trim().is_empty())
    {
        let review_profiles = profiles
            .iter()
            .enumerate()
            .map(|(index, profile)| {
                let metadata = ReviewProfileMetadata::from(&profile.resolved.metadata);
                ReviewProfile {
                    index,
                    identity: review_profile_identity(&profile.selector, Some(&metadata)),
                    selector: profile.selector.clone(),
                    stem: profile.stem.clone(),
                    sampler_added: false,
                    enabled_by_default: true,
                    configured_from_cli: true,
                    retouch_base: profile.resolved.retouch_base,
                    metadata: Some(metadata),
                    hald_path: profile.resolved.hald_path.clone(),
                }
            })
            .collect();
        Some(start_review_server(ReviewConfig {
            address,
            input_root: args.input.clone(),
            output_root: args.output.clone(),
            hald_dir: args.hald_dir.clone(),
            profiles_root: args.profiles_root.clone(),
            hald_level: args.hald_level,
            rawtherapee: args.rawtherapee.clone(),
            dng_fallback: args.dng_fallback.clone(),
            output_format: args.output_format,
            profiles: review_profiles,
            gallery: args.gallery.map(|template| ReviewGalleryConfig {
                template,
                columns: args.gallery_columns,
                thumbnail_long_edge: args.gallery_thumbnail_long_edge,
            }),
            convert: args.convert.clone(),
            export: args.export.clone(),
            jobs,
            publish_album: args.publish_album.clone(),
            no_grain: args.no_grain,
            normalize_grain_mpix: args.normalize_grain_mpix,
            lcp_root: args.lcp_root.clone(),
            color_noise_iso_threshold: args.color_noise_iso_threshold,
            lens_corrections: args.lens_corrections,
            grain: args.grain.clone(),
            grain_preset: args.grain_preset.clone(),
            grain_seed: Some(base_seed),
            grain_engine: args.grain_engine,
            diffusion: args.diffusion,
            codex: args.codex,
            codex_binary: args.codex_binary.clone(),
            codex_model: args.codex_model.clone(),
            codex_timeout: Duration::from_secs(args.codex_timeout),
            invocation: args.invocation.clone(),
            hugin_bin_dir: args.hugin_bin_dir.clone(),
            converted_input_sender: Some(trusted_input_tx.clone()),
            trusted_input_sender: (!matches!(args.input_file_filter, InputFileFilter::All))
                .then_some(trusted_input_tx),
        })?)
    } else {
        None
    };
    let cache_root = review.as_ref().map_or_else(
        || temp_dir.path().to_path_buf(),
        |review| review.cache_root().to_path_buf(),
    );
    let auto_import_catalog = if args.auto_import {
        Some(review.as_ref().map_or_else(
            || crate::app::review::AutoImportCatalog::open(&args.input, &args.output),
            |review| Ok(review.auto_import_catalog()),
        )?)
    } else {
        None
    };
    let args = Arc::new(args);

    let multi = MultiProgress::new();
    let batch = multi.add(ProgressBar::new(0));
    batch.set_style(batch_progress_style());
    batch.set_message("starting".to_string());

    batch.println(format!(
        "[{}] resolved profiles: {}",
        elapsed_human(start.elapsed()),
        profiles.len()
    ));
    for profile in &*profiles {
        let source = if let Some(hald_path) = profile.resolved.hald_path.as_ref() {
            hald_path.display().to_string()
        } else {
            "(no hald)".to_string()
        };
        batch.println(format!(
            "[{}]   - {} => {} [{}]",
            elapsed_human(start.elapsed()),
            profile.selector,
            profile.stem,
            source
        ));
        if !profile.resolved.rawtherapee_profiles.is_empty() {
            for pp3 in &profile.resolved.rawtherapee_profiles {
                batch.println(format!(
                    "[{}]       + pp3: {}",
                    elapsed_human(start.elapsed()),
                    pp3.display()
                ));
            }
        }
    }

    let (watch_tx, watch_rx) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(
        move |result| {
            if watch_tx.send(result).is_err() {
                // If the receiver disappeared, just exit the callback silently.
            }
        },
        Config::default(),
    )
    .context("starting filesystem watcher")?;
    watcher
        .watch(&args.input, RecursiveMode::Recursive)
        .with_context(|| format!("watching {}", args.input.display()))?;

    let nikon_wtu_receiver = if let Some(config) = nikon_wtu_config {
        batch.println(format!(
            "[{}] nikon-wtu: enabled, camera {}:{}, inbox {}",
            elapsed_human(start.elapsed()),
            config.camera,
            config.port,
            config.output_dir.display()
        ));
        Some(start_nikon_wtu_receiver(config)?)
    } else {
        None
    };
    let auto_import_receiver = if let Some(catalog) = auto_import_catalog {
        batch.println(format!(
            "[{}] auto-import: enabled for mounted Linux GVfs PTP/MTP cameras",
            elapsed_human(start.elapsed())
        ));
        Some(start_auto_import(AutoImportConfig {
            input_root: args.input.clone(),
            catalog,
            dng_fallback: args.dng_fallback.clone(),
            exiftool: PathBuf::from("exiftool"),
            progress: multi.clone(),
            progress_anchor: batch.clone(),
        })?)
    } else {
        None
    };

    batch.println(format!(
        "[{}] daemon started, watching {}",
        elapsed_human(start.elapsed()),
        args.input.display()
    ));
    if let Some(review) = &review
        && let Some(address) = &args.review_address
    {
        batch.println(format!(
            "[{}] review: http://{} (state {})",
            elapsed_human(start.elapsed()),
            address,
            review.state_path().display()
        ));
    }
    batch.println(format!(
        "[{}] output: {}, profiles: {}, inputs: {}, jobs: {}, debounce: {}",
        elapsed_human(start.elapsed()),
        args.output.display(),
        profiles.len(),
        input_filter_name(args.input_file_filter),
        jobs,
        if debounce.is_zero() {
            "immediate".to_string()
        } else {
            format!("{}s", args.debounce_seconds)
        }
    ));
    batch.println(format!(
        "[{}] press Ctrl+C to stop",
        elapsed_human(start.elapsed())
    ));

    batch.set_message("waiting for pictures".to_string());

    let worker_bars: Vec<_> = (0..jobs)
        .map(|index| {
            let file = multi.add(ProgressBar::new(progress_length()));
            file.set_style(file_progress_style());
            file.set_message(format!("worker {} waiting", index + 1));
            file
        })
        .collect();
    let worker_bars = Arc::new(Mutex::new(worker_bars));

    let mut pending: HashMap<PathBuf, PendingFile> = HashMap::new();
    let startup_inputs =
        collect_batch_inputs(&args.input, args.input_file_filter, &args.dng_fallback)?;
    let repaired_links =
        repair_compressed_output_links(&startup_inputs, &profiles, &args.input, &args.output)?;
    if repaired_links > 0 {
        batch.println(format!(
            "[{}] startup: repaired {} original/SOOC links",
            elapsed_human(start.elapsed()),
            repaired_links
        ));
    }
    if let Some(review) = &review {
        let metadata_count = review.prefetch_startup_exif_metadata(&startup_inputs);
        if metadata_count > 0 {
            batch.println(format!(
                "[{}] startup: read review metadata for {} files with {} workers",
                elapsed_human(start.elapsed()),
                metadata_count,
                cpu_thread_count()
            ));
        }
    }
    for input in &startup_inputs {
        queue_input_file(
            &mut pending,
            input.clone(),
            Duration::ZERO,
            args.input_file_filter,
        );
    }

    let mut queue = PendingTasks::default();
    let mut in_flight: Vec<InFlightTask> = Vec::new();
    let estimates = Arc::new(StageEstimates::default());
    let mut state = DaemonProgressState {
        total_processed: 0,
        total_succeeded: 0,
        total_failed: 0,
        total_elapsed_ms: 0,
        started_at: Instant::now(),
        files: Vec::new(),
        profile_stats: vec![DaemonProfileStats::default(); profiles.len()],
        profile_output_dirs: vec![HashSet::new(); profiles.len()],
        resource_usage: ResourceUsageSummary::default(),
        last_resource_sample: Instant::now(),
    };

    let queued_from_startup = schedule_pending_due_paths(
        &mut pending,
        Duration::ZERO,
        &mut queue,
        &profiles,
        &batch,
        &ProfileScheduleContext {
            input_root: &args.input,
            output_root: &args.output,
            output_format: args.output_format,
            input_file_filter: args.input_file_filter,
            skip_existing: true,
            review: review.as_ref(),
        },
    )?;
    if !startup_inputs.is_empty() {
        batch.println(format!(
            "[{}] startup: {} files discovered, {} queued",
            elapsed_human(start.elapsed()),
            startup_inputs.len(),
            queued_from_startup
        ));
    }

    state.sample_resources();
    state.last_resource_sample = Instant::now();
    write_daemon_info_txt(
        &args.output,
        &args,
        &profiles,
        &state,
        Duration::ZERO,
        Some(&state.resource_usage),
    )?;

    loop {
        if let Some(review) = &review {
            review.ensure_database_healthy()?;
        }
        drain_auto_import_logs(auto_import_receiver.as_ref(), &batch, start);
        drain_nikon_wtu_logs(nikon_wtu_receiver.as_ref(), &batch, start);
        drain_watch_events(&watch_rx, &mut pending, debounce, args.input_file_filter);
        while let Ok(input) = trusted_input_rx.try_recv() {
            let queued = enqueue_profile_jobs(
                &mut queue,
                &profiles,
                input,
                &ProfileScheduleContext {
                    input_root: &args.input,
                    output_root: &args.output,
                    output_format: args.output_format,
                    input_file_filter: InputFileFilter::All,
                    skip_existing: false,
                    review: review.as_ref(),
                },
            )?;
            batch.inc_length(queued as u64);
        }
        schedule_pending_due_paths(
            &mut pending,
            debounce,
            &mut queue,
            &profiles,
            &batch,
            &ProfileScheduleContext {
                input_root: &args.input,
                output_root: &args.output,
                output_format: args.output_format,
                input_file_filter: args.input_file_filter,
                skip_existing: false,
                review: review.as_ref(),
            },
        )?;

        let mut active_keys = in_flight
            .iter()
            .map(|task| task.key.clone())
            .collect::<HashSet<_>>();
        while in_flight.len() < jobs && queue.has_unblocked(&active_keys) {
            let task = match &review {
                Some(review) => {
                    let snapshot = review.render_priority_snapshot();
                    let dropped =
                        queue.drop_unschedulable(|task| review_pending_task_key(&snapshot, task));
                    batch.inc(dropped as u64);
                    queue.pop_ranked_excluding(&active_keys, |task| {
                        review_pending_task_key(&snapshot, task)
                    })
                }
                None => queue.pop_fifo_excluding(&active_keys),
            };
            let Some(mut task) = task else {
                break;
            };
            if let Some(image_id) = task.key.review_image_id()
                && let Some(review) = &review
            {
                let Some(current_raw) = review.review_raw_for_image_id(image_id) else {
                    batch.inc(1);
                    continue;
                };
                task.raw = current_raw;
            }
            if matches!(
                &task.kind,
                DaemonTaskKind::RawProfile(_) | DaemonTaskKind::SoocSidecar { .. }
            ) && compressed_profile_has_matching_raw(&task.raw)
            {
                batch.inc(1);
                continue;
            }
            let profile = match &task.kind {
                DaemonTaskKind::RawProfile(profile_index) => {
                    match profiles.get(*profile_index).cloned() {
                        Some(profile) => Some(profile),
                        None => {
                            batch.inc(1);
                            continue;
                        }
                    }
                }
                DaemonTaskKind::StandaloneCompressed | DaemonTaskKind::SoocSidecar { .. } => None,
            };
            let bar = acquire_worker_bar(&worker_bars);
            let task_key = task.key.clone();
            let raw = task.raw.clone();
            let worker_raw = raw.clone();
            let task_kind = task.kind.clone();
            let task_review_image_id = task.key.review_image_id();
            let raw_name = raw
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown")
                .to_string();
            let bar_pool = Arc::clone(&worker_bars);
            let thread_args = Arc::clone(&args);
            let thread_estimates = Arc::clone(&estimates);
            let thread_review = review.clone();
            let thread_cache_root = cache_root.clone();

            let handle = thread::spawn(move || {
                let context = DaemonTaskContext {
                    args: thread_args,
                    base_seed,
                    estimates: thread_estimates,
                    review: thread_review,
                    cache_root: thread_cache_root,
                };
                let mut result = match task_kind {
                    DaemonTaskKind::RawProfile(profile_index) => {
                        let profile = profile.expect("profile exists for RAW task");
                        if let Some(review) = &context.review {
                            let _ = if let Some(image_id) = task_review_image_id {
                                review.record_profile_processing_for_image(image_id, profile_index)
                            } else {
                                review.record_profile_processing(&worker_raw, profile_index)
                            };
                        }
                        process_single_profile(
                            &worker_raw,
                            &profile,
                            profile_index as u64,
                            &profile.stem,
                            &context,
                            &bar,
                            &raw_name,
                        )
                    }
                    DaemonTaskKind::StandaloneCompressed => {
                        if let Some(review) = &context.review {
                            let _ = review.record_compressed_processing(&worker_raw);
                        }
                        process_single_compressed(&worker_raw, &context, &bar, &raw_name)
                    }
                    DaemonTaskKind::SoocSidecar { sidecar } => {
                        if let Some(review) = &context.review {
                            let _ = if let Some(image_id) = task_review_image_id {
                                review.record_profile_processing_for_image(
                                    image_id,
                                    SOOC_PROFILE_INDEX,
                                )
                            } else {
                                review.record_profile_processing(&worker_raw, SOOC_PROFILE_INDEX)
                            };
                        }
                        process_single_sooc(&worker_raw, &sidecar, &context, &bar, &raw_name)
                    }
                };
                if let Some(review) = &context.review
                    && result.raw != worker_raw
                    && let Err(error) = review.rebind_raw_source(&worker_raw, &result.raw)
                {
                    result.error = Some(format!(
                        "render succeeded but review database could not replace {} with {}: {error:#}",
                        worker_raw.display(),
                        result.raw.display()
                    ));
                    result.raw.clone_from(&worker_raw);
                }
                release_worker_bar(&bar_pool, bar);
                result
            });
            in_flight.push(InFlightTask {
                key: task_key.clone(),
                kind: task.kind,
                raw,
                handle,
            });
            active_keys.insert(task_key);
        }

        let mut index = 0;
        while index < in_flight.len() {
            if !in_flight[index].handle.is_finished() {
                index += 1;
                continue;
            }

            let task = in_flight.swap_remove(index);
            batch.inc(1);
            let deferred_rerun = queue.contains_key(&task.key);
            let result = match task.handle.join() {
                Ok(result) => result,
                Err(_) => DaemonFileResult {
                    raw: task.raw.clone(),
                    output: PathBuf::new(),
                    duration: Duration::ZERO,
                    profile_index: match &task.kind {
                        DaemonTaskKind::RawProfile(profile_index) => Some(*profile_index),
                        DaemonTaskKind::SoocSidecar { .. } => Some(SOOC_PROFILE_INDEX),
                        DaemonTaskKind::StandaloneCompressed => None,
                    },
                    dcp_profile_filename: None,
                    error: Some("worker thread panicked".to_string()),
                    lens_profile_status: lens_profile_status(LensCorrections::default(), false),
                    sharpening_status: sharpening_status(false),
                    denoise_status: denoise_status(false),
                },
            };
            if !deferred_rerun && let Some(review) = &review {
                record_daemon_review_result(review, task.key.review_image_id(), &result);
            }
            if let Some(error) = &result.error {
                batch.println(format!("failed {}: {}", result.raw.display(), error));
            }
            state.record(&result);
            state.sample_resources();
            state.last_resource_sample = Instant::now();
            write_daemon_info_txt(
                &args.output,
                &args,
                &profiles,
                &state,
                start.elapsed(),
                Some(&state.resource_usage),
            )?;
        }

        state.resource_usage_if_needed(Instant::now());
        drain_auto_import_logs(auto_import_receiver.as_ref(), &batch, start);
        drain_nikon_wtu_logs(nikon_wtu_receiver.as_ref(), &batch, start);
        if queue.is_empty() && in_flight.is_empty() {
            batch.reset_eta();
            batch.set_length(0);
            batch.set_position(0);
            batch.set_message("waiting for pictures".to_string());
        } else {
            batch.set_message(format!(
                "queued {} running {} done {}",
                queue.len(),
                in_flight.len(),
                state.total_processed
            ));
        }

        std::thread::sleep(DEFAULT_POLL_INTERVAL);
    }
}

fn record_daemon_review_result(
    review: &ReviewHandle,
    review_image_id: Option<u64>,
    result: &DaemonFileResult,
) {
    let current_raw = review_image_id
        .and_then(|image_id| review.review_raw_for_image_id(image_id))
        .unwrap_or_else(|| result.raw.clone());
    match result.profile_index {
        Some(profile_index) => {
            if let Some(error) = &result.error {
                let output =
                    (!result.output.as_os_str().is_empty()).then_some(result.output.as_path());
                let _ = if let Some(image_id) = review_image_id {
                    review.record_profile_failed_for_image(
                        image_id,
                        profile_index,
                        output,
                        result.duration,
                        error,
                    )
                } else {
                    review.record_profile_failed(
                        &current_raw,
                        profile_index,
                        output,
                        result.duration,
                        error,
                    )
                };
            } else {
                let _ = if let Some(image_id) = review_image_id {
                    review.record_profile_done_with_dcp_for_image(
                        image_id,
                        profile_index,
                        &result.output,
                        result.duration,
                        result.dcp_profile_filename.as_deref(),
                    )
                } else {
                    review.record_profile_done_with_dcp(
                        &current_raw,
                        profile_index,
                        &result.output,
                        result.duration,
                        result.dcp_profile_filename.as_deref(),
                    )
                };
            }
        }
        None => {
            if let Some(error) = &result.error {
                let output =
                    (!result.output.as_os_str().is_empty()).then_some(result.output.as_path());
                let _ =
                    review.record_compressed_failed(&current_raw, output, result.duration, error);
            } else {
                let _ =
                    review.record_compressed_done(&current_raw, &result.output, result.duration);
            }
        }
    }
}

fn drain_nikon_wtu_logs(receiver: Option<&NikonWtuReceiver>, batch: &ProgressBar, start: Instant) {
    let Some(receiver) = receiver else {
        return;
    };
    for log in receiver.drain_logs() {
        batch.println(format!("[{}] {log}", elapsed_human(start.elapsed())));
    }
}

fn drain_auto_import_logs(
    receiver: Option<&AutoImportReceiver>,
    batch: &ProgressBar,
    start: Instant,
) {
    let Some(receiver) = receiver else {
        return;
    };
    for log in receiver.drain_logs() {
        batch.println(format!("[{}] {log}", elapsed_human(start.elapsed())));
    }
}

fn resolve_batch_daemon_jobs(jobs: Option<usize>) -> Result<usize> {
    let jobs = jobs.unwrap_or_else(half_cpu_thread_count);
    if jobs == 0 {
        bail!("--jobs must be at least 1");
    }
    Ok(jobs)
}

fn write_daemon_info_txt(
    output_root: &Path,
    args: &BatchDaemonArgs,
    profiles: &[Arc<DaemonProfile>],
    state: &DaemonProgressState,
    elapsed: Duration,
    resource_usage: Option<&ResourceUsageSummary>,
) -> Result<()> {
    use std::fmt::Write;

    let mut out = String::new();
    let started = chrono::Local::now();
    let started_str = started.format("%Y-%m-%d %H:%M:%S").to_string();
    let time_of_day = started.format("%H:%M:%S").to_string();
    let runtime = format_duration(elapsed);
    let uptime = format_duration(state.started_at.elapsed());

    writeln!(out, "mini-film daemon report").ok();
    writeln!(out, "Generated: {started_str}").ok();
    writeln!(out, "Time of day: {time_of_day}").ok();
    writeln!(out, "Timezone: {}", started.format("%:z")).ok();
    writeln!(out, "Mini-film version: {}", env!("CARGO_PKG_VERSION")).ok();
    writeln!(out, "Input directory: {}", args.input.display()).ok();
    writeln!(out, "Output directory: {}", args.output.display()).ok();
    writeln!(out, "Profiles: {}", profiles.len()).ok();
    writeln!(out, "Output format: {:?}", args.output_format).ok();
    writeln!(
        out,
        "Auto-import mounted PTP/MTP cameras: {}",
        if args.auto_import {
            "enabled"
        } else {
            "disabled"
        }
    )
    .ok();
    if let Some(address) = &args.review_address {
        writeln!(out, "Review server: http://{address}").ok();
        writeln!(
            out,
            "Review state: {}",
            args.output.join("mini-film-review.sqlite").display()
        )
        .ok();
        writeln!(
            out,
            "Review publish root: {}",
            args.output.join(&args.publish_album).display()
        )
        .ok();
        if let Some(gallery) = args.gallery {
            writeln!(out, "Review publish gallery: {gallery}").ok();
        }
    }
    writeln!(
        out,
        "Jobs: {}",
        args.jobs.unwrap_or_else(half_cpu_thread_count)
    )
    .ok();
    writeln!(out, "Elapsed: {runtime} (up since report start: {uptime})").ok();
    writeln!(
        out,
        "Files: processed={}, succeeded={}, failed={}",
        state.total_processed, state.total_succeeded, state.total_failed
    )
    .ok();
    append_resource_usage(&mut out, resource_usage);

    writeln!(out, "\nProfiles:").ok();
    for (index, profile) in profiles.iter().enumerate() {
        let stats = &state.profile_stats[index];
        writeln!(
            out,
            "  - [{}] {} => {} ({}/{} success/fail, avg {}/file ms)",
            index + 1,
            profile.selector,
            profile.stem,
            stats.succeeded,
            stats.failed,
            stats.avg_ms()
        )
        .ok();
        out.push_str("    Profile report:\n");
        for line in profile.profile_report.lines() {
            writeln!(out, "      {line}").ok();
        }
    }

    writeln!(out, "\nLatest files:").ok();
    for file in &state.files {
        if let Some(error) = &file.error {
            writeln!(
                out,
                "  FAILURE | {} | {} | {} | {} | {} | {}",
                file.raw.display(),
                file.output.display(),
                error,
                file.lens_profile_status,
                file.sharpening_status,
                file.denoise_status
            )
            .ok();
        } else {
            writeln!(
                out,
                "  OK | {} | {} | {} | {} | {} | {}",
                file.raw.display(),
                file.output.display(),
                format_duration(file.duration),
                file.lens_profile_status,
                file.sharpening_status,
                file.denoise_status
            )
            .ok();
        }
    }

    if let Some(parent) = output_root.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating output root parent {}", parent.display()))?;
    }
    if !output_root.exists() {
        fs::create_dir_all(output_root)
            .with_context(|| format!("creating output root {}", output_root.display()))?;
    }

    fs::write(output_root.join("info.txt"), out)?;

    for (index, profile) in profiles.iter().enumerate() {
        let report_for_profile =
            profile_daemon_info(args, profile, state, elapsed, resource_usage, index)?;
        for profile_dir in state
            .profile_output_dirs
            .get(index)
            .into_iter()
            .flat_map(|dirs| dirs.iter())
        {
            if let Err(error) = fs::create_dir_all(profile_dir) {
                return Err(anyhow::anyhow!(
                    "creating profile info directory {}: {error:#}",
                    profile_dir.display()
                ));
            }
            let report_path = profile_dir.join("info.txt");
            fs::write(&report_path, &report_for_profile).with_context(|| {
                format!("writing daemon profile report {}", report_path.display())
            })?;
        }
    }

    Ok(())
}

fn append_resource_usage(out: &mut String, resource_usage: Option<&ResourceUsageSummary>) {
    if let Some(usage) = resource_usage {
        out.push_str(&usage.report_block());
        out.push('\n');
        return;
    }
    out.push_str("Resource usage: unavailable\n");
}

fn profile_daemon_info(
    args: &BatchDaemonArgs,
    profile: &DaemonProfile,
    state: &DaemonProgressState,
    elapsed: Duration,
    resource_usage: Option<&ResourceUsageSummary>,
    profile_index: usize,
) -> Result<String> {
    use std::fmt::Write;

    let mut out = String::new();
    let started = chrono::Local::now();
    let started_str = started.format("%Y-%m-%d %H:%M:%S").to_string();
    let time_of_day = started.format("%H:%M:%S").to_string();
    let runtime = format_duration(elapsed);
    let uptime = format_duration(state.started_at.elapsed());
    let stats = state
        .profile_stats
        .get(profile_index)
        .cloned()
        .unwrap_or_default();

    writeln!(out, "mini-film daemon profile report").ok();
    writeln!(out, "Generated: {started_str}").ok();
    writeln!(out, "Time of day: {time_of_day}").ok();
    writeln!(out, "Timezone: {}", started.format("%:z")).ok();
    writeln!(out, "Mini-film version: {}", env!("CARGO_PKG_VERSION")).ok();
    writeln!(out, "Input directory: {}", args.input.display()).ok();
    writeln!(out, "Output directory: {}", args.output.display()).ok();
    writeln!(out, "Profile: {}", profile.selector).ok();
    writeln!(
        out,
        "Resolved profile: {}",
        daemon_profile_label(&profile.stem)
    )
    .ok();
    writeln!(out, "Output format: {:?}", args.output_format).ok();
    if let Some(address) = &args.review_address {
        writeln!(out, "Review server: http://{address}").ok();
        writeln!(
            out,
            "Review publish root: {}",
            args.output.join(&args.publish_album).display()
        )
        .ok();
        if let Some(gallery) = args.gallery {
            writeln!(out, "Review publish gallery: {gallery}").ok();
        }
    }
    writeln!(out, "Elapsed: {runtime} (up since report start: {uptime})").ok();
    writeln!(
        out,
        "Profile files: processed={}, succeeded={}, failed={}",
        stats.processed, stats.succeeded, stats.failed
    )
    .ok();
    writeln!(out, "Profile avg: {} ms/file", stats.avg_ms()).ok();

    append_resource_usage(&mut out, resource_usage);

    writeln!(out, "\nProfile info details:").ok();
    for line in profile.profile_report.lines() {
        writeln!(out, "{line}").ok();
    }
    writeln!(out).ok();

    writeln!(out, "Files for this profile:").ok();
    for file in state
        .files
        .iter()
        .filter(|file| file.profile_index == Some(profile_index))
    {
        if let Some(error) = &file.error {
            writeln!(
                out,
                "  FAILURE | {} | {} | {} | {} | {} | {}",
                file.raw.display(),
                file.output.display(),
                error,
                file.lens_profile_status,
                file.sharpening_status,
                file.denoise_status
            )
            .ok();
        } else {
            writeln!(
                out,
                "  OK | {} | {} | {} | {} | {} | {}",
                file.raw.display(),
                file.output.display(),
                format_duration(file.duration),
                file.lens_profile_status,
                file.sharpening_status,
                file.denoise_status
            )
            .ok();
        }
    }

    Ok(out)
}

fn resolve_daemon_profiles(args: &BatchDaemonArgs, temp_dir: &Path) -> Result<Vec<DaemonProfile>> {
    let selectors = args
        .profile
        .iter()
        .filter_map(|selector| {
            let selector = selector.trim();
            (!selector.is_empty()).then(|| selector.to_string())
        })
        .collect::<Vec<_>>();

    if selectors.is_empty() && matches!(args.input_file_filter, InputFileFilter::JpgOnly) {
        return Ok(Vec::new());
    }

    selectors
        .iter()
        .enumerate()
        .map(|(index, selector)| {
            let profile_tmp_dir = temp_dir.join(
                sanitize_filename::sanitize(format!("{:03}-{}", index + 1, selector)).into_owned(),
            );
            fs::create_dir_all(&profile_tmp_dir).with_context(|| {
                format!("creating profile temp dir {}", profile_tmp_dir.display())
            })?;

            let apply_args = ApplyArgs {
                raw: PathBuf::new(),
                output: PathBuf::new(),
                profile: Some(selector.clone()),
                hald_dir: args.hald_dir.clone(),
                profiles_root: args.profiles_root.clone(),
                hald_level: args.hald_level,
                rawtherapee: args.rawtherapee.clone(),
                dng_fallback: args.dng_fallback.clone(),
                convert: args.convert.clone(),
                keep_intermediate: None,
                no_grain: args.no_grain,
                normalize_grain_mpix: args.normalize_grain_mpix,
                lcp_root: args.lcp_root.clone(),
                lens_corrections: args.lens_corrections,
                color_noise_iso_threshold: args.color_noise_iso_threshold,
                grain: args.grain.clone(),
                grain_preset: args.grain_preset.clone(),
                grain_seed: args.grain_seed,
                grain_engine: args.grain_engine,
                diffusion: args.diffusion,
                export: args.export.clone(),
                retouch: None,
                retouch_white_balance: crate::app::retouch::RetouchWhiteBalance::default(),
                bw_filter: crate::app::retouch::BwFilter::None,
            };
            let mut resolved = resolve_profile(&apply_args, &profile_tmp_dir)
                .with_context(|| format!("resolving profile {selector}"))?;
            if let Some(grain) =
                resolve_grain_override(args.grain.as_deref(), args.grain_preset.as_deref())?
            {
                resolved.grain = grain;
            }
            let profile_report = profile_info_text_for_selector(
                selector,
                &args.profiles_root,
                &args.hald_dir,
                args.hald_level,
            )?;
            let stem = resolved.resolved_stem.clone();
            Ok(DaemonProfile {
                selector: selector.clone(),
                stem,
                resolved,
                profile_report,
            })
        })
        .collect::<Result<Vec<_>>>()
        .map(|profiles| {
            if profiles.is_empty() {
                vec![DaemonProfile {
                    selector: String::new(),
                    stem: String::new(),
                    resolved: neutral_profile(),
                    profile_report: "No profile configured; matching Adobe Standard DCPs are used when available, otherwise RawTherapee defaults are used.".to_string(),
                }]
            } else {
                profiles
            }
        })
}

fn event_stability_delay(kind: &EventKind, debounce: Duration) -> Duration {
    if is_close_or_rename_event(kind) {
        Duration::ZERO
    } else if debounce.is_zero() {
        Duration::from_millis(100)
    } else {
        debounce
    }
}

fn is_close_or_rename_event(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Access(AccessKind::Close(_)) | EventKind::Modify(ModifyKind::Name(_))
    )
}

fn is_relevant_daemon_event(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Access(AccessKind::Close(_))
            | EventKind::Modify(ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Metadata(_))
            | EventKind::Modify(ModifyKind::Name(_))
            | EventKind::Create(_)
            | EventKind::Any,
    )
}

fn drain_watch_events(
    watch_rx: &Receiver<Result<Event, notify::Error>>,
    pending: &mut HashMap<PathBuf, PendingFile>,
    debounce: Duration,
    filter: InputFileFilter,
) {
    loop {
        match watch_rx.try_recv() {
            Ok(Ok(event)) => {
                if is_relevant_daemon_event(&event.kind) {
                    let delay = event_stability_delay(&event.kind, debounce);
                    for path in event.paths {
                        queue_input_file(pending, path, delay, filter);
                    }
                }
            }
            Ok(Err(_)) => {}
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => break,
        }
    }
}

fn schedule_pending_due_paths(
    pending: &mut HashMap<PathBuf, PendingFile>,
    debounce: Duration,
    queue: &mut PendingTasks,
    profiles: &[Arc<DaemonProfile>],
    batch: &ProgressBar,
    context: &ProfileScheduleContext<'_>,
) -> Result<usize> {
    let due = coalesce_due_input_sidecars(
        collect_due_paths(pending, debounce),
        context.input_file_filter,
    );
    if due.is_empty() {
        return Ok(0);
    }

    let mut queued_count = 0u64;
    for raw in due {
        queued_count += enqueue_profile_jobs(queue, profiles, raw, context)? as u64;
    }
    if queued_count == 0 {
        return Ok(0);
    }
    batch.inc_length(queued_count);
    Ok(queued_count as usize)
}

fn enqueue_profile_jobs(
    queue: &mut PendingTasks,
    profiles: &[Arc<DaemonProfile>],
    raw: PathBuf,
    context: &ProfileScheduleContext<'_>,
) -> Result<usize> {
    let mut queued = 0usize;
    let raw_input = is_raw_input_file(&raw);
    if !raw_input && !daemon_profiles_are_explicit(profiles) {
        return enqueue_compressed_job(queue, raw, context);
    }
    if compressed_profile_has_matching_raw(&raw) {
        return Ok(0);
    }
    let sooc_sidecar = (raw_input && matches!(context.input_file_filter, InputFileFilter::All))
        .then(|| matching_sidecar_for_raw(&raw))
        .flatten();
    if let Some(review) = context.review {
        if raw_input {
            review.record_discovered_raw_with_sidecar(&raw, sooc_sidecar.as_deref())?;
        } else {
            review.record_profiled_compressed_discovered(&raw)?;
        }
    }
    let review_image_id = context
        .review
        .and_then(|review| review.review_image_id_for(&raw));
    for (profile_index, profile) in profiles.iter().enumerate() {
        let expected_output = daemon_output_path(
            context.input_root,
            context.output_root,
            context.output_format,
            &raw,
            &profile.stem,
        )?;
        if should_skip_existing_profile_output(context, &raw, profile_index, &expected_output) {
            continue;
        }

        if let Some(review) = context.review
            && !review.record_profile_queued(&raw, profile_index, &expected_output)?
        {
            continue;
        }
        queued += usize::from(queue.push(
            raw.clone(),
            DaemonTaskKind::RawProfile(profile_index),
            review_image_id,
        ));
    }
    let sooc_source = sooc_sidecar.or_else(|| (!raw_input).then(|| raw.clone()));
    if let Some(sidecar) = sooc_source {
        queued += enqueue_sooc_job(queue, raw, sidecar, review_image_id, context)?;
    }
    Ok(queued)
}

fn daemon_profiles_are_explicit(profiles: &[Arc<DaemonProfile>]) -> bool {
    profiles
        .iter()
        .any(|profile| !profile.selector.trim().is_empty())
}

fn enqueue_compressed_job(
    queue: &mut PendingTasks,
    input: PathBuf,
    context: &ProfileScheduleContext<'_>,
) -> Result<usize> {
    let expected_output =
        daemon_passthrough_output_path(context.input_root, context.output_root, &input)?;
    if let Some(review) = context.review {
        review.record_compressed_queued(&input, &expected_output)?;
    }
    if context.skip_existing && expected_output.exists() {
        if let Some(review) = context.review {
            review.record_compressed_done(&input, &expected_output, Duration::ZERO)?;
        }
        return Ok(0);
    }

    let review_image_id = context
        .review
        .and_then(|review| review.review_image_id_for(&input));
    let inserted = queue.push(input, DaemonTaskKind::StandaloneCompressed, review_image_id);
    Ok(usize::from(inserted))
}

fn enqueue_sooc_job(
    queue: &mut PendingTasks,
    raw: PathBuf,
    sidecar: PathBuf,
    review_image_id: Option<u64>,
    context: &ProfileScheduleContext<'_>,
) -> Result<usize> {
    let expected_output =
        daemon_sooc_output_path(context.input_root, context.output_root, &raw, &sidecar)?;
    if should_skip_existing_profile_output(context, &raw, SOOC_PROFILE_INDEX, &expected_output) {
        return Ok(0);
    }
    if let Some(review) = context.review
        && !review.record_profile_queued(&raw, SOOC_PROFILE_INDEX, &expected_output)?
    {
        return Ok(0);
    }

    let inserted = queue.push(
        raw,
        DaemonTaskKind::SoocSidecar { sidecar },
        review_image_id,
    );
    Ok(usize::from(inserted))
}

fn should_skip_existing_profile_output(
    context: &ProfileScheduleContext<'_>,
    raw: &Path,
    profile_index: usize,
    expected_output: &Path,
) -> bool {
    if !context.skip_existing || !expected_output.exists() {
        return false;
    }
    context
        .review
        .is_none_or(|review| review.profile_render_current(raw, profile_index, expected_output))
}

fn collect_batch_inputs(
    input: &Path,
    filter: InputFileFilter,
    dng_fallback: &DngFallbackConfig,
) -> Result<Vec<PathBuf>> {
    let mut inputs = Vec::new();
    for entry in WalkDir::new(input).into_iter().filter_map(Result::ok) {
        if entry.path().is_file() && is_supported_input_file(entry.path(), filter) {
            inputs.push(entry.path().to_path_buf());
        }
    }
    dng_fallback.coalesce_existing_replacements(coalesce_input_sidecars(inputs, filter))
}

fn repair_compressed_output_links(
    inputs: &[PathBuf],
    profiles: &[Arc<DaemonProfile>],
    input_root: &Path,
    output_root: &Path,
) -> Result<usize> {
    let profiles_are_explicit = daemon_profiles_are_explicit(profiles);
    let mut repaired = 0;
    let mut destinations = HashSet::new();
    for input in inputs {
        let source_and_output = if is_raw_input_file(input) {
            if let Some(sidecar) = matching_sidecar_for_raw(input) {
                let output = daemon_sooc_output_path(input_root, output_root, input, &sidecar)?;
                Some((sidecar, output))
            } else {
                None
            }
        } else if is_rendered_input_file(input) {
            if let Some(raw) = matching_raw_for_sidecar(input) {
                let output = daemon_sooc_output_path(input_root, output_root, &raw, input)?;
                Some((input.clone(), output))
            } else if profiles_are_explicit {
                let output = daemon_sooc_output_path(input_root, output_root, input, input)?;
                Some((input.clone(), output))
            } else {
                let output = daemon_passthrough_output_path(input_root, output_root, input)?;
                Some((input.clone(), output))
            }
        } else {
            None
        };
        let Some((source, output)) = source_and_output else {
            continue;
        };
        if destinations.insert(output.clone()) && ensure_file_symlink(&source, &output, true)? {
            repaired += 1;
        }
    }
    Ok(repaired)
}

struct DaemonTaskContext {
    args: Arc<BatchDaemonArgs>,
    base_seed: u64,
    estimates: Arc<StageEstimates>,
    review: Option<ReviewHandle>,
    cache_root: PathBuf,
}

fn process_single_profile(
    raw: &Path,
    profile: &DaemonProfile,
    profile_index: u64,
    profile_stem: &str,
    context: &DaemonTaskContext,
    file: &ProgressBar,
    raw_name: &str,
) -> DaemonFileResult {
    let args = &context.args;
    let output = match daemon_output_path(
        &args.input,
        &args.output,
        args.output_format,
        raw,
        &profile.stem,
    ) {
        Ok(output) => output,
        Err(error) => {
            return DaemonFileResult {
                raw: raw.to_path_buf(),
                output: PathBuf::new(),
                duration: Duration::ZERO,
                profile_index: Some(profile_index as usize),
                dcp_profile_filename: None,
                error: Some(error.to_string()),
                lens_profile_status: daemon_input_lens_status(raw, args.lens_corrections, false),
                sharpening_status: sharpening_status(false),
                denoise_status: denoise_status(false),
            };
        }
    };
    if compressed_profile_has_matching_raw(raw) {
        return DaemonFileResult {
            raw: raw.to_path_buf(),
            output,
            duration: Duration::ZERO,
            profile_index: Some(profile_index as usize),
            dcp_profile_filename: None,
            error: None,
            lens_profile_status: daemon_input_lens_status(raw, args.lens_corrections, false),
            sharpening_status: sharpening_status(false),
            denoise_status: denoise_status(false),
        };
    }

    if let Some(parent) = output.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return DaemonFileResult {
            raw: raw.to_path_buf(),
            output,
            duration: Duration::ZERO,
            profile_index: Some(profile_index as usize),
            dcp_profile_filename: None,
            error: Some(error.to_string()),
            lens_profile_status: daemon_input_lens_status(raw, args.lens_corrections, false),
            sharpening_status: sharpening_status(false),
            denoise_status: denoise_status(false),
        };
    }

    let temp_dir = match Builder::new().prefix("mini-film-daemon-job-").tempdir() {
        Ok(temp_dir) => temp_dir,
        Err(error) => {
            return DaemonFileResult {
                raw: raw.to_path_buf(),
                output,
                duration: Duration::ZERO,
                profile_index: Some(profile_index as usize),
                dcp_profile_filename: None,
                error: Some(error.to_string()),
                lens_profile_status: daemon_input_lens_status(raw, args.lens_corrections, false),
                sharpening_status: sharpening_status(false),
                denoise_status: denoise_status(false),
            };
        }
    };
    let staged_output = if is_raw_input_file(raw) {
        None
    } else {
        match daemon_profile_output_temp(&context.cache_root, &output) {
            Ok(staged_output) => Some(staged_output),
            Err(error) => {
                return DaemonFileResult {
                    raw: raw.to_path_buf(),
                    output,
                    duration: Duration::ZERO,
                    profile_index: Some(profile_index as usize),
                    dcp_profile_filename: None,
                    error: Some(error.to_string()),
                    lens_profile_status: daemon_input_lens_status(
                        raw,
                        args.lens_corrections,
                        false,
                    ),
                    sharpening_status: sharpening_status(false),
                    denoise_status: denoise_status(false),
                };
            }
        }
    };
    let apply_output = staged_output.as_deref().unwrap_or(&output);

    file.set_position(0);
    let profile_label = daemon_profile_label(&profile.stem);
    file.set_message(format!("{} -> {}: queued", raw_name, profile_label));

    let file_start = Instant::now();
    let progress = ApplyProgress {
        file,
        started: file_start,
        estimates: Some(Arc::clone(&context.estimates)),
    };
    let seed = stable_profile_seed(context.base_seed, raw, profile_index);
    let apply_outcome = match apply_resolved(
        ApplyJob {
            raw,
            output: apply_output,
            rawtherapee: &args.rawtherapee,
            dng_fallback: &args.dng_fallback,
            prepared_raw: None,
            convert: &args.convert,
            keep_intermediate: None,
            no_grain: args.no_grain,
            normalize_grain_mpix: args.normalize_grain_mpix,
            grain_engine: args.grain_engine,
            diffusion: args.diffusion,
            lcp_root: args.lcp_root.as_deref(),
            lens_corrections: args.lens_corrections,
            color_noise_iso_threshold: args.color_noise_iso_threshold,
            export: &args.export,
            quiet: true,
            exif_comment: Some(format!(
                "mini-film {} usage=daemon profile={}",
                env!("CARGO_PKG_VERSION"),
                if profile_stem.trim().is_empty() {
                    "none"
                } else {
                    profile_stem
                }
            )),
            retouch: None,
            retouch_white_balance: crate::app::retouch::RetouchWhiteBalance::default(),
            bw_filter: crate::app::retouch::BwFilter::None,
            profile_input_cache_root: Some(&context.cache_root),
        },
        &profile.resolved,
        seed,
        temp_dir.path(),
        Some(&progress),
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            return DaemonFileResult {
                raw: raw.to_path_buf(),
                output,
                duration: file_start.elapsed(),
                profile_index: Some(profile_index as usize),
                dcp_profile_filename: None,
                error: Some(error.to_string()),
                lens_profile_status: daemon_input_lens_status(raw, args.lens_corrections, false),
                sharpening_status: sharpening_status(false),
                denoise_status: denoise_status(false),
            };
        }
    };
    let dcp_profile_filename = apply_outcome.dcp_profile_filename;
    let canonical_raw = apply_outcome
        .replacement
        .map_or(apply_outcome.source_path, |replacement| {
            replacement.new_path
        });
    if let Some(staged_output) = staged_output
        && let Err(error) = publish_daemon_profile_output(&canonical_raw, staged_output, &output)
    {
        return DaemonFileResult {
            raw: canonical_raw,
            output,
            duration: file_start.elapsed(),
            profile_index: Some(profile_index as usize),
            dcp_profile_filename: None,
            error: Some(error.to_string()),
            lens_profile_status: daemon_input_lens_status(raw, args.lens_corrections, false),
            sharpening_status: sharpening_status(false),
            denoise_status: denoise_status(false),
        };
    }

    let (sharpening_applied, denoise_applied) = resolve_apply_effects(
        &canonical_raw,
        &profile.resolved,
        args.color_noise_iso_threshold,
    );

    file.set_message(format!(
        "{} -> {}: done in {}",
        raw_name,
        profile_label,
        format_duration(file_start.elapsed())
    ));

    DaemonFileResult {
        raw: canonical_raw,
        output,
        duration: file_start.elapsed(),
        profile_index: Some(profile_index as usize),
        dcp_profile_filename,
        error: None,
        lens_profile_status: daemon_input_lens_status(raw, args.lens_corrections, true),
        sharpening_status: sharpening_status(sharpening_applied),
        denoise_status: denoise_status(denoise_applied),
    }
}

fn daemon_input_lens_status(input: &Path, corrections: LensCorrections, applied: bool) -> String {
    if is_raw_input_file(input) {
        lens_profile_status(corrections, applied)
    } else {
        "lens-profile: skipped (compressed input)".to_string()
    }
}

fn daemon_profile_output_temp(cache_root: &Path, output: &Path) -> Result<TempPath> {
    let parent = cache_root.join(DAEMON_PROFILE_OUTPUTS_CACHE_DIR);
    fs::create_dir_all(&parent)
        .with_context(|| format!("creating daemon profile cache {}", parent.display()))?;
    let extension = output
        .extension()
        .and_then(|extension| extension.to_str())
        .ok_or_else(|| {
            anyhow!(
                "daemon profile output has no extension: {}",
                output.display()
            )
        })?;
    let suffix = format!(".{extension}");
    let staged = Builder::new()
        .prefix(".mini-film-profile-output-")
        .suffix(&suffix)
        .tempfile_in(&parent)
        .with_context(|| format!("creating staged profile output in {}", parent.display()))?
        .into_temp_path();
    fs::remove_file(&staged)
        .with_context(|| format!("preparing staged profile output {}", staged.display()))?;
    Ok(staged)
}

fn publish_daemon_profile_output(input: &Path, staged: TempPath, output: &Path) -> Result<()> {
    if compressed_profile_has_matching_raw(input) {
        return Ok(());
    }
    match fs::remove_file(output) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("replacing daemon profile output {}", output.display()));
        }
    }
    fs::copy(&staged, output)
        .with_context(|| format!("publishing daemon profile output {}", output.display()))?;
    fs::File::open(output)
        .with_context(|| format!("opening daemon profile output {}", output.display()))?
        .sync_all()
        .with_context(|| format!("syncing daemon profile output {}", output.display()))
}

fn compressed_profile_has_matching_raw(input: &Path) -> bool {
    is_jpeg_input_file(input) && matching_raw_for_sidecar(input).is_some()
}

fn process_single_compressed(
    input: &Path,
    context: &DaemonTaskContext,
    file: &ProgressBar,
    raw_name: &str,
) -> DaemonFileResult {
    let args = &context.args;
    let output = match daemon_passthrough_output_path(&args.input, &args.output, input) {
        Ok(output) => output,
        Err(error) => {
            return DaemonFileResult {
                raw: input.to_path_buf(),
                output: PathBuf::new(),
                duration: Duration::ZERO,
                profile_index: None,
                dcp_profile_filename: None,
                error: Some(error.to_string()),
                lens_profile_status: "lens-profile: skipped (compressed input)".to_string(),
                sharpening_status: sharpening_status(false),
                denoise_status: denoise_status(false),
            };
        }
    };

    if let Some(parent) = output.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return DaemonFileResult {
            raw: input.to_path_buf(),
            output,
            duration: Duration::ZERO,
            profile_index: None,
            dcp_profile_filename: None,
            error: Some(error.to_string()),
            lens_profile_status: "lens-profile: skipped (compressed input)".to_string(),
            sharpening_status: sharpening_status(false),
            denoise_status: denoise_status(false),
        };
    }

    file.set_position(0);
    file.set_message(format!("{raw_name}: compressed queued"));

    let file_start = Instant::now();
    if let Err(error) = ensure_file_symlink(input, &output, true) {
        return DaemonFileResult {
            raw: input.to_path_buf(),
            output,
            duration: file_start.elapsed(),
            profile_index: None,
            dcp_profile_filename: None,
            error: Some(error.to_string()),
            lens_profile_status: "lens-profile: skipped (compressed input)".to_string(),
            sharpening_status: sharpening_status(false),
            denoise_status: denoise_status(false),
        };
    }

    file.set_message(format!(
        "{}: compressed done in {}",
        raw_name,
        format_duration(file_start.elapsed())
    ));

    DaemonFileResult {
        raw: input.to_path_buf(),
        output,
        duration: file_start.elapsed(),
        profile_index: None,
        dcp_profile_filename: None,
        error: None,
        lens_profile_status: "lens-profile: skipped (compressed input)".to_string(),
        sharpening_status: sharpening_status(false),
        denoise_status: denoise_status(false),
    }
}

fn process_single_sooc(
    raw: &Path,
    sidecar: &Path,
    context: &DaemonTaskContext,
    file: &ProgressBar,
    raw_name: &str,
) -> DaemonFileResult {
    let args = &context.args;
    let output = match daemon_sooc_output_path(&args.input, &args.output, raw, sidecar) {
        Ok(output) => output,
        Err(error) => {
            return DaemonFileResult {
                raw: raw.to_path_buf(),
                output: PathBuf::new(),
                duration: Duration::ZERO,
                profile_index: Some(SOOC_PROFILE_INDEX),
                dcp_profile_filename: None,
                error: Some(error.to_string()),
                lens_profile_status: "lens-profile: skipped (sooc sidecar)".to_string(),
                sharpening_status: sharpening_status(false),
                denoise_status: denoise_status(false),
            };
        }
    };

    if compressed_profile_has_matching_raw(raw) {
        return DaemonFileResult {
            raw: raw.to_path_buf(),
            output,
            duration: Duration::ZERO,
            profile_index: Some(SOOC_PROFILE_INDEX),
            dcp_profile_filename: None,
            error: None,
            lens_profile_status: "lens-profile: skipped (sooc sidecar)".to_string(),
            sharpening_status: sharpening_status(false),
            denoise_status: denoise_status(false),
        };
    }

    if let Some(parent) = output.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return DaemonFileResult {
            raw: raw.to_path_buf(),
            output,
            duration: Duration::ZERO,
            profile_index: Some(SOOC_PROFILE_INDEX),
            dcp_profile_filename: None,
            error: Some(error.to_string()),
            lens_profile_status: "lens-profile: skipped (sooc sidecar)".to_string(),
            sharpening_status: sharpening_status(false),
            denoise_status: denoise_status(false),
        };
    }

    file.set_position(0);
    file.set_message(format!("{raw_name} -> sooc: queued"));

    let file_start = Instant::now();
    if let Err(error) = ensure_file_symlink(sidecar, &output, true) {
        return DaemonFileResult {
            raw: raw.to_path_buf(),
            output,
            duration: file_start.elapsed(),
            profile_index: Some(SOOC_PROFILE_INDEX),
            dcp_profile_filename: None,
            error: Some(error.to_string()),
            lens_profile_status: "lens-profile: skipped (sooc sidecar)".to_string(),
            sharpening_status: sharpening_status(false),
            denoise_status: denoise_status(false),
        };
    }
    file.set_message(format!(
        "{} -> sooc: done in {}",
        raw_name,
        format_duration(file_start.elapsed())
    ));

    DaemonFileResult {
        raw: raw.to_path_buf(),
        output,
        duration: file_start.elapsed(),
        profile_index: Some(SOOC_PROFILE_INDEX),
        dcp_profile_filename: None,
        error: None,
        lens_profile_status: "lens-profile: skipped (sooc sidecar)".to_string(),
        sharpening_status: sharpening_status(false),
        denoise_status: denoise_status(false),
    }
}

fn sharpening_status(applied: bool) -> String {
    format!("sharpening: {}", if applied { "yes" } else { "no" })
}

fn denoise_status(applied: bool) -> String {
    format!("denoise: {}", if applied { "yes" } else { "no" })
}

fn acquire_worker_bar(bar_pool: &Arc<Mutex<Vec<ProgressBar>>>) -> ProgressBar {
    loop {
        if let Some(file) = bar_pool
            .lock()
            .expect("worker progress bar pool poisoned")
            .pop()
        {
            return file;
        }
        thread::yield_now();
    }
}

fn release_worker_bar(bar_pool: &Arc<Mutex<Vec<ProgressBar>>>, file: ProgressBar) {
    bar_pool
        .lock()
        .expect("worker progress bar pool poisoned")
        .push(file);
}

pub(crate) fn daemon_output_path(
    input_root: &Path,
    output_root: &Path,
    output_format: BatchOutputFormat,
    raw: &Path,
    profile_stem: &str,
) -> Result<PathBuf> {
    let profile_stem = sanitize_filename::sanitize(profile_stem);
    let profile_dir = daemon_output_dir(input_root, output_root, raw, &profile_stem)?;
    let raw_stem = relative_raw_stem(raw)?;
    Ok(profile_dir.join(format!(
        "{}.{}",
        sanitize_filename::sanitize(raw_stem),
        output_format.extension()
    )))
}

fn daemon_passthrough_output_path(
    input_root: &Path,
    output_root: &Path,
    input: &Path,
) -> Result<PathBuf> {
    daemon_link_output_path(input_root, output_root, input, input, "")
}

fn daemon_sooc_output_path(
    input_root: &Path,
    output_root: &Path,
    raw: &Path,
    source: &Path,
) -> Result<PathBuf> {
    daemon_link_output_path(input_root, output_root, raw, source, SOOC_PROFILE_STEM)
}

fn daemon_link_output_path(
    input_root: &Path,
    output_root: &Path,
    relative_source: &Path,
    linked_source: &Path,
    profile_stem: &str,
) -> Result<PathBuf> {
    let profile_stem = sanitize_filename::sanitize(profile_stem);
    let output_dir = daemon_output_dir(input_root, output_root, relative_source, &profile_stem)?;
    let raw_stem = relative_raw_stem(relative_source)?;
    let extension = linked_source
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.trim().is_empty())
        .ok_or_else(|| {
            anyhow!(
                "compressed input has no extension: {}",
                linked_source.display()
            )
        })?
        .to_ascii_lowercase();
    let extension = if matches!(extension.as_str(), "jpg" | "jpeg") {
        "jpg"
    } else {
        extension.as_str()
    };
    Ok(output_dir.join(format!(
        "{}.{extension}",
        sanitize_filename::sanitize(raw_stem)
    )))
}

fn daemon_output_dir(
    input_root: &Path,
    output_root: &Path,
    raw: &Path,
    profile_stem: &str,
) -> Result<PathBuf> {
    let relative = raw
        .strip_prefix(input_root)
        .with_context(|| format!("mapping {} under {}", raw.display(), input_root.display()))?;
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    if profile_stem.trim().is_empty() {
        Ok(output_root.join(parent))
    } else {
        Ok(output_root.join(parent).join(profile_stem))
    }
}

fn daemon_profile_label(profile_stem: &str) -> &str {
    if profile_stem.trim().is_empty() {
        "RawTherapee defaults"
    } else {
        profile_stem
    }
}

fn relative_raw_stem(raw: &Path) -> Result<&str> {
    raw.file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| anyhow!("raw path has no file stem: {}", raw.display()))
}

fn queue_input_file(
    pending: &mut HashMap<PathBuf, PendingFile>,
    path: PathBuf,
    debounce: Duration,
    filter: InputFileFilter,
) {
    if DngFallbackConfig::generated_this_process(&path)
        || !path.is_file()
        || !is_supported_input_file(&path, filter)
    {
        return;
    }
    if let Ok(metadata) = fs::metadata(&path) {
        pending.insert(
            path.clone(),
            PendingFile {
                path: path.clone(),
                process_at: Instant::now() + debounce,
                size: metadata.len(),
                modified: metadata.modified().ok(),
            },
        );
    }
}

fn collect_due_paths(
    pending: &mut HashMap<PathBuf, PendingFile>,
    debounce: Duration,
) -> Vec<PathBuf> {
    let now = Instant::now();
    let mut next = HashMap::new();
    let mut due = Vec::new();

    for (key, state) in pending.drain() {
        if state.process_at > now {
            next.insert(key, state);
            continue;
        }

        if let Ok(metadata) = fs::metadata(&state.path) {
            let size = metadata.len();
            let modified = metadata.modified().ok();
            if size == state.size && modified == state.modified {
                due.push(state.path);
            } else {
                next.insert(
                    state.path.clone(),
                    PendingFile {
                        path: state.path,
                        process_at: now + debounce,
                        size,
                        modified,
                    },
                );
            }
        }
    }

    *pending = next;
    due
}

fn stable_profile_seed(base_seed: u64, path: &Path, profile_index: u64) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    profile_index.hash(&mut hasher);
    path.hash(&mut hasher);
    base_seed.hash(&mut hasher);
    hasher.finish()
}

fn elapsed_human(elapsed: Duration) -> String {
    format!("{:>8.2}s", elapsed.as_secs_f32())
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{
        AccessKind, AccessMode, CreateKind, DataChange, EventKind, MetadataKind, ModifyKind,
        RenameMode,
    };
    use std::collections::{HashMap, HashSet};

    fn queued_task(queue: &mut PendingTasks, image_id: u64, file_name: &str, kind: DaemonTaskKind) {
        queue.push(PathBuf::from(file_name), kind, Some(image_id));
    }

    fn task_identity(task: &PendingTask) -> (u64, Option<usize>) {
        (
            task.key
                .review_image_id()
                .expect("test task has review image id"),
            task.kind.review_profile_index(),
        )
    }

    #[test]
    fn pending_tasks_preserve_fifo_without_review_priority() {
        let mut queue = PendingTasks::default();
        queued_task(&mut queue, 1, "first.NEF", DaemonTaskKind::RawProfile(1));
        queued_task(
            &mut queue,
            2,
            "second.JPG",
            DaemonTaskKind::StandaloneCompressed,
        );
        queued_task(
            &mut queue,
            3,
            "third.NEF",
            DaemonTaskKind::SoocSidecar {
                sidecar: PathBuf::from("third.JPG"),
            },
        );

        let active = HashSet::new();
        assert_eq!(
            queue.pop_fifo_excluding(&active).map(|task| task.raw),
            Some(PathBuf::from("first.NEF"))
        );
        assert_eq!(
            queue.pop_fifo_excluding(&active).map(|task| task.raw),
            Some(PathBuf::from("second.JPG"))
        );
        assert_eq!(
            queue.pop_fifo_excluding(&active).map(|task| task.raw),
            Some(PathBuf::from("third.NEF"))
        );
        assert!(queue.pop_fifo_excluding(&active).is_none());
    }

    #[test]
    fn pending_tasks_follow_all_six_review_priority_buckets() {
        let mut queue = PendingTasks::default();
        queued_task(&mut queue, 30, "hidden.NEF", DaemonTaskKind::RawProfile(0));
        queued_task(&mut queue, 20, "visible.NEF", DaemonTaskKind::RawProfile(0));
        queued_task(&mut queue, 10, "current.NEF", DaemonTaskKind::RawProfile(0));
        queued_task(&mut queue, 30, "hidden.NEF", DaemonTaskKind::RawProfile(2));
        queued_task(
            &mut queue,
            20,
            "visible.NEF",
            DaemonTaskKind::SoocSidecar {
                sidecar: PathBuf::from("visible.JPG"),
            },
        );
        queued_task(&mut queue, 10, "current.NEF", DaemonTaskKind::RawProfile(1));
        queued_task(
            &mut queue,
            10,
            "current.NEF",
            DaemonTaskKind::SoocSidecar {
                sidecar: PathBuf::from("current.JPG"),
            },
        );

        let schedule = |task: &PendingTask| {
            let (image_id, profile_index) = task_identity(task);
            let (bucket, image_order) = match (image_id, profile_index) {
                (10, Some(1)) => (1, 0),
                (10, _) => (2, 0),
                (20, Some(0)) => (3, 1),
                (20, _) => (4, 1),
                (30, Some(2)) => (5, 2),
                (30, _) => (6, 2),
                _ => return None,
            };
            Some((bucket, image_order, task.enqueue_sequence))
        };

        let active = HashSet::new();
        let mut claimed = Vec::new();
        while let Some(task) = queue.pop_ranked_excluding(&active, schedule) {
            claimed.push(task_identity(&task));
        }
        assert_eq!(
            claimed,
            vec![
                (10, Some(1)),
                (10, Some(0)),
                (10, Some(SOOC_PROFILE_INDEX)),
                (20, Some(0)),
                (20, Some(SOOC_PROFILE_INDEX)),
                (30, Some(2)),
                (30, Some(0)),
            ]
        );
    }

    #[test]
    fn pending_tasks_recompute_priority_for_every_claim() {
        let mut queue = PendingTasks::default();
        for image_id in [1, 2] {
            queued_task(
                &mut queue,
                image_id,
                &format!("{image_id}.NEF"),
                DaemonTaskKind::RawProfile(0),
            );
            queued_task(
                &mut queue,
                image_id,
                &format!("{image_id}.NEF"),
                DaemonTaskKind::RawProfile(1),
            );
        }

        let mut active = HashSet::new();
        let first = queue
            .pop_ranked_excluding(&active, |task| {
                let (image_id, profile_index) = task_identity(task);
                Some((
                    if image_id == 1 && profile_index == Some(0) {
                        1
                    } else if image_id == 1 {
                        2
                    } else if profile_index == Some(0) {
                        3
                    } else {
                        4
                    },
                    image_id as usize,
                    task.enqueue_sequence,
                ))
            })
            .unwrap();
        assert_eq!(task_identity(&first), (1, Some(0)));
        active.insert(first.key);

        let second = queue
            .pop_ranked_excluding(&active, |task| {
                let (image_id, profile_index) = task_identity(task);
                Some((
                    if image_id == 2 && profile_index == Some(1) {
                        1
                    } else if image_id == 2 {
                        2
                    } else if profile_index == Some(1) {
                        3
                    } else {
                        4
                    },
                    image_id as usize,
                    task.enqueue_sequence,
                ))
            })
            .unwrap();
        assert_eq!(task_identity(&second), (2, Some(1)));
    }

    #[test]
    fn pending_tasks_drop_disabled_profiles_and_keep_unknown_fifo() {
        let mut queue = PendingTasks::default();
        queued_task(&mut queue, 1, "disabled.NEF", DaemonTaskKind::RawProfile(9));
        queue.push(
            PathBuf::from("unknown-first.NEF"),
            DaemonTaskKind::RawProfile(0),
            None,
        );
        queue.push(
            PathBuf::from("unknown-second.JPG"),
            DaemonTaskKind::StandaloneCompressed,
            None,
        );
        queued_task(&mut queue, 2, "ranked.NEF", DaemonTaskKind::RawProfile(0));

        let schedule = |task: &PendingTask| match task.key.review_image_id() {
            Some(1) => None,
            Some(2) => Some((false, 5, 2, task.enqueue_sequence)),
            _ => Some((true, u8::MAX, usize::MAX, task.enqueue_sequence)),
        };
        let active = HashSet::new();
        assert_eq!(queue.drop_unschedulable(schedule), 1);
        assert_eq!(
            queue
                .pop_ranked_excluding(&active, schedule)
                .map(|task| task.raw),
            Some(PathBuf::from("ranked.NEF"))
        );
        assert_eq!(
            queue
                .pop_ranked_excluding(&active, schedule)
                .map(|task| task.raw),
            Some(PathBuf::from("unknown-first.NEF"))
        );
        assert_eq!(
            queue
                .pop_ranked_excluding(&active, schedule)
                .map(|task| task.raw),
            Some(PathBuf::from("unknown-second.JPG"))
        );
    }

    #[test]
    fn pending_tasks_coalesce_latest_payload_and_preserve_sequence() {
        let mut queue = PendingTasks::default();
        assert!(queue.push(
            PathBuf::from("first-path.NEF"),
            DaemonTaskKind::SoocSidecar {
                sidecar: PathBuf::from("first.JPG"),
            },
            Some(42),
        ));
        let sequence = queue.tasks[0].enqueue_sequence;

        assert!(!queue.push(
            PathBuf::from("updated-path.NEF"),
            DaemonTaskKind::SoocSidecar {
                sidecar: PathBuf::from("updated.JPG"),
            },
            Some(42),
        ));
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.tasks[0].raw, PathBuf::from("updated-path.NEF"));
        assert_eq!(queue.tasks[0].enqueue_sequence, sequence);
        assert!(matches!(
            &queue.tasks[0].kind,
            DaemonTaskKind::SoocSidecar { sidecar }
                if sidecar == &PathBuf::from("updated.JPG")
        ));
    }

    #[test]
    fn pending_tasks_deduplicate_by_path_without_review_ids() {
        let mut queue = PendingTasks::default();
        assert!(queue.push(
            PathBuf::from("frame.NEF"),
            DaemonTaskKind::RawProfile(0),
            None,
        ));
        assert!(!queue.push(
            PathBuf::from("frame.NEF"),
            DaemonTaskKind::RawProfile(0),
            None,
        ));
        assert!(queue.push(
            PathBuf::from("frame.NEF"),
            DaemonTaskKind::RawProfile(1),
            None,
        ));
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn pending_tasks_defer_one_rerun_while_matching_job_is_active() {
        let mut queue = PendingTasks::default();
        queued_task(&mut queue, 1, "active.NEF", DaemonTaskKind::RawProfile(0));
        queued_task(&mut queue, 2, "ready.NEF", DaemonTaskKind::RawProfile(0));
        let active_key = queue.tasks[0].key.clone();
        let active = HashSet::from([active_key]);

        assert_eq!(
            queue.pop_fifo_excluding(&active).map(|task| task.raw),
            Some(PathBuf::from("ready.NEF"))
        );
        assert!(queue.pop_fifo_excluding(&active).is_none());
        assert_eq!(queue.len(), 1);

        assert!(!queue.push(
            PathBuf::from("active-newer.NEF"),
            DaemonTaskKind::RawProfile(0),
            Some(1),
        ));
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.tasks[0].raw, PathBuf::from("active-newer.NEF"));

        assert_eq!(
            queue
                .pop_fifo_excluding(&HashSet::new())
                .map(|task| task.raw),
            Some(PathBuf::from("active-newer.NEF"))
        );
    }

    #[test]
    fn daemon_task_kinds_map_to_review_profile_identity() {
        assert_eq!(
            DaemonTaskKind::RawProfile(7).review_profile_index(),
            Some(7)
        );
        assert_eq!(
            DaemonTaskKind::SoocSidecar {
                sidecar: PathBuf::from("frame.JPG")
            }
            .review_profile_index(),
            Some(SOOC_PROFILE_INDEX)
        );
        assert_eq!(
            DaemonTaskKind::StandaloneCompressed.review_profile_index(),
            None
        );
    }

    #[test]
    fn compressed_inputs_queue_profile_tasks_only_for_explicit_profiles() {
        let root = tempfile::tempdir().unwrap();
        let input_root = root.path().join("in");
        let output_root = root.path().join("out");
        fs::create_dir_all(&input_root).unwrap();
        fs::create_dir_all(&output_root).unwrap();
        let context = ProfileScheduleContext {
            input_root: &input_root,
            output_root: &output_root,
            output_format: BatchOutputFormat::Jpg,
            input_file_filter: InputFileFilter::All,
            skip_existing: false,
            review: None,
        };
        let compressed = input_root.join("frame.JPG");
        fs::write(&compressed, b"jpeg").unwrap();

        let explicit = vec![Arc::new(DaemonProfile {
            selector: "Classic".to_string(),
            stem: "Classic".to_string(),
            resolved: neutral_profile(),
            profile_report: String::new(),
        })];
        let mut queue = PendingTasks::default();
        assert_eq!(
            enqueue_profile_jobs(&mut queue, &explicit, compressed.clone(), &context).unwrap(),
            2
        );
        assert!(matches!(
            queue.tasks.first().map(|task| &task.kind),
            Some(DaemonTaskKind::RawProfile(0))
        ));
        assert!(matches!(
            queue.tasks.last().map(|task| &task.kind),
            Some(DaemonTaskKind::SoocSidecar { sidecar }) if sidecar == &compressed
        ));

        let implicit = vec![Arc::new(DaemonProfile {
            selector: String::new(),
            stem: String::new(),
            resolved: neutral_profile(),
            profile_report: String::new(),
        })];
        queue.tasks.clear();
        assert_eq!(
            enqueue_profile_jobs(&mut queue, &implicit, compressed, &context).unwrap(),
            1
        );
        assert!(matches!(
            queue.tasks.first().map(|task| &task.kind),
            Some(DaemonTaskKind::StandaloneCompressed)
        ));
    }

    #[test]
    fn compressed_sidecars_never_queue_explicit_profile_tasks() {
        let root = tempfile::tempdir().unwrap();
        let input_root = root.path().join("in");
        let output_root = root.path().join("out");
        fs::create_dir_all(&input_root).unwrap();
        fs::create_dir_all(&output_root).unwrap();
        let raw = input_root.join("frame.NEF");
        let sidecar = input_root.join("frame.JPG");
        fs::write(&raw, b"raw").unwrap();
        fs::write(&sidecar, b"jpeg").unwrap();
        let context = ProfileScheduleContext {
            input_root: &input_root,
            output_root: &output_root,
            output_format: BatchOutputFormat::Jpg,
            input_file_filter: InputFileFilter::JpgOnly,
            skip_existing: false,
            review: None,
        };
        let profiles = vec![Arc::new(DaemonProfile {
            selector: "Classic".to_string(),
            stem: "Classic".to_string(),
            resolved: neutral_profile(),
            profile_report: String::new(),
        })];
        let mut queue = PendingTasks::default();

        assert_eq!(
            enqueue_profile_jobs(&mut queue, &profiles, sidecar, &context).unwrap(),
            0
        );
        assert!(queue.is_empty());
    }

    #[test]
    fn standalone_compressed_output_is_a_managed_source_link() {
        let root = tempfile::tempdir().unwrap();
        let input_root = root.path().join("in");
        let output_root = root.path().join("out");
        fs::create_dir_all(&input_root).unwrap();
        fs::create_dir_all(&output_root).unwrap();
        let compressed = input_root.join("frame.HEIC");
        let output = output_root.join("frame.heic");
        fs::write(&compressed, b"heic source").unwrap();
        fs::write(&output, b"old generated output").unwrap();
        let implicit = vec![Arc::new(DaemonProfile {
            selector: String::new(),
            stem: String::new(),
            resolved: neutral_profile(),
            profile_report: String::new(),
        })];

        assert_eq!(
            repair_compressed_output_links(
                std::slice::from_ref(&compressed),
                &implicit,
                &input_root,
                &output_root,
            )
            .unwrap(),
            1
        );
        assert!(
            fs::symlink_metadata(&output)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::canonicalize(output).unwrap(),
            fs::canonicalize(compressed).unwrap()
        );
    }

    #[test]
    fn raw_sidecars_and_profiled_compressed_sooc_are_source_links() {
        let root = tempfile::tempdir().unwrap();
        let input_root = root.path().join("in");
        let output_root = root.path().join("out");
        fs::create_dir_all(&input_root).unwrap();
        fs::create_dir_all(&output_root).unwrap();
        let raw = input_root.join("raw-frame.NEF");
        let sidecar = input_root.join("raw-frame.JPG");
        let standalone = input_root.join("standalone.TIFF");
        fs::write(&raw, b"raw").unwrap();
        fs::write(&sidecar, b"jpeg source").unwrap();
        fs::write(&standalone, b"tiff source").unwrap();
        let profiles = vec![Arc::new(DaemonProfile {
            selector: "Classic".to_string(),
            stem: "Classic".to_string(),
            resolved: neutral_profile(),
            profile_report: String::new(),
        })];

        assert_eq!(
            repair_compressed_output_links(
                &[raw, standalone.clone()],
                &profiles,
                &input_root,
                &output_root,
            )
            .unwrap(),
            2
        );
        for (output, source) in [
            (output_root.join("sooc/raw-frame.jpg"), sidecar),
            (output_root.join("sooc/standalone.tiff"), standalone),
        ] {
            assert!(
                fs::symlink_metadata(&output)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(
                fs::canonicalize(output).unwrap(),
                fs::canonicalize(source).unwrap()
            );
        }
    }

    #[test]
    fn daemon_profile_publication_never_replaces_a_matching_raw_with_its_sidecar() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("in");
        let output_root = root.path().join("out");
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output_root).unwrap();
        let raw = input.join("frame.NEF");
        let sidecar = input.join("frame.JPG");
        let output = output_root.join("frame.jpg");
        fs::write(&raw, b"raw source").unwrap();
        fs::write(&sidecar, b"sidecar source").unwrap();
        fs::write(&output, b"raw render").unwrap();

        let staged = daemon_profile_output_temp(root.path(), &output).unwrap();
        fs::write(&staged, b"sidecar render").unwrap();
        publish_daemon_profile_output(&sidecar, staged, &output).unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"raw render");
    }

    #[test]
    fn write_daemon_info_txt_emits_tree_profile_level_info_file() {
        let root = tempfile::tempdir().unwrap();
        let input_root = root.path().join("in");
        let output_root = root.path().join("out");
        let raw = input_root.join("day1").join("DSC_0001.NEF");
        fs::create_dir_all(raw.parent().unwrap()).unwrap();
        fs::write(&raw, b"raw").unwrap();

        let args = BatchDaemonArgs {
            input: input_root.clone(),
            output: output_root.clone(),
            profile: vec!["Portra 400 grainy".into()],
            input_file_filter: InputFileFilter::All,
            hald_dir: root.path().to_path_buf(),
            profiles_root: root.path().to_path_buf(),
            hald_level: 16,
            rawtherapee: PathBuf::from("rawtherapee"),
            dng_fallback: crate::app::dng::DngFallbackConfig::default(),
            convert: PathBuf::from("convert"),
            no_grain: false,
            normalize_grain_mpix: Some(12.0),
            grain: None,
            grain_preset: None,
            grain_seed: None,
            grain_engine: GrainEngine::default(),
            diffusion: DiffusionSettings::default(),
            color_noise_iso_threshold: 1600,
            lcp_root: None,
            lens_corrections: LensCorrections::default(),
            jobs: Some(2),
            debounce_seconds: 0,
            auto_import: false,
            nikon_wtu: None,
            nikon_wtu_port: 15740,
            nikon_wtu_name: None,
            nikon_wtu_guid: None,
            review_address: None,
            hugin_bin_dir: None,
            codex: None,
            codex_binary: PathBuf::from("codex"),
            codex_model: "gpt-5.4-mini".to_string(),
            codex_timeout: 45,
            gallery: None,
            gallery_thumbnail_long_edge: 1024,
            gallery_columns: 4,
            publish_album: "published".to_string(),
            output_format: BatchOutputFormat::Jpg,
            export: ExportOptions {
                jpg_quality: 80,
                resize: None,
                long_edge: None,
                max_width: None,
                max_height: None,
                jpeg_subsampling: crate::cli::JpegSubsampling::S420,
                strip_metadata: true,
                progressive_jpeg: false,
            },
            invocation: None,
        };

        let mut output_dirs = HashSet::new();
        let output = daemon_output_path(
            &input_root,
            &output_root,
            BatchOutputFormat::Jpg,
            &raw,
            "Portra 400 grainy",
        )
        .unwrap();
        output_dirs.insert(output.parent().unwrap().to_path_buf());

        let profile = Arc::new(DaemonProfile {
            selector: "Portra 400 grainy".to_string(),
            stem: "Portra 400 grainy".to_string(),
            resolved: ResolvedProfile {
                hald_path: None,
                rawtherapee_profiles: Vec::new(),
                grain: mini_film::GrainSettings::default(),
                sharpening_applied: false,
                resolved_stem: "Portra 400 grainy".to_string(),
                retouch_base: Default::default(),
                metadata: crate::app::profile::ResolvedProfileMetadata {
                    profile_name: "Portra 400 grainy".to_string(),
                    profile_uuid: None,
                    look_name: None,
                    look_uuid: None,
                    source_profile_name: None,
                    source_profile_uuid: None,
                    hald_path: None,
                    pp3_path: None,
                    pp3_adjustments: Vec::new(),
                    grain: mini_film::GrainSettings::default(),
                    source_adjustments: Default::default(),
                    source_sharpening: Default::default(),
                    emulation_adjustments: Default::default(),
                    emulation_sharpening: Default::default(),
                    has_camera_raw_settings: false,
                },
            },
            profile_report: "profile report".to_string(),
        });

        let state = DaemonProgressState {
            total_processed: 1,
            total_succeeded: 1,
            total_failed: 0,
            total_elapsed_ms: 100,
            started_at: Instant::now(),
            files: vec![DaemonFileResult {
                raw: raw.clone(),
                output,
                duration: Duration::from_millis(100),
                profile_index: Some(0),
                dcp_profile_filename: None,
                error: None,
                lens_profile_status: lens_profile_status(LensCorrections::default(), false),
                sharpening_status: sharpening_status(false),
                denoise_status: denoise_status(false),
            }],
            profile_stats: vec![DaemonProfileStats {
                processed: 1,
                succeeded: 1,
                failed: 0,
                elapsed_ms: 100,
            }],
            profile_output_dirs: vec![output_dirs],
            resource_usage: ResourceUsageSummary::default(),
            last_resource_sample: Instant::now(),
        };

        write_daemon_info_txt(
            &output_root,
            &args,
            std::slice::from_ref(&profile),
            &state,
            Duration::ZERO,
            None,
        )
        .unwrap();

        let tree_profile_info = output_root
            .join("day1")
            .join("Portra 400 grainy")
            .join("info.txt");
        assert!(tree_profile_info.exists());
        assert!(output_root.join("info.txt").exists());
        let txt = fs::read_to_string(tree_profile_info).unwrap();
        assert!(txt.contains("Portra 400 grainy"));
        assert!(txt.contains("Resource usage: unavailable"));
        assert!(txt.contains("Mini-film version:"));
        assert!(txt.contains("Time of day:"));
    }

    #[test]
    fn write_daemon_info_txt_includes_resource_usage_and_timing() {
        let root = tempfile::tempdir().unwrap();
        let args = BatchDaemonArgs {
            input: PathBuf::from("/input"),
            output: root.path().join("out"),
            profile: vec!["Portra 400 grainy".into()],
            input_file_filter: InputFileFilter::All,
            hald_dir: root.path().to_path_buf(),
            profiles_root: root.path().to_path_buf(),
            hald_level: 16,
            rawtherapee: PathBuf::from("rawtherapee"),
            dng_fallback: crate::app::dng::DngFallbackConfig::default(),
            convert: PathBuf::from("convert"),
            no_grain: false,
            normalize_grain_mpix: Some(12.0),
            grain: None,
            grain_preset: None,
            grain_seed: None,
            grain_engine: GrainEngine::default(),
            diffusion: DiffusionSettings::default(),
            color_noise_iso_threshold: 1600,
            lcp_root: None,
            lens_corrections: LensCorrections::default(),
            jobs: Some(2),
            debounce_seconds: 0,
            auto_import: false,
            nikon_wtu: None,
            nikon_wtu_port: 15740,
            nikon_wtu_name: None,
            nikon_wtu_guid: None,
            review_address: None,
            hugin_bin_dir: None,
            codex: None,
            codex_binary: PathBuf::from("codex"),
            codex_model: "gpt-5.4-mini".to_string(),
            codex_timeout: 45,
            gallery: None,
            gallery_thumbnail_long_edge: 1024,
            gallery_columns: 4,
            publish_album: "published".to_string(),
            output_format: BatchOutputFormat::Jpg,
            export: ExportOptions {
                jpg_quality: 80,
                resize: None,
                long_edge: None,
                max_width: None,
                max_height: None,
                jpeg_subsampling: crate::cli::JpegSubsampling::S420,
                strip_metadata: true,
                progressive_jpeg: false,
            },
            invocation: None,
        };
        let profile = Arc::new(DaemonProfile {
            selector: "Portra 400 grainy".to_string(),
            stem: "Portra 400 grainy".to_string(),
            resolved: ResolvedProfile {
                hald_path: None,
                rawtherapee_profiles: Vec::new(),
                grain: mini_film::GrainSettings::default(),
                sharpening_applied: false,
                resolved_stem: "Portra 400 grainy".to_string(),
                retouch_base: Default::default(),
                metadata: crate::app::profile::ResolvedProfileMetadata {
                    profile_name: "Portra 400 grainy".to_string(),
                    profile_uuid: None,
                    look_name: None,
                    look_uuid: None,
                    source_profile_name: None,
                    source_profile_uuid: None,
                    hald_path: None,
                    pp3_path: None,
                    pp3_adjustments: Vec::new(),
                    grain: mini_film::GrainSettings::default(),
                    source_adjustments: Default::default(),
                    source_sharpening: Default::default(),
                    emulation_adjustments: Default::default(),
                    emulation_sharpening: Default::default(),
                    has_camera_raw_settings: false,
                },
            },
            profile_report: "profile report".to_string(),
        });
        let state = DaemonProgressState {
            total_processed: 1,
            total_succeeded: 1,
            total_failed: 0,
            total_elapsed_ms: 1234,
            started_at: Instant::now(),
            files: vec![DaemonFileResult {
                raw: PathBuf::from("/input/DSC_0001.NEF"),
                output: PathBuf::from("/out/day/Portra 400 grainy/DSC_0001.jpg"),
                duration: Duration::from_millis(120),
                profile_index: Some(0),
                dcp_profile_filename: None,
                error: None,
                lens_profile_status: lens_profile_status(LensCorrections::default(), true),
                sharpening_status: sharpening_status(false),
                denoise_status: denoise_status(false),
            }],
            profile_stats: vec![DaemonProfileStats {
                processed: 1,
                succeeded: 1,
                failed: 0,
                elapsed_ms: 1234,
            }],
            profile_output_dirs: vec![{
                let mut dirs = HashSet::new();
                dirs.insert(
                    root.path()
                        .join("out")
                        .join("day")
                        .join("Portra 400 grainy"),
                );
                dirs
            }],
            resource_usage: ResourceUsageSummary::from(&crate::app::system_stats::ResourceUsage {
                process_cpu_percent: 12.3,
                process_memory_bytes: 2048,
                system_cpu_percent: 10.0,
                system_memory_used_bytes: 10_000,
                system_memory_total_bytes: 16_000,
            }),
            last_resource_sample: Instant::now(),
        };

        write_daemon_info_txt(
            &args.output,
            &args,
            std::slice::from_ref(&profile),
            &state,
            Duration::from_secs(1),
            Some(&state.resource_usage),
        )
        .unwrap();

        let root_info = args.output.join("info.txt");
        let tree_profile_info = args
            .output
            .join("day")
            .join("Portra 400 grainy")
            .join("info.txt");
        let root_txt = fs::read_to_string(root_info).unwrap();
        let profile_txt = fs::read_to_string(tree_profile_info).unwrap();
        assert!(root_txt.contains("CPU usage:"));
        assert!(root_txt.contains("Mini-film version:"));
        assert!(root_txt.contains("Time of day:"));
        assert!(profile_txt.contains("CPU usage:"));
        assert!(profile_txt.contains("Mini-film version:"));
        assert!(profile_txt.contains("Time of day:"));
        assert!(profile_txt.contains("Profile files: processed=1, succeeded=1, failed=0"));
        assert!(profile_txt.contains("Files for this profile:"));
        assert!(profile_txt.contains("DSC_0001.NEF"));
    }

    #[test]
    fn daemon_output_path_keeps_input_tree_and_uses_profile_dir() {
        let output = daemon_output_path(
            Path::new("/in"),
            Path::new("/out"),
            BatchOutputFormat::Jpg,
            Path::new("/in/day/DSC_0001.NEF"),
            "Portra 400 grainy",
        )
        .unwrap();
        assert_eq!(output, Path::new("/out/day/Portra 400 grainy/DSC_0001.jpg"));
    }

    #[test]
    fn daemon_stable_profile_seed_changes_with_profile_and_path() {
        assert_eq!(
            stable_profile_seed(1, Path::new("a.RAW"), 0),
            stable_profile_seed(1, Path::new("a.RAW"), 0)
        );
        assert_ne!(
            stable_profile_seed(1, Path::new("a.RAW"), 0),
            stable_profile_seed(1, Path::new("a.RAW"), 1)
        );
        assert_ne!(
            stable_profile_seed(1, Path::new("a.RAW"), 0),
            stable_profile_seed(2, Path::new("a.RAW"), 0)
        );
    }

    #[test]
    fn resolve_batch_daemon_jobs_validates_bounds() {
        assert_eq!(resolve_batch_daemon_jobs(Some(4)).unwrap(), 4);
        assert!(resolve_batch_daemon_jobs(Some(0)).is_err());
    }

    #[test]
    fn event_helpers_recognize_close_or_rename_events() {
        assert_eq!(
            event_stability_delay(
                &EventKind::Access(AccessKind::Close(AccessMode::Any)),
                Duration::ZERO
            ),
            Duration::ZERO
        );
        assert_eq!(
            event_stability_delay(
                &EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                Duration::from_millis(250),
            ),
            Duration::ZERO
        );
        assert_eq!(
            event_stability_delay(
                &EventKind::Modify(ModifyKind::Data(DataChange::Any)),
                Duration::from_millis(250),
            ),
            Duration::from_millis(250)
        );
    }

    #[test]
    fn event_relevance_matches_expected_variants() {
        assert!(is_relevant_daemon_event(&EventKind::Access(
            AccessKind::Close(AccessMode::Any)
        )));
        assert!(is_relevant_daemon_event(&EventKind::Modify(
            ModifyKind::Data(DataChange::Any)
        )));
        assert!(is_relevant_daemon_event(&EventKind::Modify(
            ModifyKind::Metadata(MetadataKind::Any)
        )));
        assert!(is_relevant_daemon_event(&EventKind::Modify(
            ModifyKind::Name(RenameMode::To)
        )));
        assert!(is_relevant_daemon_event(&EventKind::Create(
            CreateKind::File
        )));
        assert!(is_relevant_daemon_event(&EventKind::Any));
        assert!(!is_relevant_daemon_event(&EventKind::Remove(
            notify::event::RemoveKind::File
        )));
    }

    #[test]
    fn collect_due_paths_returns_stable_raws_and_requeues_changed() {
        let root = tempfile::tempdir().unwrap();
        let raw = root.path().join("frame.NEF");
        fs::write(&raw, b"hello").unwrap();
        let metadata = fs::metadata(&raw).unwrap();

        let mut pending = HashMap::new();
        pending.insert(
            raw.clone(),
            PendingFile {
                path: raw.clone(),
                process_at: Instant::now() - Duration::from_millis(1),
                size: metadata.len(),
                modified: metadata.modified().ok(),
            },
        );
        let due = collect_due_paths(&mut pending, Duration::ZERO);
        assert_eq!(due.len(), 1);
        assert!(pending.is_empty());

        let metadata = fs::metadata(&raw).unwrap();
        pending.insert(
            raw.clone(),
            PendingFile {
                path: raw.clone(),
                process_at: Instant::now() - Duration::from_millis(1),
                size: metadata.len(),
                modified: metadata.modified().ok(),
            },
        );
        fs::write(&raw, b"hello world").unwrap();
        let due = collect_due_paths(&mut pending, Duration::ZERO);
        assert!(due.is_empty());
        assert!(pending.contains_key(&raw));

        let future = Instant::now() + Duration::from_secs(30);
        pending.insert(
            raw.clone(),
            PendingFile {
                path: raw.clone(),
                process_at: future,
                size: metadata.len(),
                modified: metadata.modified().ok(),
            },
        );
        let due = collect_due_paths(&mut pending, Duration::from_secs(30));
        assert!(due.is_empty());
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn queue_input_file_filters_supported_inputs() {
        let root = tempfile::tempdir().unwrap();
        let mut pending = HashMap::new();

        let raw = root.path().join("frame.NEF");
        fs::write(&raw, b"raw").unwrap();
        let compressed = root.path().join("frame.jpg");
        fs::write(&compressed, b"jpg").unwrap();
        let unsupported = root.path().join("notes.txt");
        fs::write(&unsupported, b"text").unwrap();

        queue_input_file(
            &mut pending,
            raw.clone(),
            Duration::ZERO,
            InputFileFilter::All,
        );
        queue_input_file(
            &mut pending,
            compressed.clone(),
            Duration::ZERO,
            InputFileFilter::All,
        );
        queue_input_file(
            &mut pending,
            unsupported.clone(),
            Duration::ZERO,
            InputFileFilter::All,
        );

        assert!(pending.contains_key(&raw));
        assert!(pending.contains_key(&compressed));
        assert!(!pending.contains_key(&unsupported));
    }
}
