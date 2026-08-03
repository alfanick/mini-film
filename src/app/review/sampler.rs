use super::{
    handle::{
        profile_render_key_value_with_diffusion, queue_profile_retouch_render,
        retouch_white_balance_for_image,
    },
    model::*,
    prelude::*,
    store::*,
};
use crate::app::{
    dng::PreparedRawSource,
    export::add_convert_thread_limit_with_count,
    pp3::write_rawtherapee_disable_sharpening_profile,
    profile::{neutral_profile, profile_from_xmp_quiet},
    raw::run_raw_develop,
    sampler::{
        collect_xmp_profiles, emulation_root, profile_display_name_from_relative,
        profile_name_parts,
    },
    util::half_cpu_thread_count,
};
use crate::cli::{ExportOptions, JpegSubsampling};
use mini_film::write_rawtherapee_resize_profile;
use std::{
    fs::File,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const REVIEW_SAMPLER_CACHE_VERSION: &str = "review-sampler-v3-normalized-grain";
const REVIEW_SAMPLER_LONG_EDGE: u32 = 512;
const REVIEW_SAMPLER_JPEG_QUALITY: u8 = 85;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ReviewSamplerJobStatus {
    Preparing,
    Rendering,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ReviewSamplerEntryStatus {
    Queued,
    Rendering,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ReviewSamplerScope {
    Current,
    All,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(super) struct ReviewSamplerSelectionRequest {
    pub(super) scope: ReviewSamplerScope,
    pub(super) enabled: bool,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(super) struct ReviewSamplerStartRequest {
    pub(super) image_id: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct ReviewSamplerPriorityRequest {
    #[serde(default)]
    pub(super) visible_keys: Vec<String>,
    #[serde(default)]
    pub(super) expanded_keys: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct ReviewSamplerJobSnapshot {
    pub(super) id: u64,
    pub(super) image_id: u64,
    pub(super) file_name: String,
    pub(super) status: ReviewSamplerJobStatus,
    pub(super) source_url: Option<String>,
    pub(super) source_width: Option<u32>,
    pub(super) source_height: Option<u32>,
    pub(super) completed: usize,
    pub(super) total: usize,
    pub(super) failed: usize,
    pub(super) workers: usize,
    pub(super) error: Option<String>,
    pub(super) entries: Vec<ReviewSamplerEntrySnapshot>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct ReviewSamplerEntrySnapshot {
    pub(super) key: String,
    pub(super) name: String,
    pub(super) filename: String,
    pub(super) parts: Vec<String>,
    pub(super) status: ReviewSamplerEntryStatus,
    pub(super) thumbnail_url: Option<String>,
    pub(super) duration_ms: Option<u64>,
    pub(super) error: Option<String>,
    pub(super) current_enabled: bool,
    pub(super) all_enabled: bool,
    pub(super) configured_from_cli: bool,
    pub(super) selected: bool,
}

pub(super) struct ReviewSamplerRegistry {
    jobs: Mutex<HashMap<u64, ReviewSamplerJob>>,
    next_job_id: AtomicU64,
}

impl Default for ReviewSamplerRegistry {
    fn default() -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
            next_job_id: AtomicU64::new(1),
        }
    }
}

#[derive(Clone)]
struct ReviewSamplerJob {
    id: u64,
    image_id: u64,
    file_name: String,
    job_key: String,
    status: ReviewSamplerJobStatus,
    prepared_source: Option<PathBuf>,
    source_preview: Option<PathBuf>,
    source_width: Option<u32>,
    source_height: Option<u32>,
    completed: usize,
    failed: usize,
    workers: usize,
    error: Option<String>,
    entries: Vec<ReviewSamplerEntry>,
}

#[derive(Clone)]
struct ReviewSamplerEntry {
    key: String,
    identity: String,
    name: String,
    filename: String,
    parts: Vec<String>,
    profile_path: PathBuf,
    status: ReviewSamplerEntryStatus,
    thumbnail: Option<PathBuf>,
    duration_ms: Option<u64>,
    error: Option<String>,
    candidate: Option<ReviewProfile>,
    priority: u8,
}

struct PreparedSamplerSource {
    path: PathBuf,
    preview_path: PathBuf,
    digest: String,
    width: u32,
    height: u32,
}

impl ReviewHandle {
    pub(super) fn sampler_available(&self) -> bool {
        emulation_root(&self.profiles_root).is_dir()
    }

    pub(super) fn start_sampler_job(&self, image_id: u64) -> Result<ReviewSamplerJobSnapshot> {
        let (source, file_name) = {
            let store = self.store_snapshot();
            let image = store
                .images
                .iter()
                .find(|image| image.id == image_id)
                .ok_or_else(|| anyhow!("review image {image_id} does not exist"))?;
            if !image.raw_path.is_file() {
                bail!("sampler source is missing: {}", image.raw_path.display());
            }
            (image.raw_path.clone(), image.file_name.clone())
        };

        let root = emulation_root(&self.profiles_root);
        let profiles = collect_xmp_profiles(&root)?;
        if profiles.is_empty() {
            bail!("no XMP emulations found under {}", root.display());
        }
        let job_key = sampler_job_key(&source, &profiles)?;
        if let Some(job_id) = self.sampler_registry.find_job(image_id, &job_key) {
            return self.sampler_job_snapshot(job_id);
        }

        let entries = profiles
            .into_iter()
            .map(|profile_path| sampler_entry(&root, profile_path))
            .collect::<Result<Vec<_>>>()?;
        let id = self
            .sampler_registry
            .next_job_id
            .fetch_add(1, Ordering::Relaxed);
        let workers = half_cpu_thread_count();
        self.sampler_registry.insert(ReviewSamplerJob {
            id,
            image_id,
            file_name,
            job_key,
            status: ReviewSamplerJobStatus::Preparing,
            prepared_source: None,
            source_preview: None,
            source_width: None,
            source_height: None,
            completed: 0,
            failed: 0,
            workers,
            error: None,
            entries,
        });

        let handle = self.clone();
        thread::Builder::new()
            .name(format!("mini-film-review-sampler-{id}"))
            .spawn(move || handle.run_sampler_job(id, source))
            .context("starting review sampler worker")?;
        self.sampler_job_snapshot(id)
    }

    pub(super) fn sampler_job_snapshot(&self, job_id: u64) -> Result<ReviewSamplerJobSnapshot> {
        let job = self.sampler_registry.job(job_id)?;
        let store = self.store_snapshot();
        let image = store.images.iter().find(|image| image.id == job.image_id);
        let entries = job
            .entries
            .iter()
            .map(|entry| {
                let profile = store
                    .profiles
                    .iter()
                    .find(|profile| profile.identity == entry.identity);
                let current_enabled = profile.is_some_and(|profile| {
                    image.is_some_and(|image| {
                        image
                            .profiles
                            .iter()
                            .any(|render| render.profile_index == profile.index && render.enabled)
                    })
                });
                ReviewSamplerEntrySnapshot {
                    key: entry.key.clone(),
                    name: entry.name.clone(),
                    filename: entry.filename.clone(),
                    parts: entry.parts.clone(),
                    status: entry.status,
                    thumbnail_url: entry
                        .thumbnail
                        .as_ref()
                        .filter(|path| path.is_file())
                        .map(|_| format!("sampler-media/{job_id}/{}", entry.key)),
                    duration_ms: entry.duration_ms,
                    error: entry.error.clone(),
                    current_enabled,
                    all_enabled: profile.is_some_and(|profile| profile.enabled_by_default),
                    configured_from_cli: profile.is_some_and(|profile| profile.configured_from_cli),
                    selected: profile.is_some_and(|profile| {
                        image.is_some_and(|image| image.selected_profile_index == profile.index)
                    }),
                }
            })
            .collect();
        Ok(ReviewSamplerJobSnapshot {
            id: job.id,
            image_id: job.image_id,
            file_name: job.file_name,
            status: job.status,
            source_url: job
                .source_preview
                .as_ref()
                .filter(|path| path.is_file())
                .map(|_| format!("sampler-media/{job_id}/source")),
            source_width: job.source_width,
            source_height: job.source_height,
            completed: job.completed,
            total: job.entries.len(),
            failed: job.failed,
            workers: job.workers,
            error: job.error,
            entries,
        })
    }

    pub(super) fn sampler_media_path(&self, job_id: u64, key: &str) -> Result<PathBuf> {
        let job = self.sampler_registry.job(job_id)?;
        let path = if key == "source" {
            job.source_preview
        } else {
            job.entries
                .iter()
                .find(|entry| entry.key == key)
                .and_then(|entry| entry.thumbnail.clone())
        }
        .ok_or_else(|| anyhow!("sampler media is not ready"))?;
        if !path.starts_with(self.sampler_cache_root()) || !path.is_file() {
            bail!("sampler media is missing: {}", path.display());
        }
        Ok(path)
    }

    pub(super) fn prioritize_sampler_job(
        &self,
        job_id: u64,
        request: ReviewSamplerPriorityRequest,
    ) -> Result<ReviewSamplerJobSnapshot> {
        let visible = request.visible_keys.into_iter().collect::<HashSet<_>>();
        let expanded = request.expanded_keys.into_iter().collect::<HashSet<_>>();
        self.sampler_registry
            .set_priorities(job_id, &visible, &expanded)?;
        self.sampler_job_snapshot(job_id)
    }

    pub(super) async fn apply_sampler_selection_async(
        &self,
        job_id: u64,
        entry_key: &str,
        request: ReviewSamplerSelectionRequest,
    ) -> Result<ReviewSamplerJobSnapshot> {
        let (image_id, candidate) = self.sampler_registry.candidate(job_id, entry_key)?;
        let candidate_for_store = candidate.clone();
        let profile_index = self
            .update_store_async(|store| {
                let existing = store
                    .profiles
                    .iter()
                    .find(|profile| profile.identity == candidate_for_store.identity)
                    .map(|profile| profile.index);
                if !request.enabled && existing.is_none() {
                    return Ok(None);
                }
                let mut candidate = candidate_for_store;
                candidate.enabled_by_default =
                    request.scope == ReviewSamplerScope::All && request.enabled;
                let profile_index = match existing {
                    Some(index) => index,
                    None => store.ensure_sampler_profile(candidate)?,
                };
                let configured_from_cli = store
                    .profiles
                    .iter()
                    .find(|profile| profile.index == profile_index)
                    .is_some_and(|profile| profile.configured_from_cli);
                match request.scope {
                    ReviewSamplerScope::Current => {
                        store.set_profile_enabled_for_image(
                            image_id,
                            profile_index,
                            request.enabled,
                        )?;
                    }
                    ReviewSamplerScope::All => {
                        if configured_from_cli && !request.enabled {
                            bail!("profiles configured on the command line remain available to all images");
                        }
                        store.set_profile_enabled_for_all(profile_index, request.enabled)?;
                    }
                }
                Ok(Some(profile_index))
            })
            .await?;
        self.broadcast_state()?;
        if let Some(profile_index) = profile_index
            && request.enabled
        {
            let image_ids = match request.scope {
                ReviewSamplerScope::Current => vec![image_id],
                ReviewSamplerScope::All => {
                    let store = self.store_snapshot();
                    let mut ids = store
                        .images
                        .iter()
                        .map(|image| image.id)
                        .collect::<Vec<_>>();
                    ids.sort_by_key(|id| (*id != image_id, *id));
                    ids
                }
            };
            self.queue_sampler_profile_renders_async(profile_index, image_id, image_ids)
                .await?;
        }
        self.sampler_job_snapshot(job_id)
    }

    pub(super) fn schedule_ready_sampler_profile_renders(&self) -> Result<()> {
        let (current_image_id, jobs) = {
            let store = self.store_snapshot();
            let sampler_indexes = store
                .profiles
                .iter()
                .filter(|profile| profile.sampler_added)
                .map(|profile| profile.index)
                .collect::<Vec<_>>();
            let jobs = sampler_indexes
                .into_iter()
                .map(|profile_index| {
                    let mut image_ids = store
                        .images
                        .iter()
                        .filter(|image| {
                            image.profiles.iter().any(|render| {
                                render.profile_index == profile_index && render.enabled
                            })
                        })
                        .map(|image| image.id)
                        .collect::<Vec<_>>();
                    image_ids.sort_by_key(|id| (Some(*id) != store.ui.current_image_id, *id));
                    (profile_index, image_ids)
                })
                .collect::<Vec<_>>();
            (store.ui.current_image_id.unwrap_or_default(), jobs)
        };
        for (profile_index, image_ids) in jobs {
            self.database_runtime
                .block_on(self.queue_sampler_profile_renders_async(
                    profile_index,
                    current_image_id,
                    image_ids,
                ))?;
        }
        Ok(())
    }

    pub(super) fn schedule_sampler_profiles_for_source(&self, source: &Path) -> Result<()> {
        let (image_id, profile_indexes) = {
            let store = self.store_snapshot();
            let Some(image) = store.images.iter().find(|image| image.raw_path == source) else {
                return Ok(());
            };
            let sampler_indexes = store
                .profiles
                .iter()
                .filter(|profile| profile.sampler_added)
                .map(|profile| profile.index)
                .collect::<HashSet<_>>();
            let profile_indexes = image
                .profiles
                .iter()
                .filter(|render| render.enabled && sampler_indexes.contains(&render.profile_index))
                .map(|render| render.profile_index)
                .collect::<Vec<_>>();
            (image.id, profile_indexes)
        };
        for profile_index in profile_indexes {
            self.database_runtime
                .block_on(self.queue_sampler_profile_renders_async(
                    profile_index,
                    image_id,
                    vec![image_id],
                ))?;
        }
        Ok(())
    }

    async fn queue_sampler_profile_renders_async(
        &self,
        profile_index: usize,
        priority_image_id: u64,
        image_ids: Vec<u64>,
    ) -> Result<()> {
        let jobs = self
            .update_store_async(|store| {
                let profile = store
                    .profiles
                    .iter()
                    .find(|profile| profile.index == profile_index)
                    .cloned()
                    .ok_or_else(|| anyhow!("review profile {profile_index} does not exist"))?;
                let mut jobs = Vec::new();
                for image_id in image_ids {
                    let Some(image) = store.images.iter_mut().find(|image| image.id == image_id)
                    else {
                        continue;
                    };
                    let Some(render_index) = image
                        .profiles
                        .iter()
                        .position(|render| render.profile_index == profile_index && render.enabled)
                    else {
                        continue;
                    };
                    if image.profiles[render_index].output_path.is_none() {
                        image.profiles[render_index].output_path =
                            Some(crate::app::batch_daemon::daemon_output_path(
                                &self.input_root,
                                &self.output_root,
                                self.output_format,
                                &image.raw_path,
                                &profile.stem,
                            )?);
                    }
                    let processing_key = review_render_processing_key_for_input_with_diffusion(
                        &image.raw_path,
                        profile_index,
                        self.normalize_grain_mpix,
                        &self.export,
                        self.diffusion,
                    );
                    image.profiles[render_index].processing_key = Some(processing_key.clone());
                    let bw_filter = effective_bw_filter_for_profile(image, &profile);
                    let render_key = profile_render_key_value_with_diffusion(
                        &image.retouch,
                        retouch_white_balance_for_image(image),
                        bw_filter,
                        self.normalize_grain_mpix,
                        &processing_key,
                        self.diffusion,
                    );
                    queue_profile_retouch_render(
                        image,
                        render_index,
                        render_key,
                        image.retouch.clone().normalized() == RetouchSettings::default()
                            && bw_filter == BwFilter::None,
                        &mut jobs,
                        &self.output_root,
                        &self.cache_root,
                    );
                }
                Ok(jobs)
            })
            .await?;
        self.broadcast_state()?;
        for (position, job) in jobs.into_iter().enumerate() {
            let delay = if job.image_id == priority_image_id {
                Duration::ZERO
            } else {
                Duration::from_millis(25 + position as u64)
            };
            self.retouch_scheduler.schedule_after(job, delay);
        }
        Ok(())
    }

    fn run_sampler_job(&self, job_id: u64, source: PathBuf) {
        if let Err(error) = self.run_sampler_job_inner(job_id, &source) {
            self.sampler_registry.fail_job(job_id, format!("{error:#}"));
        }
    }

    fn run_sampler_job_inner(&self, job_id: u64, source: &Path) -> Result<()> {
        let prepared = self.prepare_sampler_source(source)?;
        self.sampler_registry
            .source_ready(job_id, &prepared, ReviewSamplerJobStatus::Rendering)?;
        let temp = Builder::new()
            .prefix("mini-film-review-sampler-")
            .tempdir()?;
        let workers = half_cpu_thread_count();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .thread_name(|index| format!("mini-film-sampler-profile-{}", index + 1))
            .build()
            .context("building review sampler profile pool")?;
        pool.install(|| {
            (0..workers).into_par_iter().for_each(|_| {
                while let Some((index, entry)) = self.sampler_registry.claim_entry(job_id) {
                    let started = Instant::now();
                    let result =
                        self.render_sampler_entry(source, &prepared, &entry, index, temp.path());
                    self.sampler_registry.finish_entry(
                        job_id,
                        &entry.key,
                        started.elapsed(),
                        result,
                    );
                }
            });
        });
        self.sampler_registry.finish_job(job_id);
        Ok(())
    }

    fn sampler_cache_root(&self) -> PathBuf {
        self.cache_root
            .join(crate::app::cache::SAMPLER_CACHE_DIR)
            .join(REVIEW_SAMPLER_CACHE_VERSION)
    }

    fn prepare_sampler_source(&self, source: &Path) -> Result<PreparedSamplerSource> {
        let digest = sampler_source_digest(self, source)?;
        let source_dir = self.sampler_cache_root().join("sources");
        fs::create_dir_all(&source_dir)
            .with_context(|| format!("creating {}", source_dir.display()))?;
        let output = source_dir.join(format!("{digest}.tif"));
        let preview = source_dir.join(format!("{digest}.jpg"));
        if let Ok((width, height)) = image::image_dimensions(&output) {
            ensure_sampler_source_preview(
                &self.convert,
                &output,
                &preview,
                half_cpu_thread_count(),
            )?;
            return Ok(PreparedSamplerSource {
                path: output,
                preview_path: preview,
                digest,
                width,
                height,
            });
        }

        let work = Builder::new()
            .prefix("mini-film-review-sampler-source-")
            .tempdir_in(&source_dir)?;
        let (developed, converted_source) = if is_raw_input_file(source) {
            let prepared_source = self.dng_fallback.prepare_known(source)?;
            let active_source = prepared_source.active();
            let dcp_profile = resolve_dcp_profile(active_source, &self.dng_fallback);
            let developed = work.path().join("neutral.tif");
            let neutral = neutral_profile();
            let mut profiles = rawtherapee_profiles_for_input(
                RawTherapeeProfileOptions {
                    input: active_source,
                    retouch: None,
                    retouch_white_balance: RetouchWhiteBalance::default(),
                    bw_filter: BwFilter::None,
                    color_noise_iso_threshold: self.color_noise_iso_threshold,
                    lens_corrections: self.lens_corrections,
                    dcp_profile: dcp_profile.as_ref(),
                },
                &neutral,
                work.path(),
            )?;
            profiles.push(write_rawtherapee_disable_sharpening_profile(
                &work.path().join("neutral-no-sharpening.pp3"),
            )?);
            profiles.push(write_rawtherapee_resize_profile(
                &work.path().join("neutral-resize.pp3"),
                REVIEW_SAMPLER_LONG_EDGE,
            )?);
            let outcome = run_raw_develop(
                &self.rawtherapee,
                &profiles,
                prepared_source,
                &developed,
                self.lcp_root.as_deref(),
                true,
                &self.dng_fallback,
            )?;
            (developed, Some(outcome.source))
        } else {
            (source.to_path_buf(), None)
        };

        let temp = Builder::new()
            .prefix(".mini-film-sampler-source-")
            .suffix(".tif")
            .tempfile_in(&source_dir)?
            .into_temp_path();
        fs::remove_file(&temp).with_context(|| format!("preparing {}", temp.display()))?;
        convert_sampler_source(&self.convert, &developed, &temp, half_cpu_thread_count())?;
        temp.persist(&output)
            .map_err(|error| error.error)
            .with_context(|| format!("publishing sampler source {}", output.display()))?;
        if let Some(converted_source) = converted_source {
            self.dng_fallback
                .finish_successful_development(&converted_source)?;
            if converted_source.active() != source {
                self.rebind_and_queue_converted_source(source, converted_source.active())?;
            }
        }
        let (width, height) = image::image_dimensions(&output)
            .with_context(|| format!("reading {}", output.display()))?;
        ensure_sampler_source_preview(&self.convert, &output, &preview, half_cpu_thread_count())?;
        Ok(PreparedSamplerSource {
            path: output,
            preview_path: preview,
            digest,
            width,
            height,
        })
    }

    fn render_sampler_entry(
        &self,
        original_source: &Path,
        prepared: &PreparedSamplerSource,
        entry: &ReviewSamplerEntry,
        index: usize,
        temp_root: &Path,
    ) -> Result<(PathBuf, ReviewProfile)> {
        let profile_temp = temp_root.join(format!("profile-{index:04}"));
        fs::create_dir_all(&profile_temp)
            .with_context(|| format!("creating {}", profile_temp.display()))?;
        let mut resolved = profile_from_xmp_quiet(
            &entry.profile_path,
            self.hald_level,
            &self.profiles_root,
            &self.hald_dir,
            &profile_temp,
        )?;
        if let Some(grain) =
            resolve_grain_override(self.grain.as_deref(), self.grain_preset.as_deref())?
        {
            resolved.grain = grain;
        }
        if is_jpeg_input_file(original_source) {
            resolved
                .rawtherapee_profiles
                .push(write_rawtherapee_disable_sharpening_profile(
                    &profile_temp.join("compressed-no-sharpening.pp3"),
                )?);
            resolved.sharpening_applied = false;
        }

        let metadata = ReviewProfileMetadata::from(&resolved.metadata);
        let identity =
            review_profile_identity(&entry.profile_path.to_string_lossy(), Some(&metadata));
        let profile_digest = sampler_profile_digest(prepared, entry, &resolved, self)?;
        let thumb_dir = self
            .sampler_cache_root()
            .join("thumbnails")
            .join(&prepared.digest);
        fs::create_dir_all(&thumb_dir)
            .with_context(|| format!("creating {}", thumb_dir.display()))?;
        let output = thumb_dir.join(format!("{profile_digest}.jpg"));
        if image::image_dimensions(&output).is_err() {
            let temp = Builder::new()
                .prefix(".mini-film-sampler-thumb-")
                .suffix(".jpg")
                .tempfile_in(&thumb_dir)?
                .into_temp_path();
            fs::remove_file(&temp).with_context(|| format!("preparing {}", temp.display()))?;
            let export = ExportOptions {
                jpg_quality: REVIEW_SAMPLER_JPEG_QUALITY,
                resize: None,
                long_edge: Some(REVIEW_SAMPLER_LONG_EDGE),
                max_width: None,
                max_height: None,
                jpeg_subsampling: JpegSubsampling::S420,
                strip_metadata: true,
                progressive_jpeg: true,
            };
            let seed = sampler_seed(&prepared.digest, &identity);
            apply_resolved(
                ApplyJob {
                    raw: &prepared.path,
                    output: &temp,
                    rawtherapee: &self.rawtherapee,
                    dng_fallback: &self.dng_fallback,
                    prepared_raw: Some(PreparedRawSource::unchanged(&prepared.path)),
                    convert: &self.convert,
                    keep_intermediate: None,
                    no_grain: self.no_grain,
                    normalize_grain_mpix: self.normalize_grain_mpix,
                    grain_engine: self.grain_engine,
                    diffusion: self.diffusion,
                    color_noise_iso_threshold: 0,
                    lens_corrections: LensCorrections::default(),
                    lcp_root: None,
                    export: &export,
                    quiet: true,
                    exif_comment: None,
                    retouch: None,
                    retouch_white_balance: RetouchWhiteBalance::default(),
                    bw_filter: BwFilter::None,
                    profile_input_cache_root: None,
                },
                &resolved,
                seed,
                &profile_temp,
                None,
            )?;
            temp.persist(&output)
                .map_err(|error| error.error)
                .with_context(|| format!("publishing sampler thumbnail {}", output.display()))?;
        }
        Ok((
            output,
            ReviewProfile {
                index: 0,
                identity,
                selector: entry.profile_path.to_string_lossy().to_string(),
                stem: resolved.resolved_stem,
                sampler_added: true,
                enabled_by_default: false,
                configured_from_cli: false,
                retouch_base: resolved.retouch_base,
                metadata: Some(metadata),
                hald_path: resolved.hald_path,
            },
        ))
    }
}

impl ReviewSamplerRegistry {
    fn jobs(&self) -> std::sync::MutexGuard<'_, HashMap<u64, ReviewSamplerJob>> {
        self.jobs.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn insert(&self, job: ReviewSamplerJob) {
        self.jobs().insert(job.id, job);
    }

    fn find_job(&self, image_id: u64, job_key: &str) -> Option<u64> {
        self.jobs()
            .values()
            .filter(|job| job.image_id == image_id && job.job_key == job_key)
            .max_by_key(|job| job.id)
            .map(|job| job.id)
    }

    fn job(&self, job_id: u64) -> Result<ReviewSamplerJob> {
        self.jobs()
            .get(&job_id)
            .cloned()
            .ok_or_else(|| anyhow!("sampler job {job_id} does not exist"))
    }

    fn candidate(&self, job_id: u64, entry_key: &str) -> Result<(u64, ReviewProfile)> {
        let jobs = self.jobs();
        let job = jobs
            .get(&job_id)
            .ok_or_else(|| anyhow!("sampler job {job_id} does not exist"))?;
        let entry = job
            .entries
            .iter()
            .find(|entry| entry.key == entry_key)
            .ok_or_else(|| anyhow!("sampler profile {entry_key:?} does not exist"))?;
        let candidate = entry
            .candidate
            .clone()
            .ok_or_else(|| anyhow!("sampler profile is not ready"))?;
        Ok((job.image_id, candidate))
    }

    fn source_ready(
        &self,
        job_id: u64,
        source: &PreparedSamplerSource,
        status: ReviewSamplerJobStatus,
    ) -> Result<()> {
        let mut jobs = self.jobs();
        let job = jobs
            .get_mut(&job_id)
            .ok_or_else(|| anyhow!("sampler job {job_id} does not exist"))?;
        job.prepared_source = Some(source.path.clone());
        job.source_preview = Some(source.preview_path.clone());
        job.source_width = Some(source.width);
        job.source_height = Some(source.height);
        job.status = status;
        Ok(())
    }

    fn claim_entry(&self, job_id: u64) -> Option<(usize, ReviewSamplerEntry)> {
        let mut jobs = self.jobs();
        let job = jobs.get_mut(&job_id)?;
        let index = job
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.status == ReviewSamplerEntryStatus::Queued)
            .max_by_key(|(index, entry)| (entry.priority, std::cmp::Reverse(*index)))
            .map(|(index, _)| index)?;
        job.entries[index].status = ReviewSamplerEntryStatus::Rendering;
        Some((index, job.entries[index].clone()))
    }

    fn set_priorities(
        &self,
        job_id: u64,
        visible: &HashSet<String>,
        expanded: &HashSet<String>,
    ) -> Result<()> {
        let mut jobs = self.jobs();
        let job = jobs
            .get_mut(&job_id)
            .ok_or_else(|| anyhow!("sampler job {job_id} does not exist"))?;
        for entry in &mut job.entries {
            entry.priority = if visible.contains(&entry.key) {
                2
            } else if expanded.contains(&entry.key) {
                1
            } else {
                0
            };
        }
        Ok(())
    }

    fn finish_entry(
        &self,
        job_id: u64,
        key: &str,
        duration: Duration,
        result: Result<(PathBuf, ReviewProfile)>,
    ) {
        let mut jobs = self.jobs();
        let Some(job) = jobs.get_mut(&job_id) else {
            return;
        };
        let Some(entry) = job.entries.iter_mut().find(|entry| entry.key == key) else {
            return;
        };
        entry.duration_ms = Some(duration.as_millis() as u64);
        match result {
            Ok((thumbnail, candidate)) => {
                entry.status = ReviewSamplerEntryStatus::Done;
                entry.thumbnail = Some(thumbnail);
                entry.candidate = Some(candidate);
                entry.error = None;
            }
            Err(error) => {
                entry.status = ReviewSamplerEntryStatus::Failed;
                entry.error = Some(format!("{error:#}"));
                job.failed += 1;
            }
        }
        job.completed += 1;
    }

    fn finish_job(&self, job_id: u64) {
        if let Some(job) = self.jobs().get_mut(&job_id) {
            job.status = ReviewSamplerJobStatus::Done;
        }
    }

    fn fail_job(&self, job_id: u64, error: String) {
        if let Some(job) = self.jobs().get_mut(&job_id) {
            job.status = ReviewSamplerJobStatus::Failed;
            job.error = Some(error);
        }
    }
}

fn sampler_entry(root: &Path, profile_path: PathBuf) -> Result<ReviewSamplerEntry> {
    let relative = profile_path
        .strip_prefix(root)
        .unwrap_or(&profile_path)
        .to_string_lossy()
        .to_string();
    let name = profile_display_name_from_relative(&relative);
    let key = short_sha1(relative.as_bytes());
    let identity = mini_film::extract_film_recipe(&profile_path)
        .ok()
        .and_then(|recipe| recipe.uuid)
        .as_deref()
        .map(str::trim)
        .filter(|uuid| !uuid.is_empty())
        .map(|uuid| format!("xmp:{}", uuid.to_ascii_lowercase()))
        .unwrap_or_else(|| review_profile_identity(&profile_path.to_string_lossy(), None));
    Ok(ReviewSamplerEntry {
        key,
        identity,
        parts: profile_name_parts(&name),
        name,
        filename: relative,
        profile_path,
        status: ReviewSamplerEntryStatus::Queued,
        thumbnail: None,
        duration_ms: None,
        error: None,
        candidate: None,
        priority: 0,
    })
}

fn sampler_job_key(source: &Path, profiles: &[PathBuf]) -> Result<String> {
    let metadata = fs::metadata(source).with_context(|| format!("reading {}", source.display()))?;
    let mut hasher = Sha1::new();
    hasher.update(REVIEW_SAMPLER_CACHE_VERSION.as_bytes());
    hasher.update(source.to_string_lossy().as_bytes());
    hasher.update(metadata.len().to_le_bytes());
    hash_modified_time(&mut hasher, metadata.modified().ok());
    for profile in profiles {
        hasher.update(profile.to_string_lossy().as_bytes());
        if let Ok(metadata) = fs::metadata(profile) {
            hasher.update(metadata.len().to_le_bytes());
            hash_modified_time(&mut hasher, metadata.modified().ok());
        }
    }
    Ok(hex_digest(hasher.finalize()))
}

fn sampler_source_digest(handle: &ReviewHandle, source: &Path) -> Result<String> {
    let mut hasher = Sha1::new();
    hasher.update(REVIEW_SAMPLER_CACHE_VERSION.as_bytes());
    hasher.update(REVIEW_SAMPLER_LONG_EDGE.to_le_bytes());
    hasher.update(handle.color_noise_iso_threshold.to_le_bytes());
    hasher.update(format!("{:?}", handle.lens_corrections));
    hasher.update(dcp_cache_identity(source, &handle.dng_fallback));
    hash_file_into(&mut hasher, source)?;
    Ok(hex_digest(hasher.finalize()))
}

fn sampler_profile_digest(
    prepared: &PreparedSamplerSource,
    entry: &ReviewSamplerEntry,
    resolved: &crate::app::profile::ResolvedProfile,
    handle: &ReviewHandle,
) -> Result<String> {
    let mut hasher = Sha1::new();
    hasher.update(REVIEW_SAMPLER_CACHE_VERSION.as_bytes());
    hasher.update(prepared.digest.as_bytes());
    hasher.update(REVIEW_SAMPLER_JPEG_QUALITY.to_le_bytes());
    hasher.update(format!("{}:{}", handle.no_grain, handle.grain_engine));
    hasher.update(grain_normalization_identity(handle.normalize_grain_mpix));
    hasher.update(handle.grain.as_deref().unwrap_or_default());
    hasher.update(handle.grain_preset.as_deref().unwrap_or_default());
    hash_file_into(&mut hasher, &entry.profile_path)?;
    for profile in &resolved.rawtherapee_profiles {
        hash_file_into(&mut hasher, profile)?;
    }
    if let Some(hald) = &resolved.hald_path {
        hasher.update(hald.to_string_lossy().as_bytes());
        if let Ok(metadata) = fs::metadata(hald) {
            hasher.update(metadata.len().to_le_bytes());
            hash_modified_time(&mut hasher, metadata.modified().ok());
        }
    }
    Ok(hex_digest(hasher.finalize()))
}

fn hash_file_into(hasher: &mut Sha1, path: &Path) -> Result<()> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
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
    Ok(())
}

fn hash_modified_time(hasher: &mut Sha1, modified: Option<SystemTime>) {
    if let Some(duration) = modified.and_then(|time| time.duration_since(UNIX_EPOCH).ok()) {
        hasher.update(duration.as_secs().to_le_bytes());
        hasher.update(duration.subsec_nanos().to_le_bytes());
    }
}

fn sampler_seed(source_digest: &str, identity: &str) -> u64 {
    let mut hasher = Sha1::new();
    hasher.update(source_digest.as_bytes());
    hasher.update(identity.as_bytes());
    let digest = hasher.finalize();
    u64::from_le_bytes(digest[..8].try_into().expect("SHA-1 has at least 8 bytes"))
}

fn short_sha1(value: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(value);
    hex_digest(hasher.finalize())[..16].to_string()
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn convert_sampler_source(convert: &Path, input: &Path, output: &Path, jobs: usize) -> Result<()> {
    let mut command = Command::new(convert);
    add_convert_thread_limit_with_count(&mut command, convert, jobs);
    let result = command
        .arg(input)
        .arg("-auto-orient")
        .arg("-filter")
        .arg("Triangle")
        .arg("-resize")
        .arg(format!(
            "{REVIEW_SAMPLER_LONG_EDGE}x{REVIEW_SAMPLER_LONG_EDGE}>"
        ))
        .arg("-depth")
        .arg("16")
        .arg("-compress")
        .arg("Zip")
        .arg(output)
        .output()
        .with_context(|| format!("preparing sampler TIFF with {}", convert.display()))?;
    if !result.status.success() {
        bail!(
            "sampler TIFF conversion failed with status {}\nstderr:\n{}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        );
    }
    if !output.is_file() {
        bail!(
            "sampler TIFF conversion did not create {}",
            output.display()
        );
    }
    Ok(())
}

fn ensure_sampler_source_preview(
    convert: &Path,
    input: &Path,
    output: &Path,
    jobs: usize,
) -> Result<()> {
    if image::image_dimensions(output).is_ok() {
        return Ok(());
    }
    let parent = output
        .parent()
        .ok_or_else(|| anyhow!("sampler preview path has no parent: {}", output.display()))?;
    let temp = Builder::new()
        .prefix(".mini-film-sampler-preview-")
        .suffix(".jpg")
        .tempfile_in(parent)?
        .into_temp_path();
    fs::remove_file(&temp).with_context(|| format!("preparing {}", temp.display()))?;
    let mut command = Command::new(convert);
    add_convert_thread_limit_with_count(&mut command, convert, jobs);
    let result = command
        .arg(input)
        .arg("-auto-orient")
        .arg("-depth")
        .arg("8")
        .arg("-interlace")
        .arg("Line")
        .arg("-sampling-factor")
        .arg("2x2,1x1,1x1")
        .arg("-quality")
        .arg(REVIEW_SAMPLER_JPEG_QUALITY.to_string())
        .arg(&temp)
        .output()
        .with_context(|| format!("preparing sampler preview with {}", convert.display()))?;
    if !result.status.success() {
        bail!(
            "sampler preview conversion failed with status {}\nstderr:\n{}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        );
    }
    temp.persist(output)
        .map_err(|error| error.error)
        .with_context(|| format!("publishing sampler preview {}", output.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queued_entry(key: &str) -> ReviewSamplerEntry {
        ReviewSamplerEntry {
            key: key.to_string(),
            identity: format!("test:{key}"),
            name: key.to_string(),
            filename: format!("{key}.xmp"),
            parts: vec![key.to_string()],
            profile_path: PathBuf::from(format!("{key}.xmp")),
            status: ReviewSamplerEntryStatus::Queued,
            thumbnail: None,
            duration_ms: None,
            error: None,
            candidate: None,
            priority: 0,
        }
    }

    #[test]
    fn visible_sampler_entries_precede_expanded_and_background_entries() {
        let registry = ReviewSamplerRegistry::default();
        registry.insert(ReviewSamplerJob {
            id: 1,
            image_id: 1,
            file_name: "frame.NEF".to_string(),
            job_key: "job".to_string(),
            status: ReviewSamplerJobStatus::Rendering,
            prepared_source: None,
            source_preview: None,
            source_width: None,
            source_height: None,
            completed: 0,
            failed: 0,
            workers: 2,
            error: None,
            entries: vec![
                queued_entry("background"),
                queued_entry("expanded"),
                queued_entry("visible"),
            ],
        });
        registry
            .set_priorities(
                1,
                &HashSet::from(["visible".to_string()]),
                &HashSet::from(["expanded".to_string(), "visible".to_string()]),
            )
            .unwrap();

        assert_eq!(registry.claim_entry(1).unwrap().1.key, "visible");
        assert_eq!(registry.claim_entry(1).unwrap().1.key, "expanded");
        assert_eq!(registry.claim_entry(1).unwrap().1.key, "background");
        assert!(registry.claim_entry(1).is_none());
    }
}
