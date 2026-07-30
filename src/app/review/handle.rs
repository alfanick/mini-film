use super::{
    db::*, gallery_download::*, history::*, model::*, prelude::*, preview::*, publish::*,
    sampler::*, scheduler::*, server::*, store::*,
};
use crate::app::cache::{
    PANORAMA_CACHE_DIR, PROFILE_DETAILS_CACHE_DIR, RETOUCH_CACHE_DIR, REVIEW_PREVIEWS_CACHE_DIR,
};
use crate::app::panorama::{
    PanoramaPreview, PanoramaProgress, PanoramaProgressSink, render_final, render_preview_row,
};

pub(super) const REVIEW_CODEX_WORKERS: usize = 2;
pub(super) const REVIEW_THUMBNAIL_WORKERS: usize = 1;
pub(super) const REVIEW_PREVIEW_WORKERS: usize = 2;

pub(super) struct ReviewProfileRetouchTask {
    pub(super) raw: PathBuf,
    profile: ReviewProfile,
    retouch: RetouchSettings,
    white_balance: RetouchWhiteBalance,
    bw_filter: BwFilter,
}

/// Start the embedded review server and return a handle daemon workers can update.
///
/// The server is an async Axum/Tokio HTTP listener running on its own thread.
/// Daemon workers update the shared review store whenever RAW files are queued
/// or rendered, and browser clients subscribe to `/api/events` for live SSE
/// state updates. This keeps review responsive while daemon workers keep
/// processing in parallel.
pub(crate) fn start_review_server(config: ReviewConfig) -> Result<ReviewHandle> {
    if !matches!(config.output_format, BatchOutputFormat::Jpg) {
        bail!(
            "daemon review currently requires --output-format jpg because browsers cannot preview TIFF outputs"
        );
    }

    fs::create_dir_all(&config.output_root)
        .with_context(|| format!("creating {}", config.output_root.display()))?;
    let database_runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("mini-film-review-db")
            .enable_all()
            .build()
            .context("building review database runtime")?,
    );
    let (database, stored) = database_runtime.block_on(ReviewDatabase::open_output(
        &config.input_root,
        &config.output_root,
    ))?;
    let cache_root = database.cache_root().to_path_buf();
    let mut store = stored
        .clone()
        .unwrap_or_else(|| ReviewStore::new(Vec::new()));
    store.normalize_grain_mpix = config.normalize_grain_mpix;
    store.render_export.clone_from(&config.export);
    let stored_raw_paths = store
        .images
        .iter()
        .map(|image| image.raw_path.clone())
        .collect::<Vec<_>>();
    for old_path in stored_raw_paths {
        if old_path.is_file() {
            continue;
        }
        if let Some(successor) = config.dng_fallback.existing_successor(&old_path)? {
            store.rebind_raw_source(&config.input_root, &old_path, successor.active())?;
        }
    }
    let needs_exif_schema_refresh = store.needs_exif_schema_refresh();
    store.sync_profiles(config.profiles);
    let refreshed_exif_count = if needs_exif_schema_refresh {
        let count = store.refresh_missing_exif_data_for_schema();
        store.mark_exif_schema_refreshed();
        count
    } else {
        store.refresh_missing_exif_data()
    };
    if refreshed_exif_count > 0 {
        eprintln!(
            "review metadata: refreshed {refreshed_exif_count} images in parallel with {} workers",
            cpu_thread_count()
        );
    }
    if let Some(stored) = &stored {
        database_runtime.block_on(database.apply_delta(stored, &store))?;
    } else {
        database_runtime.block_on(database.replace_store(&store))?;
    }
    let state_path = database.path().to_path_buf();
    let history_profiles = store.profiles.clone();
    let panorama_config = crate::app::panorama::PanoramaConfig {
        hugin_bin_dir: config.hugin_bin_dir.clone(),
        rawtherapee: config.rawtherapee.clone(),
        dng_fallback: config.dng_fallback.clone(),
        convert: config.convert.clone(),
        jobs: config.jobs,
        color_noise_iso_threshold: config.color_noise_iso_threshold,
        lens_corrections: config.lens_corrections,
        lcp_root: config.lcp_root.clone(),
    };
    let panorama_capability =
        crate::app::panorama::PanoramaCapability::probe(panorama_config.hugin_bin_dir.as_deref());
    let mut panorama_projects = database_runtime.block_on(database.load_panorama_projects())?;
    for project in &mut panorama_projects {
        if matches!(
            project.status,
            ReviewPanoramaStatus::Previewing | ReviewPanoramaStatus::Rendering
        ) {
            project.status = ReviewPanoramaStatus::Interrupted;
            project.progress_stage = None;
            project.error =
                Some("mini-film restarted before this panorama operation completed".to_string());
            project.updated_at = now_string();
            database_runtime.block_on(database.save_panorama_project(project))?;
        }
    }

    let gallery_defaults = handle_gallery_defaults(&config.gallery);
    let publish_defaults = ReviewPublishDefaults::new(
        config.publish_album,
        config.output_format,
        &config.export,
        gallery_defaults,
        config.grain_engine,
        config.normalize_grain_mpix,
    );
    let codex = config
        .codex
        .filter(|flags| flags.is_enabled())
        .map(|flags| ReviewCodexConfig {
            flags,
            codex_binary: config.codex_binary,
            model: config.codex_model,
            timeout: config.codex_timeout,
        });
    let (subscribers, _) = broadcast::channel(256);
    let handle = ReviewHandle {
        state: Arc::new(ArcSwap::from_pointee(store)),
        subscribers: Arc::new(subscribers),
        state_cache: Arc::new(ArcSwapOption::empty()),
        state_path,
        database,
        database_runtime,
        input_root: config.input_root,
        output_root: config.output_root,
        cache_root,
        hald_dir: config.hald_dir,
        profiles_root: config.profiles_root,
        hald_level: config.hald_level,
        rawtherapee: config.rawtherapee,
        dng_fallback: config.dng_fallback,
        lcp_root: config.lcp_root,
        output_format: config.output_format,
        gallery: config.gallery,
        convert: config.convert,
        export: config.export.clone(),
        jobs: config.jobs,
        no_grain: config.no_grain,
        color_noise_iso_threshold: config.color_noise_iso_threshold,
        lens_corrections: config.lens_corrections,
        grain: config.grain,
        grain_preset: config.grain_preset,
        grain_seed: config.grain_seed,
        grain_engine: config.grain_engine,
        normalize_grain_mpix: config.normalize_grain_mpix,
        publish_defaults,
        publish_jobs: Arc::new(ArcSwap::from_pointee(Vec::new())),
        next_publish_job_id: Arc::new(AtomicU64::new(1)),
        media_scheduler: Arc::new(ReviewMediaScheduler::default()),
        retouch_scheduler: Arc::new(ReviewRetouchScheduler::default()),
        codex,
        codex_scheduler: Arc::new(ReviewCodexScheduler::default()),
        invocation: config.invocation,
        panorama_config,
        panorama_capability,
        panorama_projects: Arc::new(ArcSwap::from_pointee(panorama_projects)),
        panorama_operation: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        sampler_registry: Arc::new(ReviewSamplerRegistry::default()),
        trusted_input_sender: config.trusted_input_sender,
        converted_input_sender: config.converted_input_sender,
    };
    handle.refresh_state_cache()?;
    handle.append_history(history_server_started(
        &handle.input_root,
        &handle.output_root,
        &history_profiles,
    ))?;

    let listener = std::net::TcpListener::bind(&config.address)
        .with_context(|| format!("binding review server to {}", config.address))?;
    listener
        .set_nonblocking(true)
        .context("setting review listener nonblocking")?;
    let server_handle = handle.clone();
    thread::Builder::new()
        .name("mini-film-review".to_string())
        .spawn(move || {
            if let Err(error) = run_review_listener(listener, server_handle) {
                eprintln!("review server failed: {error:#}");
            }
        })
        .context("starting daemon review server thread")?;
    handle.start_media_scheduler()?;
    handle.start_retouch_scheduler()?;
    handle.schedule_ready_sampler_profile_renders()?;
    handle.start_codex_scheduler()?;
    handle.schedule_ready_codex_jobs()?;

    Ok(handle)
}

impl ReviewHandle {
    pub(crate) fn auto_import_catalog(&self) -> AutoImportCatalog {
        self.database
            .auto_import_catalog(Arc::clone(&self.database_runtime))
    }

    pub(crate) fn state_path(&self) -> &Path {
        &self.state_path
    }

    pub(crate) fn prefetch_startup_exif_metadata(&self, inputs: &[PathBuf]) -> usize {
        let store = self.state.load();
        let mut seen = HashSet::new();
        let files = inputs
            .iter()
            .filter(|path| seen.insert((*path).clone()))
            .filter(|path| {
                store
                    .images
                    .iter()
                    .find(|image| image.raw_path.as_path() == path.as_path())
                    .is_none_or(|image| gallery_exif_needs_refresh(&image.exif, false))
            })
            .cloned()
            .collect::<Vec<_>>();
        drop(store);
        prefetch_gallery_exif(&files);
        files.len()
    }

    pub(crate) fn rebind_raw_source(&self, old_path: &Path, new_path: &Path) -> Result<bool> {
        let changed = self
            .update_store(|store| store.rebind_raw_source(&self.input_root, old_path, new_path))?;
        if changed {
            self.broadcast_state()?;
        }
        Ok(changed)
    }

    pub(super) fn rebind_and_queue_converted_source(
        &self,
        old_path: &Path,
        new_path: &Path,
    ) -> Result<bool> {
        let changed = self.rebind_raw_source(old_path, new_path)?;
        if changed && let Some(sender) = &self.converted_input_sender {
            sender
                .send(new_path.to_path_buf())
                .context("queueing converted DNG in daemon")?;
        }
        Ok(changed)
    }

    pub(super) fn append_history(&self, entry: HistoryEntry) -> Result<()> {
        append_history_entry(&self.output_root, entry)
    }

    pub(crate) fn publish_root(&self) -> PathBuf {
        self.output_root.join("reviewed")
    }

    pub(super) fn output_root(&self) -> &Path {
        &self.output_root
    }

    pub(crate) fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    pub(super) fn preview_root(&self) -> PathBuf {
        self.cache_root.join(REVIEW_PREVIEWS_CACHE_DIR)
    }

    pub(super) fn panorama_cache_root(&self) -> PathBuf {
        self.cache_root.join(PANORAMA_CACHE_DIR)
    }

    pub(super) fn panorama_projects_snapshot(&self) -> Arc<Vec<ReviewPanoramaProject>> {
        self.panorama_projects.load_full()
    }

    pub(super) async fn create_panorama_project_async(
        &self,
        request: ReviewPanoramaCreateRequest,
    ) -> Result<u64> {
        self.ensure_panorama_available()?;
        let sources = self.panorama_source_paths(&request.image_ids)?;
        let default_name = sources
            .first()
            .and_then(|path| path.file_stem())
            .and_then(|stem| stem.to_str())
            .map(|stem| format!("{stem} panorama"))
            .unwrap_or_else(|| "Panorama".to_string());
        let now = now_string();
        let mut project = ReviewPanoramaProject {
            id: 0,
            name: normalize_panorama_name(request.name.as_deref().unwrap_or(&default_name))?,
            status: ReviewPanoramaStatus::Draft,
            matching_mode: request.matching_mode,
            selected_projection: None,
            output_path: None,
            result_image_id: None,
            progress_stage: None,
            progress_completed: 0,
            progress_total: 0,
            error: None,
            created_at: now.clone(),
            updated_at: now,
            image_ids: request.image_ids,
            previews: Vec::new(),
        };
        let _write_guard = self.database.write_lock.lock().await;
        self.database.create_panorama_project(&mut project).await?;
        let id = project.id;
        self.cache_panorama_project(project);
        drop(_write_guard);
        self.broadcast_state()?;
        Ok(id)
    }

    pub(super) async fn update_panorama_project_async(
        &self,
        project_id: u64,
        request: ReviewPanoramaUpdateRequest,
    ) -> Result<()> {
        let mut project = self.panorama_project(project_id)?;
        if matches!(
            project.status,
            ReviewPanoramaStatus::Previewing | ReviewPanoramaStatus::Rendering
        ) {
            bail!("panorama project {project_id} is currently processing");
        }
        let mut invalidates_previews = false;
        if let Some(image_ids) = request.image_ids {
            self.panorama_source_paths(&image_ids)?;
            invalidates_previews |= project.image_ids != image_ids;
            project.image_ids = image_ids;
        }
        if let Some(name) = request.name {
            let name = normalize_panorama_name(&name)?;
            if project.name != name {
                project.name = name;
                project.output_path = None;
            }
        }
        if let Some(matching_mode) = request.matching_mode {
            invalidates_previews |= project.matching_mode != matching_mode;
            project.matching_mode = matching_mode;
        }
        if invalidates_previews {
            project.previews.clear();
            project.selected_projection = None;
            project.status = ReviewPanoramaStatus::Draft;
        } else if let Some(projection) = request.selected_projection {
            project.selected_projection = Some(projection);
        }
        project.error = None;
        project.updated_at = now_string();
        self.save_panorama_project_async(project).await?;
        self.broadcast_state()
    }

    pub(super) async fn start_panorama_previews_async(
        &self,
        project_id: u64,
        request: ReviewPanoramaPreviewRequest,
    ) -> Result<()> {
        self.ensure_panorama_available()?;
        self.claim_panorama_operation()?;
        let result = self.prepare_panorama_preview_job(project_id, request).await;
        let (project, sources) = match result {
            Ok(job) => job,
            Err(error) => {
                self.panorama_operation.store(false, Ordering::Release);
                return Err(error);
            }
        };
        let handle = self.clone();
        let spawn = thread::Builder::new()
            .name("mini-film-panorama-preview".to_string())
            .spawn(move || handle.run_panorama_preview_job(project, sources));
        if let Err(error) = spawn {
            self.panorama_operation.store(false, Ordering::Release);
            self.fail_panorama_project(
                project_id,
                format!("starting panorama preview worker: {error}"),
            )?;
            return Err(error).context("starting panorama preview worker");
        }
        Ok(())
    }

    pub(super) async fn start_panorama_render_async(
        &self,
        project_id: u64,
        request: ReviewPanoramaRenderRequest,
    ) -> Result<()> {
        self.ensure_panorama_available()?;
        self.claim_panorama_operation()?;
        let result = self.prepare_panorama_render_job(project_id, request).await;
        let (project, sources, output) = match result {
            Ok(job) => job,
            Err(error) => {
                self.panorama_operation.store(false, Ordering::Release);
                return Err(error);
            }
        };
        let handle = self.clone();
        let spawn = thread::Builder::new()
            .name("mini-film-panorama-render".to_string())
            .spawn(move || handle.run_panorama_render_job(project, sources, output));
        if let Err(error) = spawn {
            self.panorama_operation.store(false, Ordering::Release);
            self.fail_panorama_project(
                project_id,
                format!("starting panorama render worker: {error}"),
            )?;
            return Err(error).context("starting panorama render worker");
        }
        Ok(())
    }

    pub(super) fn panorama_preview_media_path(
        &self,
        project_id: u64,
        matching: PanoramaMatchingMode,
        projection: PanoramaProjection,
    ) -> Result<PathBuf> {
        let project = self.panorama_project(project_id)?;
        let preview = project
            .previews
            .iter()
            .find(|preview| preview.matching_mode == matching && preview.projection == projection)
            .ok_or_else(|| anyhow!("panorama preview is not available"))?;
        if preview.status != ReviewPanoramaPreviewStatus::Done {
            bail!("panorama preview is not ready");
        }
        let path = preview
            .path
            .as_ref()
            .ok_or_else(|| anyhow!("panorama preview has no output path"))?;
        if !path.starts_with(self.panorama_cache_root()) || !path.is_file() {
            bail!("panorama preview is missing: {}", path.display());
        }
        Ok(path.clone())
    }

    fn ensure_panorama_available(&self) -> Result<()> {
        if self.panorama_capability.available {
            Ok(())
        } else {
            bail!(
                "panorama mode is unavailable: {}",
                self.panorama_capability
                    .reason
                    .as_deref()
                    .unwrap_or("Hugin CLI tools were not found")
            )
        }
    }

    fn claim_panorama_operation(&self) -> Result<()> {
        self.panorama_operation
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| anyhow!("another panorama operation is already running"))
    }

    fn panorama_project(&self, project_id: u64) -> Result<ReviewPanoramaProject> {
        self.panorama_projects
            .load()
            .iter()
            .find(|project| project.id == project_id)
            .cloned()
            .ok_or_else(|| anyhow!("panorama project {project_id} does not exist"))
    }

    fn panorama_source_paths(&self, image_ids: &[u64]) -> Result<Vec<PathBuf>> {
        if image_ids.len() < 2 {
            bail!("a panorama requires at least two review images");
        }
        let unique = image_ids.iter().copied().collect::<HashSet<_>>();
        if unique.len() != image_ids.len() {
            bail!("a panorama cannot contain the same review image more than once");
        }
        let store = self.store_snapshot();
        image_ids
            .iter()
            .map(|image_id| {
                let image = store
                    .images
                    .iter()
                    .find(|image| image.id == *image_id)
                    .ok_or_else(|| anyhow!("review image {image_id} does not exist"))?;
                if !image.raw_path.is_file() {
                    bail!("panorama source is missing: {}", image.raw_path.display());
                }
                Ok(image.raw_path.clone())
            })
            .collect()
    }

    async fn prepare_panorama_preview_job(
        &self,
        project_id: u64,
        request: ReviewPanoramaPreviewRequest,
    ) -> Result<(ReviewPanoramaProject, Vec<PathBuf>)> {
        let mut project = self.panorama_project(project_id)?;
        if let Some(image_ids) = request.image_ids {
            project.image_ids = image_ids;
        }
        if let Some(matching_mode) = request.matching_mode {
            project.matching_mode = matching_mode;
        }
        let sources = self.panorama_source_paths(&project.image_ids)?;
        let updated_at = now_string();
        project.status = ReviewPanoramaStatus::Previewing;
        project.selected_projection = None;
        project.output_path = None;
        project.result_image_id = None;
        project.progress_stage = Some("queued".to_string());
        project.progress_completed = 0;
        project.progress_total = 1;
        project.error = None;
        project.updated_at = updated_at.clone();
        project.previews = PanoramaProjection::ALL
            .into_iter()
            .map(|projection| ReviewPanoramaPreview {
                matching_mode: project.matching_mode,
                projection,
                status: ReviewPanoramaPreviewStatus::Queued,
                path: None,
                cache_key: None,
                duration_ms: None,
                error: None,
                updated_at: updated_at.clone(),
            })
            .collect();
        self.save_panorama_project_async(project.clone()).await?;
        self.broadcast_state()?;
        Ok((project, sources))
    }

    async fn prepare_panorama_render_job(
        &self,
        project_id: u64,
        request: ReviewPanoramaRenderRequest,
    ) -> Result<(ReviewPanoramaProject, Vec<PathBuf>, PathBuf)> {
        let mut project = self.panorama_project(project_id)?;
        if let Some(name) = request.name {
            let name = normalize_panorama_name(&name)?;
            if project.name != name {
                project.name = name;
                project.output_path = None;
            }
        }
        if let Some(projection) = request.projection {
            project.selected_projection = Some(projection);
        }
        let projection = project
            .selected_projection
            .ok_or_else(|| anyhow!("select a panorama projection before final rendering"))?;
        if !project.previews.iter().any(|preview| {
            preview.matching_mode == project.matching_mode
                && preview.projection == projection
                && preview.status == ReviewPanoramaPreviewStatus::Done
        }) {
            bail!("render previews before starting the full panorama");
        }
        let sources = self.panorama_source_paths(&project.image_ids)?;
        let projects = self.panorama_projects_snapshot();
        let output = project.output_path.clone().unwrap_or_else(|| {
            unique_panorama_output(
                &self.input_root,
                &project.name,
                project.id,
                projects.as_ref(),
            )
        });
        project.status = ReviewPanoramaStatus::Rendering;
        project.output_path = Some(output.clone());
        project.result_image_id = None;
        project.progress_stage = Some("queued".to_string());
        project.progress_completed = 0;
        project.progress_total = 1;
        project.error = None;
        project.updated_at = now_string();
        self.save_panorama_project_async(project.clone()).await?;
        self.broadcast_state()?;
        Ok((project, sources, output))
    }

    fn run_panorama_preview_job(&self, project: ReviewPanoramaProject, sources: Vec<PathBuf>) {
        let started = Instant::now();
        let project_id = project.id;
        let progress_handle = self.clone();
        let progress: PanoramaProgressSink = Arc::new(move |progress| {
            if let Err(error) = progress_handle.update_panorama_progress(project_id, progress) {
                eprintln!("panorama preview progress update failed: {error:#}");
            }
        });
        let result = render_preview_row(
            &self.panorama_config,
            &sources,
            &self
                .panorama_cache_root()
                .join(format!("project-{project_id}")),
            project.matching_mode,
            Some(progress),
        );
        self.panorama_operation.store(false, Ordering::Release);
        let finish = match result {
            Ok(previews) => self.finish_panorama_previews(project_id, previews, started.elapsed()),
            Err(error) => self.fail_panorama_project(project_id, format!("{error:#}")),
        };
        if let Err(error) = finish {
            eprintln!("panorama preview completion update failed: {error:#}");
        }
    }

    fn run_panorama_render_job(
        &self,
        project: ReviewPanoramaProject,
        sources: Vec<PathBuf>,
        output: PathBuf,
    ) {
        let project_id = project.id;
        let progress_handle = self.clone();
        let progress: PanoramaProgressSink = Arc::new(move |progress| {
            if let Err(error) = progress_handle.update_panorama_progress(project_id, progress) {
                eprintln!("panorama render progress update failed: {error:#}");
            }
        });
        let projection = project
            .selected_projection
            .expect("validated panorama projection");
        let render_result = render_final(
            &self.panorama_config,
            &sources,
            &self
                .panorama_cache_root()
                .join(format!("project-{project_id}")),
            project.matching_mode,
            projection,
            &output,
            false,
            Some(progress),
        );
        let reconcile_result = self.reconcile_replaced_raw_sources(&sources);
        let result = match (render_result, reconcile_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Err(render_error), Err(reconcile_error)) => Err(render_error.context(format!(
                "also failed to update converted panorama sources: {reconcile_error:#}"
            ))),
        }
        .and_then(|()| self.record_profiled_compressed_discovered(&output))
        .and_then(|()| {
            if let Some(sender) = &self.trusted_input_sender {
                sender
                    .send(output.clone())
                    .context("queueing panorama result in daemon")?;
            }
            Ok(())
        })
        .and_then(|()| {
            let image_id = self
                .store_snapshot()
                .images
                .iter()
                .find(|image| image.raw_path == output)
                .map(|image| image.id)
                .ok_or_else(|| anyhow!("panorama result was not added to review"))?;
            Ok(image_id)
        });
        self.panorama_operation.store(false, Ordering::Release);
        let completion = match result {
            Ok(image_id) => self.finish_panorama_render(project_id, image_id),
            Err(error) => self.fail_panorama_project(project_id, format!("{error:#}")),
        };
        if let Err(error) = completion {
            eprintln!("panorama render completion update failed: {error:#}");
        }
    }

    fn reconcile_replaced_raw_sources(&self, sources: &[PathBuf]) -> Result<()> {
        for source in sources {
            if source.is_file() {
                continue;
            }
            if let Some(successor) = self.dng_fallback.existing_successor(source)? {
                self.rebind_and_queue_converted_source(source, successor.active())?;
            }
        }
        Ok(())
    }

    fn update_panorama_progress(&self, project_id: u64, progress: PanoramaProgress) -> Result<()> {
        self.update_panorama_project_sync(project_id, |project| {
            project.progress_stage = Some(progress.stage);
            project.progress_completed = progress.completed;
            project.progress_total = progress.total;
            project.updated_at = now_string();
            Ok(())
        })
    }

    fn finish_panorama_previews(
        &self,
        project_id: u64,
        previews: Vec<PanoramaPreview>,
        duration: Duration,
    ) -> Result<()> {
        self.update_panorama_project_sync(project_id, |project| {
            let duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
            let updated_at = now_string();
            project.previews = previews
                .into_iter()
                .map(|preview| ReviewPanoramaPreview {
                    matching_mode: project.matching_mode,
                    projection: preview.projection,
                    status: ReviewPanoramaPreviewStatus::Done,
                    cache_key: Some(short_path_sha1(&preview.path)),
                    path: Some(preview.path),
                    duration_ms: Some(duration_ms),
                    error: None,
                    updated_at: updated_at.clone(),
                })
                .collect();
            project.status = ReviewPanoramaStatus::Ready;
            project.selected_projection = Some(PanoramaProjection::Cylindrical);
            project.progress_stage = Some("complete".to_string());
            project.progress_completed = PanoramaProjection::ALL.len();
            project.progress_total = PanoramaProjection::ALL.len();
            project.error = None;
            project.updated_at = updated_at;
            Ok(())
        })
    }

    fn finish_panorama_render(&self, project_id: u64, result_image_id: u64) -> Result<()> {
        self.update_panorama_project_sync(project_id, |project| {
            project.status = ReviewPanoramaStatus::Complete;
            project.result_image_id = Some(result_image_id);
            project.progress_stage = Some("complete".to_string());
            project.progress_completed = 1;
            project.progress_total = 1;
            project.error = None;
            project.updated_at = now_string();
            Ok(())
        })
    }

    fn fail_panorama_project(&self, project_id: u64, error: String) -> Result<()> {
        self.update_panorama_project_sync(project_id, |project| {
            project.status = ReviewPanoramaStatus::Failed;
            project.progress_stage = Some("failed".to_string());
            project.error = Some(error.clone());
            let updated_at = now_string();
            for preview in &mut project.previews {
                if matches!(
                    preview.status,
                    ReviewPanoramaPreviewStatus::Queued | ReviewPanoramaPreviewStatus::Processing
                ) {
                    preview.status = ReviewPanoramaPreviewStatus::Failed;
                    preview.error = Some(error.clone());
                    preview.updated_at = updated_at.clone();
                }
            }
            project.updated_at = updated_at;
            Ok(())
        })
    }

    fn update_panorama_project_sync<F>(&self, project_id: u64, update: F) -> Result<()>
    where
        F: FnOnce(&mut ReviewPanoramaProject) -> Result<()>,
    {
        self.database_runtime.block_on(async {
            let mut project = self.panorama_project(project_id)?;
            update(&mut project)?;
            self.save_panorama_project_async(project).await?;
            self.broadcast_state()
        })
    }

    async fn save_panorama_project_async(&self, project: ReviewPanoramaProject) -> Result<()> {
        let _write_guard = self.database.write_lock.lock().await;
        self.database.save_panorama_project(&project).await?;
        self.cache_panorama_project(project);
        Ok(())
    }

    fn cache_panorama_project(&self, project: ReviewPanoramaProject) {
        loop {
            let current = self.panorama_projects.load_full();
            let mut next = (*current).clone();
            if let Some(existing) = next.iter_mut().find(|existing| existing.id == project.id) {
                *existing = project.clone();
            } else {
                next.push(project.clone());
            }
            next.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
            let next = Arc::new(next);
            let previous = self
                .panorama_projects
                .compare_and_swap(&current, Arc::clone(&next));
            if Arc::ptr_eq(&previous, &current) {
                break;
            }
        }
    }

    pub(super) fn preview_path_for(&self, raw: &Path, image_id: u64) -> PathBuf {
        self.preview_root()
            .join(format!("{image_id:08}-{}.jpg", short_path_sha1(raw)))
    }

    pub(super) fn compressed_thumbnail_path_for(&self, source: &Path, image_id: u64) -> PathBuf {
        self.preview_root()
            .join(COMPRESSED_REVIEW_CACHE_VERSION)
            .join("thumbnails")
            .join(compressed_review_cache_file_name(source, image_id))
    }

    pub(super) fn compressed_display_preview_path_for(
        &self,
        source: &Path,
        image_id: u64,
    ) -> PathBuf {
        self.preview_root()
            .join(COMPRESSED_REVIEW_CACHE_VERSION)
            .join("previews")
            .join(compressed_review_cache_file_name(source, image_id))
    }

    pub(super) fn crop_source_preview_path_for(&self, source: &Path, image_id: u64) -> PathBuf {
        self.preview_root()
            .join(CROP_SOURCE_CACHE_VERSION)
            .join(compressed_review_cache_file_name(source, image_id))
    }

    pub(super) fn rendered_full_preview_path_for(&self, source: &Path, image_id: u64) -> PathBuf {
        self.preview_root()
            .join(RENDERED_FULL_PREVIEW_CACHE_VERSION)
            .join(compressed_review_cache_file_name(source, image_id))
    }

    #[cfg(test)]
    pub(crate) fn record_discovered_raw(&self, raw: &Path) -> Result<()> {
        self.record_discovered_raw_with_sidecar(raw, None)
    }

    pub(crate) fn record_discovered_raw_with_sidecar(
        &self,
        raw: &Path,
        sooc_sidecar: Option<&Path>,
    ) -> Result<()> {
        let (history_entry, preview_job, crop_source_job) = self.update_store(|store| {
            let mut preview_job = None;
            let mut crop_source_job = None;
            let mut history_entry = None;
            let discovered = !store.images.iter().any(|image| image.raw_path == raw);
            let profiles = store.profiles.clone();
            {
                let image = store.ensure_image(&self.input_root, raw)?;
                let old_sidecar = image.sooc_sidecar_path.clone();
                image.sooc_sidecar_path = sooc_sidecar.map(Path::to_path_buf);
                if let Some(sidecar) = &image.sooc_sidecar_path {
                    let crop_source = self.crop_source_preview_path_for(sidecar, image.id);
                    if !crop_source.is_file() {
                        crop_source_job = Some((image.id, sidecar.clone(), crop_source));
                    }
                }
                if image.sooc_sidecar_path != old_sidecar {
                    sync_image_profile_renders(
                        image,
                        &profiles,
                        false,
                        &HashSet::new(),
                        self.normalize_grain_mpix,
                        &self.export,
                    );
                }
                let preview_path = self.preview_path_for(raw, image.id);
                let mut preview_queued = false;
                if !matches!(
                    image.preview.status,
                    ReviewRenderStatus::Queued | ReviewRenderStatus::Processing
                ) && preview_path.is_file()
                {
                    image.preview.status = ReviewRenderStatus::Done;
                    image.preview.path = Some(preview_path.clone());
                    image.preview.error = None;
                    image.preview.updated_at = now_string();
                } else if !preview_path.is_file()
                    && !matches!(
                        image.preview.status,
                        ReviewRenderStatus::Queued | ReviewRenderStatus::Processing
                    )
                {
                    image.preview.status = ReviewRenderStatus::Queued;
                    image.preview.path = Some(preview_path.clone());
                    image.preview.error = None;
                    image.preview.updated_at = now_string();
                    image.updated_at = now_string();
                    preview_job = Some((raw.to_path_buf(), preview_path));
                    preview_queued = true;
                }
                if discovered || preview_queued || image.sooc_sidecar_path != old_sidecar {
                    history_entry =
                        Some(history_image_discovered(image, discovered, preview_queued));
                }
            }
            store.merge_standalone_sooc_sidecars();
            Ok((history_entry, preview_job, crop_source_job))
        })?;
        if let Some(entry) = history_entry {
            self.append_history(entry)?;
        }
        self.broadcast_state()?;
        if let Some((raw, preview_path)) = preview_job {
            self.spawn_preview_job(raw, preview_path);
        }
        if let Some((image_id, source, output)) = crop_source_job {
            self.spawn_crop_source_preview_job(image_id, source, output);
        }
        self.schedule_sampler_profiles_for_source(raw)?;
        Ok(())
    }

    fn spawn_crop_source_preview_job(&self, image_id: u64, source: PathBuf, output: PathBuf) {
        let handle = self.clone();
        let _ = thread::Builder::new()
            .name("mini-film-review-crop-source".to_string())
            .spawn(move || {
                match ensure_compressed_review_preview(&source, &output, &handle.convert) {
                    Ok(()) => {
                        if let Err(error) = handle.broadcast_state() {
                            eprintln!("review crop source state update failed: {error:#}");
                        }
                    }
                    Err(error) => {
                        eprintln!(
                            "review crop source generation failed for image {image_id} ({}): {error:#}",
                            source.display()
                        );
                    }
                }
            });
    }

    pub(crate) fn record_compressed_queued(
        &self,
        input: &Path,
        expected_output: &Path,
    ) -> Result<()> {
        let history_entries = self.update_store(|store| {
            let mut history_entries = Vec::new();
            if store.claim_sooc_sidecar(input) {
                return Ok(history_entries);
            }
            let discovered = !store.images.iter().any(|image| image.raw_path == input);
            let image = store.ensure_image(&self.input_root, input)?;
            let before = image.preview.clone();
            image.preview.status = ReviewRenderStatus::Queued;
            image.preview.path = Some(expected_output.to_path_buf());
            image.preview.error = None;
            image.preview.duration_ms = None;
            image.preview.render_key = retouch_render_key(&image.retouch, &self.export);
            image.preview.updated_at = now_string();
            image.updated_at = now_string();
            if discovered {
                history_entries.push(history_image_discovered(image, true, false));
            }
            if let Some(entry) = history_preview_changed(image, &before, &image.preview) {
                history_entries.push(entry);
            }
            Ok(history_entries)
        })?;
        self.schedule_compressed_review_media(input);
        for entry in history_entries {
            self.append_history(entry)?;
        }
        self.broadcast_state()?;
        self.schedule_sampler_profiles_for_source(input)
    }

    pub(crate) fn record_profiled_compressed_discovered(&self, input: &Path) -> Result<()> {
        let history_entries = self.update_store(|store| {
            let mut history_entries = Vec::new();
            if store.claim_sooc_sidecar(input) {
                return Ok(history_entries);
            }
            let discovered = !store.images.iter().any(|image| image.raw_path == input);
            let image = store.ensure_image(&self.input_root, input)?;
            let preview_path = self.compressed_display_preview_path_for(input, image.id);
            let before = image.preview.clone();
            image.preview.path = Some(preview_path.clone());
            image.preview.render_key = None;
            image.preview.error = None;
            image.preview.status = if preview_path.is_file() {
                ReviewRenderStatus::Done
            } else {
                ReviewRenderStatus::Queued
            };
            image.preview.updated_at = now_string();
            image.updated_at = now_string();
            if discovered {
                history_entries.push(history_image_discovered(
                    image,
                    true,
                    !preview_path.is_file(),
                ));
            }
            if let Some(entry) = history_preview_changed(image, &before, &image.preview) {
                history_entries.push(entry);
            }
            Ok(history_entries)
        })?;
        self.schedule_compressed_review_media(input);
        for entry in history_entries {
            self.append_history(entry)?;
        }
        self.broadcast_state()?;
        self.schedule_sampler_profiles_for_source(input)
    }

    pub(crate) fn record_compressed_processing(&self, input: &Path) -> Result<()> {
        self.update_preview(input, |preview| {
            preview.status = ReviewRenderStatus::Processing;
            preview.error = None;
        })
    }

    pub(crate) fn record_compressed_done(
        &self,
        input: &Path,
        output: &Path,
        duration: Duration,
    ) -> Result<()> {
        let mut pending_retouch_key = None;
        let result = self.update_preview(input, |preview| {
            pending_retouch_key = apply_base_preview_done(preview, output, duration);
            if let Some(render_key) = pending_retouch_key.as_deref()
                && apply_cached_preview_output(
                    preview,
                    output,
                    render_key,
                    &self.output_root,
                    &self.cache_root,
                )
            {
                pending_retouch_key = None;
            }
        });
        if result.is_ok()
            && let Some(render_key) = pending_retouch_key
            && let Some(image_id) = self.review_image_id_for(input)
        {
            self.schedule_retouch_job(
                image_id,
                input.to_path_buf(),
                None,
                output.to_path_buf(),
                render_key,
            );
        }
        if result.is_ok() {
            self.schedule_compressed_review_media(input);
            self.maybe_schedule_codex_for_raw(input)?;
        }
        result
    }

    pub(crate) fn record_compressed_failed(
        &self,
        input: &Path,
        output: Option<&Path>,
        duration: Duration,
        error: &str,
    ) -> Result<()> {
        let result = self.update_preview(input, |preview| {
            if preview.render_key.is_some()
                && matches!(
                    preview.status,
                    ReviewRenderStatus::Queued | ReviewRenderStatus::Processing
                )
            {
                return;
            }
            preview.status = ReviewRenderStatus::Failed;
            if let Some(output) = output {
                preview.path = Some(output.to_path_buf());
            }
            preview.error = Some(error.to_string());
            preview.duration_ms = Some(duration.as_millis() as u64);
        });
        if result.is_ok() {
            self.maybe_schedule_codex_for_raw(input)?;
        }
        result
    }

    fn schedule_compressed_review_media(&self, input: &Path) {
        let store = self.store_snapshot();
        let Some(image) = store.images.iter().find(|image| image.raw_path == input) else {
            return;
        };
        if !is_rendered_input_file(&image.raw_path) {
            return;
        }
        self.media_scheduler.schedule(
            input.to_path_buf(),
            image.id,
            image.exif.capture_timestamp,
            image.relative_path.clone(),
        );
    }

    fn start_media_scheduler(&self) -> Result<()> {
        for (kind, prefix, workers) in [
            (
                ReviewMediaKind::Thumbnail,
                "mini-film-review-thumbnail",
                REVIEW_THUMBNAIL_WORKERS,
            ),
            (
                ReviewMediaKind::Preview,
                "mini-film-review-preview",
                REVIEW_PREVIEW_WORKERS,
            ),
        ] {
            for worker in 1..=workers {
                let handle = self.clone();
                let name = format!("{prefix}-{worker}");
                thread::Builder::new()
                    .name(name.clone())
                    .spawn(move || {
                        loop {
                            let job = handle.media_scheduler.next_job(kind);
                            handle.run_scheduled_media_job(kind, job);
                        }
                    })
                    .with_context(|| format!("starting {name} scheduler thread"))?;
            }
        }
        Ok(())
    }

    fn run_scheduled_media_job(&self, kind: ReviewMediaKind, job: ScheduledReviewMediaJob) {
        let store = self.store_snapshot();
        let current = store
            .images
            .iter()
            .find(|image| image.id == job.image_id && image.raw_path == job.raw)
            .map(image_uses_profile_pipeline);
        drop(store);
        let Some(profiled) = current else {
            self.media_scheduler.finish(kind, &job.raw);
            return;
        };

        let result = match kind {
            ReviewMediaKind::Thumbnail => ensure_compressed_review_thumbnail(
                &job.raw,
                &self.compressed_thumbnail_path_for(&job.raw, job.image_id),
                &self.convert,
            ),
            ReviewMediaKind::Preview => ensure_compressed_review_preview(
                &job.raw,
                &self.compressed_display_preview_path_for(&job.raw, job.image_id),
                &self.convert,
            ),
        };
        self.media_scheduler.finish(kind, &job.raw);
        match result {
            Ok(()) => {
                let update = if kind == ReviewMediaKind::Preview && profiled {
                    self.record_preview_done(
                        &job.raw,
                        &self.compressed_display_preview_path_for(&job.raw, job.image_id),
                    )
                } else {
                    self.broadcast_state()
                };
                if let Err(error) = update {
                    eprintln!("review media state update failed: {error:#}");
                }
            }
            Err(error) => {
                if kind == ReviewMediaKind::Preview && profiled {
                    let _ = self.record_preview_failed(&job.raw, &format!("{error:#}"));
                }
                eprintln!(
                    "review {} generation failed for {}: {error:#}",
                    match kind {
                        ReviewMediaKind::Thumbnail => "thumbnail",
                        ReviewMediaKind::Preview => "preview",
                    },
                    job.raw.display()
                );
            }
        }
    }

    pub(super) fn spawn_preview_job(&self, raw: PathBuf, output: PathBuf) {
        let handle = self.clone();
        let _ = thread::Builder::new()
            .name("mini-film-review-preview".to_string())
            .spawn(move || {
                let start = std::time::Instant::now();
                if let Err(error) = handle.record_preview_processing(&raw) {
                    eprintln!("review preview state update failed: {error:#}");
                }
                let result = extract_embedded_preview(&raw, &output, &handle.convert);
                match result {
                    Ok(()) => {
                        if let Err(error) = handle.record_preview_done(&raw, &output) {
                            eprintln!("review preview state update failed: {error:#}");
                        }
                    }
                    Err(error) => {
                        let message = format!("{error:#}");
                        if let Err(error) = handle.record_preview_failed(&raw, &message) {
                            eprintln!("review preview state update failed: {error:#}");
                        }
                    }
                }
                let _ = start;
            });
    }

    pub(super) fn record_preview_processing(&self, raw: &Path) -> Result<()> {
        self.update_preview(raw, |preview| {
            preview.status = ReviewRenderStatus::Processing;
            preview.error = None;
        })
    }

    pub(super) fn record_preview_done(&self, raw: &Path, output: &Path) -> Result<()> {
        let result = self.update_preview(raw, |preview| {
            preview.status = ReviewRenderStatus::Done;
            preview.path = Some(output.to_path_buf());
            preview.error = None;
        });
        if result.is_ok() {
            self.maybe_schedule_codex_for_raw(raw)?;
        }
        result
    }

    pub(super) fn record_preview_failed(&self, raw: &Path, error: &str) -> Result<()> {
        self.update_preview(raw, |preview| {
            preview.status = ReviewRenderStatus::Failed;
            preview.error = Some(error.to_string());
        })
    }

    pub(super) fn update_preview<F>(&self, raw: &Path, mut update: F) -> Result<()>
    where
        F: FnMut(&mut ReviewPreview),
    {
        let history_entry = self.update_store(|store| {
            if store.claim_sooc_sidecar(raw) {
                return Ok(None);
            }
            let image = store.ensure_image(&self.input_root, raw)?;
            let before = image.preview.clone();
            update(&mut image.preview);
            image.preview.updated_at = now_string();
            image.updated_at = now_string();
            Ok(history_preview_changed(image, &before, &image.preview))
        })?;
        if let Some(entry) = history_entry {
            self.append_history(entry)?;
        }
        self.broadcast_state()
    }

    pub(super) fn update_preview_if_key<F>(
        &self,
        image_id: u64,
        render_key: &str,
        mut update: F,
    ) -> Result<bool>
    where
        F: FnMut(&mut ReviewPreview),
    {
        let (updated, history_entry) = self.update_store(|store| {
            let mut updated = false;
            let mut history_entry = None;
            let Some(image) = store.images.iter_mut().find(|image| image.id == image_id) else {
                return Ok((false, None));
            };
            if image.preview.render_key.as_deref() == Some(render_key) {
                let before = image.preview.clone();
                update(&mut image.preview);
                image.preview.updated_at = now_string();
                image.updated_at = now_string();
                history_entry = history_preview_changed(image, &before, &image.preview);
                updated = true;
            }
            Ok((updated, history_entry))
        })?;
        if updated {
            if let Some(entry) = history_entry {
                self.append_history(entry)?;
            }
            self.broadcast_state()?;
        }
        Ok(updated)
    }

    pub(crate) fn record_profile_queued(
        &self,
        raw: &Path,
        profile_index: usize,
        expected_output: &Path,
    ) -> Result<bool> {
        let (queued, history_entry) = self.update_store(|store| {
            if store.claim_sooc_sidecar(raw) {
                return Ok((false, None));
            }
            let profile = store
                .profiles
                .iter()
                .find(|profile| profile.index == profile_index)
                .cloned();
            let image = store.ensure_image(&self.input_root, raw)?;
            let processing_key = review_render_processing_key_for_input_with_options(
                raw,
                profile_index,
                self.normalize_grain_mpix,
                &self.export,
            );
            let bw_filter = profile
                .as_ref()
                .map(|profile| effective_bw_filter_for_profile(image, profile))
                .unwrap_or_default();
            let white_balance = retouch_white_balance_for_image(image);
            let render_key = profile_render_key(
                &image.retouch,
                white_balance,
                bw_filter,
                (profile_index != SOOC_PROFILE_INDEX)
                    .then_some(self.normalize_grain_mpix)
                    .flatten(),
                &processing_key,
            );
            let Some(render) = image
                .profiles
                .iter_mut()
                .find(|render| render.profile_index == profile_index)
            else {
                bail!("review profile index {profile_index} is not configured");
            };
            if profile_index != SOOC_PROFILE_INDEX && !render.enabled {
                return Ok((false, None));
            }
            let before = render.clone();
            render.status = ReviewRenderStatus::Queued;
            render.output_path = Some(expected_output.to_path_buf());
            render.error = None;
            render.duration_ms = None;
            render.render_key = render_key;
            render.processing_key = Some(processing_key);
            render.width = None;
            render.height = None;
            render.updated_at = now_string();
            let after = render.clone();
            image.updated_at = now_string();
            Ok((true, history_render_changed(image, &before, &after)))
        })?;
        if let Some(entry) = history_entry {
            self.append_history(entry)?;
        }
        self.broadcast_state()?;
        Ok(queued)
    }

    pub(crate) fn profile_render_current(
        &self,
        raw: &Path,
        profile_index: usize,
        expected_output: &Path,
    ) -> bool {
        if !expected_output.is_file() {
            return false;
        }
        let store = self.store_snapshot();
        let Some(image) = store.images.iter().find(|image| image.raw_path == raw) else {
            return false;
        };
        let processing_key = review_render_processing_key_for_input_with_options(
            raw,
            profile_index,
            self.normalize_grain_mpix,
            &self.export,
        );
        image.profiles.iter().any(|render| {
            let Some(output) = render.output_path.as_deref() else {
                return false;
            };
            render.profile_index == profile_index
                && render.processing_key.as_deref() == Some(processing_key.as_str())
                && output.is_file()
                && retouch_base_output(output, &self.output_root, &self.cache_root)
                    == expected_output
                && matches!(render.status, ReviewRenderStatus::Done)
                && render.render_key.is_none()
        })
    }

    pub(crate) fn record_profile_processing(&self, raw: &Path, profile_index: usize) -> Result<()> {
        self.update_render(raw, profile_index, |render| {
            if render.render_key.is_some() {
                render.error = None;
                return;
            }
            render.status = ReviewRenderStatus::Processing;
            render.error = None;
        })
        .map(|_| ())
    }

    pub(crate) fn record_profile_processing_for_image(
        &self,
        image_id: u64,
        profile_index: usize,
    ) -> Result<()> {
        self.update_render_for_image(image_id, profile_index, |render| {
            if render.render_key.is_some() {
                render.error = None;
                return;
            }
            render.status = ReviewRenderStatus::Processing;
            render.error = None;
        })
    }

    #[cfg(test)]
    pub(crate) fn record_profile_done(
        &self,
        raw: &Path,
        profile_index: usize,
        output: &Path,
        duration: Duration,
    ) -> Result<()> {
        self.record_profile_done_with_dcp(raw, profile_index, output, duration, None)
    }

    pub(crate) fn record_profile_done_with_dcp(
        &self,
        raw: &Path,
        profile_index: usize,
        output: &Path,
        duration: Duration,
        dcp_profile_filename: Option<&str>,
    ) -> Result<()> {
        let mut pending_retouch_key = None;
        let result = self.update_render(raw, profile_index, |render| {
            render.dcp_profile_filename = dcp_profile_filename.map(str::to_string);
            pending_retouch_key = apply_base_render_done(render, output, duration);
            if let Some(render_key) = pending_retouch_key.as_deref()
                && apply_cached_profile_output(
                    render,
                    output,
                    render_key,
                    false,
                    &self.output_root,
                    &self.cache_root,
                )
            {
                pending_retouch_key = None;
            }
        });
        if let Ok(image_id) = result.as_ref()
            && let Some(render_key) = pending_retouch_key
        {
            self.schedule_retouch_job(
                *image_id,
                raw.to_path_buf(),
                Some(profile_index),
                output.to_path_buf(),
                render_key,
            );
        }
        if result.is_ok() {
            self.maybe_schedule_codex_for_raw(raw)?;
        }
        result.map(|_| ())
    }

    pub(crate) fn record_profile_done_with_dcp_for_image(
        &self,
        image_id: u64,
        profile_index: usize,
        output: &Path,
        duration: Duration,
        dcp_profile_filename: Option<&str>,
    ) -> Result<()> {
        let mut pending_retouch_key = None;
        self.update_render_for_image(image_id, profile_index, |render| {
            render.dcp_profile_filename = dcp_profile_filename.map(str::to_string);
            pending_retouch_key = apply_base_render_done(render, output, duration);
            if let Some(render_key) = pending_retouch_key.as_deref()
                && apply_cached_profile_output(
                    render,
                    output,
                    render_key,
                    false,
                    &self.output_root,
                    &self.cache_root,
                )
            {
                pending_retouch_key = None;
            }
        })?;
        let current_raw = self.review_raw_for_image_id(image_id);
        if let Some(render_key) = pending_retouch_key
            && let Some(raw) = current_raw.clone()
        {
            self.schedule_retouch_job(
                image_id,
                raw,
                Some(profile_index),
                output.to_path_buf(),
                render_key,
            );
        }
        if let Some(raw) = current_raw {
            self.maybe_schedule_codex_for_raw(&raw)?;
        }
        Ok(())
    }

    pub(crate) fn record_profile_failed(
        &self,
        raw: &Path,
        profile_index: usize,
        output: Option<&Path>,
        duration: Duration,
        error: &str,
    ) -> Result<()> {
        let result = self.update_render(raw, profile_index, |render| {
            if render.render_key.is_some()
                && matches!(
                    render.status,
                    ReviewRenderStatus::Queued | ReviewRenderStatus::Processing
                )
            {
                return;
            }
            render.status = ReviewRenderStatus::Failed;
            if let Some(output) = output {
                render.output_path = Some(output.to_path_buf());
            }
            render.error = Some(error.to_string());
            render.duration_ms = Some(duration.as_millis() as u64);
            render.dcp_profile_filename = None;
        });
        if result.is_ok() {
            self.maybe_schedule_codex_for_raw(raw)?;
        }
        result.map(|_| ())
    }

    pub(crate) fn record_profile_failed_for_image(
        &self,
        image_id: u64,
        profile_index: usize,
        output: Option<&Path>,
        duration: Duration,
        error: &str,
    ) -> Result<()> {
        self.update_render_for_image(image_id, profile_index, |render| {
            if render.render_key.is_some()
                && matches!(
                    render.status,
                    ReviewRenderStatus::Queued | ReviewRenderStatus::Processing
                )
            {
                return;
            }
            render.status = ReviewRenderStatus::Failed;
            if let Some(output) = output {
                render.output_path = Some(output.to_path_buf());
            }
            render.error = Some(error.to_string());
            render.duration_ms = Some(duration.as_millis() as u64);
            render.dcp_profile_filename = None;
        })?;
        if let Some(raw) = self.review_raw_for_image_id(image_id) {
            self.maybe_schedule_codex_for_raw(&raw)?;
        }
        Ok(())
    }

    pub(super) fn update_render<F>(
        &self,
        raw: &Path,
        profile_index: usize,
        mut update: F,
    ) -> Result<u64>
    where
        F: FnMut(&mut ReviewProfileRender),
    {
        let (image_id, history_entry) = self.update_store(|store| {
            if store.claim_sooc_sidecar(raw) {
                let image_id = store
                    .images
                    .iter()
                    .find(|image| image.sooc_sidecar_path.as_deref() == Some(raw))
                    .map(|image| image.id)
                    .ok_or_else(|| anyhow!("review image for {} does not exist", raw.display()))?;
                return Ok((image_id, None));
            }
            let image = store.ensure_image(&self.input_root, raw)?;
            let image_id = image.id;
            let Some(render) = image
                .profiles
                .iter_mut()
                .find(|render| render.profile_index == profile_index)
            else {
                bail!("review profile index {profile_index} is not configured");
            };
            let before = render.clone();
            update(render);
            render.updated_at = now_string();
            let after = render.clone();
            image.updated_at = now_string();
            Ok((image_id, history_render_changed(image, &before, &after)))
        })?;
        if let Some(entry) = history_entry {
            self.append_history(entry)?;
        }
        self.broadcast_state()?;
        Ok(image_id)
    }

    fn update_render_for_image<F>(
        &self,
        image_id: u64,
        profile_index: usize,
        mut update: F,
    ) -> Result<()>
    where
        F: FnMut(&mut ReviewProfileRender),
    {
        let history_entry = self.update_store(|store| {
            let image = store
                .images
                .iter_mut()
                .find(|image| image.id == image_id)
                .ok_or_else(|| anyhow!("review image {image_id} does not exist"))?;
            let Some(render) = image
                .profiles
                .iter_mut()
                .find(|render| render.profile_index == profile_index)
            else {
                bail!("review profile index {profile_index} is not configured");
            };
            let before = render.clone();
            update(render);
            render.updated_at = now_string();
            let after = render.clone();
            image.updated_at = now_string();
            Ok(history_render_changed(image, &before, &after))
        })?;
        if let Some(entry) = history_entry {
            self.append_history(entry)?;
        }
        self.broadcast_state()
    }

    pub(super) fn update_render_if_key<F>(
        &self,
        image_id: u64,
        profile_index: usize,
        render_key: &str,
        mut update: F,
    ) -> Result<bool>
    where
        F: FnMut(&mut ReviewProfileRender),
    {
        let (updated, history_entry) = self.update_store(|store| {
            let mut updated = false;
            let mut history_entry = None;
            let Some(image) = store.images.iter_mut().find(|image| image.id == image_id) else {
                return Ok((false, None));
            };
            let Some(render) = image
                .profiles
                .iter_mut()
                .find(|render| render.profile_index == profile_index)
            else {
                bail!("review profile index {profile_index} is not configured");
            };
            if render.render_key.as_deref() == Some(render_key) {
                let before = render.clone();
                update(render);
                render.updated_at = now_string();
                let after = render.clone();
                image.updated_at = now_string();
                history_entry = history_render_changed(image, &before, &after);
                updated = true;
            }
            Ok((updated, history_entry))
        })?;
        if updated {
            if let Some(entry) = history_entry {
                self.append_history(entry)?;
            }
            self.broadcast_state()?;
        }
        Ok(updated)
    }

    pub(super) fn retouch_task_snapshot(
        &self,
        image_id: u64,
        profile_index: usize,
        render_key: &str,
    ) -> Result<Option<ReviewProfileRetouchTask>> {
        let store = self.store_snapshot();
        let Some(image) = store.images.iter().find(|image| image.id == image_id) else {
            return Ok(None);
        };
        let Some(render) = image
            .profiles
            .iter()
            .find(|render| render.profile_index == profile_index)
        else {
            return Ok(None);
        };
        if !render.enabled || render.render_key.as_deref() != Some(render_key) {
            return Ok(None);
        }
        let Some(profile) = store
            .profiles
            .iter()
            .find(|profile| profile.index == profile_index)
            .cloned()
        else {
            return Ok(None);
        };
        let bw_filter = effective_bw_filter_for_profile(image, &profile);
        let white_balance = retouch_white_balance_for_image(image);
        Ok(Some(ReviewProfileRetouchTask {
            raw: image.raw_path.clone(),
            profile,
            retouch: image.retouch.clone(),
            white_balance,
            bw_filter,
        }))
    }

    pub(super) fn sooc_retouch_task_snapshot(
        &self,
        image_id: u64,
        render_key: &str,
    ) -> Result<Option<(PathBuf, PathBuf, RetouchSettings)>> {
        let store = self.store_snapshot();
        let Some(image) = store.images.iter().find(|image| image.id == image_id) else {
            return Ok(None);
        };
        let Some(render) = image
            .profiles
            .iter()
            .find(|render| render.profile_index == SOOC_PROFILE_INDEX)
        else {
            return Ok(None);
        };
        if render.render_key.as_deref() != Some(render_key) {
            return Ok(None);
        }
        let Some(sidecar) = image_sooc_source(image)
            .map(Path::to_path_buf)
            .filter(|path| path.is_file())
        else {
            return Ok(None);
        };
        Ok(Some((
            image.raw_path.clone(),
            sidecar,
            retouch_without_adjustments(&image.retouch),
        )))
    }

    pub(super) fn compressed_retouch_task_snapshot(
        &self,
        image_id: u64,
        render_key: &str,
    ) -> Result<Option<(PathBuf, RetouchSettings)>> {
        let store = self.store_snapshot();
        let Some(image) = store.images.iter().find(|image| image.id == image_id) else {
            return Ok(None);
        };
        if !image_is_direct_compressed(image) {
            return Ok(None);
        }
        if image.preview.render_key.as_deref() != Some(render_key) {
            return Ok(None);
        }
        Ok(Some((image.raw_path.clone(), image.retouch.clone())))
    }

    pub(super) fn schedule_retouch_job(
        &self,
        image_id: u64,
        raw: PathBuf,
        profile_index: Option<usize>,
        output: PathBuf,
        render_key: String,
    ) {
        self.retouch_scheduler.schedule(ReviewRetouchRequest {
            image_id,
            raw,
            profile_index,
            output,
            render_key,
        });
    }

    pub(super) fn start_retouch_scheduler(&self) -> Result<()> {
        let handle = self.clone();
        thread::Builder::new()
            .name("mini-film-review-retouch".to_string())
            .spawn(move || {
                loop {
                    let job = handle
                        .retouch_scheduler
                        .next_job(|| handle.render_priority_snapshot());
                    handle.run_scheduled_retouch_job(job);
                }
            })
            .context("starting review retouch scheduler thread")?;
        Ok(())
    }

    pub(super) fn start_codex_scheduler(&self) -> Result<()> {
        if self.codex.is_none() {
            return Ok(());
        }
        for worker in 1..=REVIEW_CODEX_WORKERS {
            let handle = self.clone();
            thread::Builder::new()
                .name(format!("mini-film-review-codex-{worker}"))
                .spawn(move || {
                    loop {
                        let job = handle.codex_scheduler.next_job();
                        handle.run_scheduled_codex_job(job);
                    }
                })
                .with_context(|| format!("starting review Codex scheduler worker {worker}"))?;
        }
        Ok(())
    }

    pub(super) fn schedule_ready_codex_jobs(&self) -> Result<()> {
        let Some(config) = &self.codex else {
            return Ok(());
        };
        let raws = {
            let store = self.store_snapshot();
            store
                .images
                .iter()
                .filter_map(|image| {
                    codex_analysis_key_for_image(image, config)
                        .map(|key| (image.raw_path.clone(), key))
                })
                .collect::<Vec<_>>()
        };
        for (raw, key) in raws {
            self.queue_codex_job(raw, key)?;
        }
        Ok(())
    }

    pub(super) fn maybe_schedule_codex_for_raw(&self, raw: &Path) -> Result<()> {
        let Some(config) = &self.codex else {
            return Ok(());
        };
        let key = {
            let store = self.store_snapshot();
            let Some(image) = store.images.iter().find(|image| image.raw_path == raw) else {
                return Ok(());
            };
            codex_analysis_key_for_image_with_config(image, config)
        };
        if let Some(key) = key {
            self.queue_codex_job(raw.to_path_buf(), key)?;
        }
        Ok(())
    }

    fn queue_codex_job(&self, raw: PathBuf, analysis_key: String) -> Result<()> {
        let Some(config) = &self.codex else {
            return Ok(());
        };
        let (history_entry, should_schedule) = self.update_store(|store| {
            let mut history_entry = None;
            let mut should_schedule = false;
            if let Some(image) = store.images.iter_mut().find(|image| image.raw_path == raw) {
                if image.codex.status == ReviewCodexStatus::Done {
                    return Ok((None, false));
                }
                if image.codex.analysis_key.as_deref() == Some(&analysis_key)
                    && matches!(
                        image.codex.status,
                        ReviewCodexStatus::Queued | ReviewCodexStatus::Processing
                    )
                {
                    return Ok((None, false));
                }
                let before = image.codex.clone();
                image.codex.status = ReviewCodexStatus::Queued;
                image.codex.flags = config.flags;
                image.codex.model = config.model.clone();
                image.codex.analysis_key = Some(analysis_key.clone());
                image.codex.error = None;
                image.codex.updated_at = now_string();
                image.updated_at = now_string();
                history_entry = history_codex_changed(image, &before, &image.codex);
                should_schedule = true;
            }
            Ok((history_entry, should_schedule))
        })?;
        if let Some(entry) = history_entry {
            self.append_history(entry)?;
        }
        if should_schedule {
            self.broadcast_state()?;
            self.codex_scheduler.schedule(raw, analysis_key);
        }
        Ok(())
    }

    pub(super) fn run_scheduled_codex_job(&self, job: ScheduledCodexJob) {
        let Some(config) = self.codex.clone() else {
            return;
        };
        let snapshot = self.codex_task_snapshot(&job.raw, &job.analysis_key);
        let Ok(Some((preview, options))) = snapshot else {
            return;
        };
        if self
            .record_codex_processing(&job.raw, &job.analysis_key)
            .is_err()
        {
            return;
        }
        let result = run_codex_image_analysis(&preview, &options);
        match result {
            Ok(result) => {
                let _ = self.record_codex_done(&job.raw, &job.analysis_key, result, &config);
            }
            Err(error) => {
                let _ =
                    self.record_codex_failed(&job.raw, &job.analysis_key, &format!("{error:#}"));
            }
        }
    }

    fn codex_task_snapshot(
        &self,
        raw: &Path,
        analysis_key: &str,
    ) -> Result<Option<(PathBuf, CodexAnalysisOptions)>> {
        let Some(config) = &self.codex else {
            return Ok(None);
        };
        let store = self.store_snapshot();
        let Some(image) = store.images.iter().find(|image| image.raw_path == raw) else {
            return Ok(None);
        };
        if image.codex.analysis_key.as_deref() != Some(analysis_key) {
            return Ok(None);
        }
        if image.codex.status != ReviewCodexStatus::Queued {
            return Ok(None);
        }
        if image.preview.status != ReviewRenderStatus::Done {
            return Ok(None);
        }
        let Some(preview) = image.preview.path.clone().filter(|path| path.is_file()) else {
            return Ok(None);
        };
        Ok(Some((
            preview,
            CodexAnalysisOptions {
                codex_binary: config.codex_binary.clone(),
                convert_binary: self.convert.clone(),
                model: config.model.clone(),
                timeout: config.timeout,
                flags: config.flags,
                resize_preview: is_rendered_input_file(&image.raw_path),
            },
        )))
    }

    fn record_codex_processing(&self, raw: &Path, analysis_key: &str) -> Result<()> {
        self.update_codex_if_key(raw, analysis_key, |codex, _image| {
            codex.status = ReviewCodexStatus::Processing;
            codex.error = None;
        })
    }

    fn record_codex_done(
        &self,
        raw: &Path,
        analysis_key: &str,
        result: CodexAnalysisResult,
        config: &ReviewCodexConfig,
    ) -> Result<()> {
        self.update_codex_if_key(raw, analysis_key, |codex, image| {
            if config.flags.tags && image.tags_source != ReviewMetadataSource::Manual {
                image.tags = normalize_tags(result.tags.clone());
                image.tags_source = ReviewMetadataSource::Codex;
            }
            if config.flags.note
                && image.notes_source != ReviewMetadataSource::Manual
                && let Some(note) = result.note.clone()
            {
                image.notes = note;
                image.notes_source = ReviewMetadataSource::Codex;
            }
            if config.flags.rating
                && image.rating_source == ReviewMetadataSource::Default
                && let Some(rating) = result.rating
            {
                image.rating = rating.min(5);
                image.rating_source = ReviewMetadataSource::Codex;
            }
            codex.status = ReviewCodexStatus::Done;
            codex.error = None;
        })
    }

    fn record_codex_failed(&self, raw: &Path, analysis_key: &str, error: &str) -> Result<()> {
        self.update_codex_if_key(raw, analysis_key, |codex, _image| {
            codex.status = ReviewCodexStatus::Failed;
            codex.error = Some(error.to_string());
        })
    }

    fn update_codex_if_key<F>(&self, raw: &Path, analysis_key: &str, update: F) -> Result<()>
    where
        F: Fn(&mut ReviewCodexAnalysis, &mut ReviewImage),
    {
        let history_entries = self.update_store(|store| {
            let mut history_entries = Vec::new();
            let Some(image) = store.images.iter_mut().find(|image| image.raw_path == raw) else {
                return Ok(history_entries);
            };
            if image.codex.analysis_key.as_deref() != Some(analysis_key) {
                return Ok(history_entries);
            }
            let before_image = image.clone();
            let before_codex = image.codex.clone();
            let mut codex = std::mem::take(&mut image.codex);
            update(&mut codex, image);
            codex.updated_at = now_string();
            image.codex = codex;
            image.updated_at = now_string();
            if let Some(entry) = history_codex_changed(image, &before_codex, &image.codex) {
                history_entries.push(entry);
            }
            if let Some(entry) = history_review_changed(&before_image, image) {
                history_entries.push(entry);
            }
            Ok(history_entries)
        })?;
        for entry in history_entries {
            self.append_history(entry)?;
        }
        self.broadcast_state()
    }

    pub(super) fn run_scheduled_retouch_job(&self, mut job: ScheduledRetouchJob) {
        if job.profile_index == Some(SOOC_PROFILE_INDEX) {
            self.run_scheduled_sooc_retouch_job(job);
            return;
        }
        if job.profile_index.is_none() {
            self.run_scheduled_compressed_retouch_job(job);
            return;
        }
        let profile_index = job.profile_index.expect("profile retouch job has an index");
        let Ok(Some(task)) =
            self.retouch_task_snapshot(job.image_id, profile_index, &job.render_key)
        else {
            return;
        };
        job.raw = task.raw;
        let profile = task.profile;
        let retouch = task.retouch;
        let white_balance = task.white_balance;
        let bw_filter = task.bw_filter;
        let use_base_output = profile_retouch_uses_base_output(&retouch, bw_filter);
        let dcp_profile_filename =
            resolve_dcp_profile(&job.raw, &self.dng_fallback).map(|profile| profile.filename);
        let started = Instant::now();
        let mut cached = false;
        let Ok(updated) =
            self.update_render_if_key(job.image_id, profile_index, &job.render_key, |render| {
                cached = apply_cached_profile_output(
                    render,
                    &job.output,
                    &job.render_key,
                    use_base_output,
                    &self.output_root,
                    &self.cache_root,
                );
                if !cached {
                    render.status = ReviewRenderStatus::Processing;
                    render.error = None;
                } else {
                    render.dcp_profile_filename = dcp_profile_filename.clone();
                }
            })
        else {
            return;
        };
        if !updated {
            return;
        }
        if cached {
            let _ = self.maybe_schedule_codex_for_raw(&job.raw);
            return;
        }
        let final_output = profile_retouch_output(
            &job.output,
            &job.render_key,
            use_base_output,
            &self.output_root,
            &self.cache_root,
        );
        let temp_output = retouch_temp_output(&final_output, &job.render_key);
        let mut result = self.render_retouch_output(
            &job.raw,
            &profile,
            &retouch,
            white_balance,
            bw_filter,
            &temp_output,
        );
        if result.is_err() {
            let retry_raw = self
                .review_raw_for_image_id(job.image_id)
                .filter(|current_raw| current_raw != &job.raw)
                .or_else(|| {
                    self.dng_fallback
                        .existing_successor(&job.raw)
                        .ok()
                        .flatten()
                        .map(|successor| successor.active().to_path_buf())
                });
            if let Some(retry_raw) = retry_raw {
                job.raw = retry_raw;
                let _ = fs::remove_file(&temp_output);
                result = self.render_retouch_output(
                    &job.raw,
                    &profile,
                    &retouch,
                    white_balance,
                    bw_filter,
                    &temp_output,
                );
            }
        }
        match result {
            Ok(current_raw) => {
                job.raw = current_raw;
                match self.retouch_task_snapshot(job.image_id, profile_index, &job.render_key) {
                    Ok(Some(task)) => {
                        job.raw = task.raw;
                        if let Some(parent) = final_output.parent()
                            && let Err(error) = fs::create_dir_all(parent)
                        {
                            self.record_retouch_render_failed(
                                &job,
                                &temp_output,
                                started,
                                error.to_string(),
                            );
                            return;
                        }
                        if final_output.exists()
                            && let Err(error) = fs::remove_file(&final_output)
                        {
                            self.record_retouch_render_failed(
                                &job,
                                &temp_output,
                                started,
                                error.to_string(),
                            );
                            return;
                        }
                        if let Err(error) = fs::rename(&temp_output, &final_output) {
                            self.record_retouch_render_failed(
                                &job,
                                &temp_output,
                                started,
                                error.to_string(),
                            );
                            return;
                        }
                        let _ = self.update_render_if_key(
                            job.image_id,
                            profile_index,
                            &job.render_key,
                            |render| {
                                apply_profile_retouch_done(
                                    render,
                                    &final_output,
                                    started.elapsed(),
                                    dcp_profile_filename.as_deref(),
                                );
                            },
                        );
                    }
                    _ => {
                        let _ = fs::remove_file(&temp_output);
                    }
                }
            }
            Err(error) => {
                self.record_retouch_render_failed(
                    &job,
                    &temp_output,
                    started,
                    format!("{error:#}"),
                );
            }
        }
    }

    pub(super) fn run_scheduled_sooc_retouch_job(&self, mut job: ScheduledRetouchJob) {
        let Ok(Some((current_raw, sidecar, retouch))) =
            self.sooc_retouch_task_snapshot(job.image_id, &job.render_key)
        else {
            return;
        };
        job.raw = current_raw;
        let started = Instant::now();
        let _ = self.update_render_if_key(
            job.image_id,
            SOOC_PROFILE_INDEX,
            &job.render_key,
            |render| {
                render.status = ReviewRenderStatus::Processing;
                render.error = None;
            },
        );
        let final_output = retouch_cache_output(
            &job.output,
            &job.render_key,
            &self.output_root,
            &self.cache_root,
        );
        let temp_output = retouch_temp_output(&final_output, &job.render_key);
        let result = self.render_sooc_retouch_output(&sidecar, &retouch, &temp_output);
        match result {
            Ok(()) => match self.sooc_retouch_task_snapshot(job.image_id, &job.render_key) {
                Ok(Some(_)) => {
                    if let Some(parent) = final_output.parent()
                        && let Err(error) = fs::create_dir_all(parent)
                    {
                        self.record_retouch_render_failed(
                            &job,
                            &temp_output,
                            started,
                            error.to_string(),
                        );
                        return;
                    }
                    if final_output.exists()
                        && let Err(error) = fs::remove_file(&final_output)
                    {
                        self.record_retouch_render_failed(
                            &job,
                            &temp_output,
                            started,
                            error.to_string(),
                        );
                        return;
                    }
                    if let Err(error) = fs::rename(&temp_output, &final_output) {
                        self.record_retouch_render_failed(
                            &job,
                            &temp_output,
                            started,
                            error.to_string(),
                        );
                        return;
                    }
                    let _ = self.update_render_if_key(
                        job.image_id,
                        SOOC_PROFILE_INDEX,
                        &job.render_key,
                        |render| {
                            render.status = ReviewRenderStatus::Done;
                            render.render_key = None;
                            render.output_path = Some(final_output.clone());
                            render.error = None;
                            render.duration_ms = Some(started.elapsed().as_millis() as u64);
                            refresh_review_render_dimensions(render, &final_output);
                        },
                    );
                }
                _ => {
                    let _ = fs::remove_file(&temp_output);
                }
            },
            Err(error) => {
                self.record_retouch_render_failed(
                    &job,
                    &temp_output,
                    started,
                    format!("{error:#}"),
                );
            }
        }
    }

    pub(super) fn run_scheduled_compressed_retouch_job(&self, mut job: ScheduledRetouchJob) {
        let Ok(Some((current_raw, retouch))) =
            self.compressed_retouch_task_snapshot(job.image_id, &job.render_key)
        else {
            return;
        };
        job.raw = current_raw;
        let started = Instant::now();
        let _ = self.update_preview_if_key(job.image_id, &job.render_key, |preview| {
            preview.status = ReviewRenderStatus::Processing;
            preview.error = None;
        });
        let final_output = retouch_cache_output(
            &job.output,
            &job.render_key,
            &self.output_root,
            &self.cache_root,
        );
        let temp_output = retouch_temp_output(&final_output, &job.render_key);
        let result = self.render_compressed_retouch_output(&job.raw, &retouch, &temp_output);
        match result {
            Ok(()) => match self.compressed_retouch_task_snapshot(job.image_id, &job.render_key) {
                Ok(Some(_)) => {
                    if let Some(parent) = final_output.parent()
                        && let Err(error) = fs::create_dir_all(parent)
                    {
                        self.record_retouch_render_failed(
                            &job,
                            &temp_output,
                            started,
                            error.to_string(),
                        );
                        return;
                    }
                    if final_output.exists()
                        && let Err(error) = fs::remove_file(&final_output)
                    {
                        self.record_retouch_render_failed(
                            &job,
                            &temp_output,
                            started,
                            error.to_string(),
                        );
                        return;
                    }
                    if let Err(error) = fs::rename(&temp_output, &final_output) {
                        self.record_retouch_render_failed(
                            &job,
                            &temp_output,
                            started,
                            error.to_string(),
                        );
                        return;
                    }
                    let _ = self.update_preview_if_key(job.image_id, &job.render_key, |preview| {
                        preview.status = ReviewRenderStatus::Done;
                        preview.render_key = None;
                        preview.path = Some(final_output.clone());
                        preview.error = None;
                        preview.duration_ms = Some(started.elapsed().as_millis() as u64);
                    });
                }
                _ => {
                    let _ = fs::remove_file(&temp_output);
                }
            },
            Err(error) => {
                self.record_retouch_render_failed(
                    &job,
                    &temp_output,
                    started,
                    format!("{error:#}"),
                );
            }
        }
    }

    pub(super) fn record_retouch_render_failed(
        &self,
        job: &ScheduledRetouchJob,
        temp_output: &Path,
        started: Instant,
        message: String,
    ) {
        if let Some(profile_index) = job.profile_index {
            let _ =
                self.update_render_if_key(job.image_id, profile_index, &job.render_key, |render| {
                    render.status = ReviewRenderStatus::Failed;
                    render.render_key = None;
                    render.error = Some(message.clone());
                    render.duration_ms = Some(started.elapsed().as_millis() as u64);
                });
        } else {
            let _ = self.update_preview_if_key(job.image_id, &job.render_key, |preview| {
                preview.status = ReviewRenderStatus::Failed;
                preview.render_key = None;
                preview.error = Some(message.clone());
                preview.duration_ms = Some(started.elapsed().as_millis() as u64);
            });
        }
        let _ = fs::remove_file(temp_output);
    }

    pub(super) fn render_retouch_output(
        &self,
        raw: &Path,
        profile: &ReviewProfile,
        retouch: &RetouchSettings,
        white_balance: RetouchWhiteBalance,
        bw_filter: BwFilter,
        output: &Path,
    ) -> Result<PathBuf> {
        let raw = safe_existing_raw_source(raw, &self.input_root)?;
        let temp_dir = Builder::new()
            .prefix("mini-film-review-retouch-")
            .tempdir()?;
        let apply_args = ApplyArgs {
            raw: raw.clone(),
            output: output.to_path_buf(),
            profile: optional_profile_selector(&profile.selector),
            hald_dir: self.hald_dir.clone(),
            profiles_root: self.profiles_root.clone(),
            hald_level: self.hald_level,
            rawtherapee: self.rawtherapee.clone(),
            dng_fallback: self.dng_fallback.clone(),
            convert: self.convert.clone(),
            lcp_root: self.lcp_root.clone(),
            keep_intermediate: None,
            no_grain: self.no_grain,
            normalize_grain_mpix: self.normalize_grain_mpix,
            color_noise_iso_threshold: self.color_noise_iso_threshold,
            lens_corrections: self.lens_corrections,
            grain: self.grain.clone(),
            grain_preset: self.grain_preset.clone(),
            grain_seed: self.grain_seed,
            grain_engine: self.grain_engine,
            export: self.export.clone(),
            retouch: None,
            retouch_white_balance: white_balance,
            bw_filter,
        };
        let mut resolved = resolve_profile(&apply_args, temp_dir.path())?;
        if let Some(grain) =
            resolve_grain_override(self.grain.as_deref(), self.grain_preset.as_deref())?
        {
            resolved.grain = grain;
        }
        let seed = self
            .grain_seed
            .map(|seed| review_publish_seed(seed, &raw, profile.index))
            .unwrap_or_else(|| review_publish_seed(0, &raw, profile.index));
        let outcome = apply_resolved(
            ApplyJob {
                raw: &raw,
                output,
                rawtherapee: &self.rawtherapee,
                dng_fallback: &self.dng_fallback,
                prepared_raw: None,
                convert: &self.convert,
                keep_intermediate: None,
                no_grain: self.no_grain,
                normalize_grain_mpix: self.normalize_grain_mpix,
                grain_engine: self.grain_engine,
                color_noise_iso_threshold: self.color_noise_iso_threshold,
                lens_corrections: self.lens_corrections,
                lcp_root: self.lcp_root.as_deref(),
                export: &self.export,
                quiet: true,
                exif_comment: Some(format!(
                    "mini-film {} usage=review profile={} {}{}",
                    env!("CARGO_PKG_VERSION"),
                    if profile.stem.trim().is_empty() {
                        "none"
                    } else {
                        &profile.stem
                    },
                    retouch.summary(),
                    bw_filter_summary(bw_filter)
                )),
                retouch: Some(retouch),
                retouch_white_balance: white_balance,
                bw_filter,
                profile_input_cache_root: Some(&self.cache_root),
            },
            &resolved,
            seed,
            temp_dir.path(),
            None,
        )?;
        if outcome.source_path != raw {
            self.rebind_and_queue_converted_source(&raw, &outcome.source_path)?;
        }
        Ok(outcome.source_path)
    }

    pub(super) fn render_compressed_retouch_output(
        &self,
        input: &Path,
        retouch: &RetouchSettings,
        output: &Path,
    ) -> Result<()> {
        let input = safe_existing_raw_source(input, &self.input_root)?;
        apply_compressed(
            CompressedApplyJob {
                input: &input,
                output,
                convert: &self.convert,
                export: &self.export,
                exif_comment: Some(format!(
                    "mini-film {} usage=review compressed-input {}",
                    env!("CARGO_PKG_VERSION"),
                    retouch.summary()
                )),
                retouch: Some(retouch),
            },
            None,
        )
    }

    pub(super) fn render_sooc_retouch_output(
        &self,
        sidecar: &Path,
        retouch: &RetouchSettings,
        output: &Path,
    ) -> Result<()> {
        let sidecar = safe_existing_raw_source(sidecar, &self.input_root)?;
        apply_compressed(
            CompressedApplyJob {
                input: &sidecar,
                output,
                convert: &self.convert,
                export: &self.export,
                exif_comment: Some(format!(
                    "mini-film {} usage=review sooc-sidecar {}",
                    env!("CARGO_PKG_VERSION"),
                    retouch.summary()
                )),
                retouch: Some(retouch),
            },
            None,
        )
    }

    #[cfg(test)]
    pub(super) fn apply_review_update(&self, update: ReviewUpdateRequest) -> Result<()> {
        self.database_runtime
            .block_on(self.apply_review_update_async(update))
    }

    pub(super) async fn apply_review_update_async(
        &self,
        update: ReviewUpdateRequest,
    ) -> Result<()> {
        let (history_entries, retouch_jobs) = self
            .update_store_async(|store| {
                let mut retouch_jobs = Vec::new();
                let mut history_entries = Vec::new();
                let before_ui = store.ui.clone();
                let profiles_by_index = store
                    .profiles
                    .iter()
                    .cloned()
                    .map(|profile| (profile.index, profile))
                    .collect::<HashMap<_, _>>();
                let advance = update
                    .advance_after_update
                    .then(|| store.planned_advance_after(update.image_id));
                {
                    let Some(image) = store
                        .images
                        .iter_mut()
                        .find(|image| image.id == update.image_id)
                    else {
                        bail!("review image {} does not exist", update.image_id);
                    };
                    let mut direct_compressed = image_is_direct_compressed(image);
                    if let Some(selected_profile_index) = update.selected_profile_index
                        && !image
                            .profiles
                            .iter()
                            .any(|profile| profile.profile_index == selected_profile_index)
                    {
                        bail!(
                            "selected profile index {} is not available for image {}",
                            selected_profile_index,
                            update.image_id
                        );
                    }
                    let before_image = image.clone();
                    let before_rating = image.rating;
                    let before_tags = image.tags.clone();
                    let before_notes = image.notes.clone();
                    image.rating = update.rating.min(5);
                    image.labels = if update.labels.is_empty() {
                        normalize_review_labels([update.label])
                    } else {
                        normalize_review_labels(update.labels.clone())
                    };
                    image.label = first_review_label(&image.labels);
                    image.tags = normalize_tags(update.tags.clone());
                    image.notes = update.notes.trim().to_string();
                    if image.rating != before_rating {
                        image.rating_source = ReviewMetadataSource::Manual;
                    }
                    if image.tags != before_tags {
                        image.tags_source = ReviewMetadataSource::Manual;
                    }
                    if image.notes != before_notes {
                        image.notes_source = ReviewMetadataSource::Manual;
                    }
                    let retouch_changed = update
                        .retouch
                        .as_ref()
                        .is_some_and(|retouch| retouch.clone().normalized() != image.retouch);
                    let mut newly_enabled_profile_indexes = Vec::new();
                    if let Some(retouch) = update.retouch.clone() {
                        image.retouch = retouch.normalized();
                    }
                    if let Some(indexes) = update.enabled_profile_indexes.clone() {
                        if image.profiles.is_empty() {
                            bail!("profiles are not available for direct compressed review images");
                        }
                        validate_publish_profile_indexes(&indexes, &image.profiles)?;
                        let enabled = indexes.into_iter().collect::<HashSet<_>>();
                        for render in &mut image.profiles {
                            if render.profile_index != SOOC_PROFILE_INDEX {
                                let was_enabled = render.enabled;
                                render.enabled = enabled.contains(&render.profile_index);
                                if render.enabled && !was_enabled {
                                    newly_enabled_profile_indexes.push(render.profile_index);
                                } else if !render.enabled {
                                    let queued_or_retouch_processing = render.status
                                        == ReviewRenderStatus::Queued
                                        || (render.status == ReviewRenderStatus::Processing
                                            && render.render_key.is_some());
                                    render.render_key = None;
                                    if queued_or_retouch_processing {
                                        render.status = ReviewRenderStatus::Missing;
                                        render.error = None;
                                        render.duration_ms = None;
                                    }
                                }
                            }
                        }
                        if !image.profiles.iter().any(|render| {
                            render.enabled && render.profile_index == image.selected_profile_index
                        }) {
                            image.selected_profile_index = image
                                .profiles
                                .iter()
                                .find(|render| render.enabled)
                                .map(|render| render.profile_index)
                                .unwrap_or_default();
                        }
                        image.publish_profile_indexes = Some(
                            image
                                .profiles
                                .iter()
                                .filter(|render| {
                                    render.enabled && render.profile_index != SOOC_PROFILE_INDEX
                                })
                                .map(|render| render.profile_index)
                                .collect(),
                        );
                        direct_compressed = image_is_direct_compressed(image);
                    }
                    if let Some(selected_profile_index) =
                        update.selected_profile_index.filter(|_| !direct_compressed)
                    {
                        image.selected_profile_index = selected_profile_index;
                    }
                    if let Some(indexes) = update.publish_profile_indexes.clone() {
                        if direct_compressed {
                            image.publish_profile_indexes = Some(Vec::new());
                        } else {
                            validate_publish_profile_indexes(&indexes, &image.profiles)?;
                            image.publish_profile_indexes =
                                Some(normalize_publish_profile_indexes(&indexes, &image.profiles));
                        }
                    }
                    let mut changed_bw_profile_indexes = Vec::new();
                    if let Some(filters) = update.profile_bw_filters.clone() {
                        let normalized_filters = if direct_compressed {
                            Vec::new()
                        } else {
                            normalize_profile_bw_filters(&filters, &image.profiles)
                        };
                        if normalized_filters != image.profile_bw_filters {
                            changed_bw_profile_indexes = changed_bw_filter_profile_indexes(
                                &image.profile_bw_filters,
                                &normalized_filters,
                                &image.profiles,
                                &profiles_by_index,
                            );
                            image.profile_bw_filters = normalized_filters;
                        }
                    }
                    if retouch_changed {
                        if direct_compressed {
                            let base_output = image.preview.path.as_deref().map(|output| {
                                retouch_base_output(output, &self.output_root, &self.cache_root)
                            });
                            if let Some(render_key) =
                                retouch_render_key(&image.retouch, &self.export)
                            {
                                let cached = base_output.as_deref().is_some_and(|output| {
                                    apply_cached_preview_output(
                                        &mut image.preview,
                                        output,
                                        &render_key,
                                        &self.output_root,
                                        &self.cache_root,
                                    )
                                });
                                if !cached {
                                    image.preview.status = ReviewRenderStatus::Queued;
                                    image.preview.error = None;
                                    image.preview.duration_ms = None;
                                    image.preview.render_key = Some(render_key.clone());
                                    if let Some(output) = base_output {
                                        retouch_jobs.push(ReviewRetouchRequest {
                                            image_id: image.id,
                                            raw: image.raw_path.clone(),
                                            profile_index: None,
                                            output,
                                            render_key,
                                        });
                                    }
                                }
                            } else {
                                if let Some(output) = base_output {
                                    image.preview.status = if output.is_file() {
                                        ReviewRenderStatus::Done
                                    } else {
                                        ReviewRenderStatus::Queued
                                    };
                                    image.preview.path = Some(output);
                                }
                                image.preview.error = None;
                                image.preview.duration_ms = Some(0);
                                image.preview.render_key = None;
                            }
                            image.preview.updated_at = now_string();
                        } else {
                            let publish_indexes = effective_publish_profile_indexes(image);
                            let visible_profile_index =
                                preferred_preview_profile_index(image, &publish_indexes);
                            let publish_index_set =
                                publish_indexes.iter().copied().collect::<HashSet<_>>();
                            let mut render_order = image
                                .profiles
                                .iter()
                                .enumerate()
                                .filter(|(_, render)| render.enabled)
                                .map(|(index, render)| {
                                    let priority = if Some(render.profile_index)
                                        == visible_profile_index
                                    {
                                        0
                                    } else if publish_index_set.contains(&render.profile_index) {
                                        1
                                    } else {
                                        2
                                    };
                                    (priority, index)
                                })
                                .collect::<Vec<_>>();
                            render_order.sort_by_key(|(priority, index)| (*priority, *index));
                            for (_, index) in render_order {
                                let profile_index = image.profiles[index].profile_index;
                                let processing_key =
                                    review_render_processing_key_for_input_with_options(
                                        &image.raw_path,
                                        profile_index,
                                        self.normalize_grain_mpix,
                                        &self.export,
                                    );
                                if image.profiles[index].output_path.is_none()
                                    && let Some(profile) = profiles_by_index.get(&profile_index)
                                {
                                    image.profiles[index].output_path =
                                        Some(crate::app::batch_daemon::daemon_output_path(
                                            &self.input_root,
                                            &self.output_root,
                                            self.output_format,
                                            &image.raw_path,
                                            &profile.stem,
                                        )?);
                                }
                                let bw_filter = profiles_by_index
                                    .get(&profile_index)
                                    .map(|profile| effective_bw_filter_for_profile(image, profile))
                                    .unwrap_or_default();
                                let render_key = profile_render_key_value(
                                    &image.retouch,
                                    retouch_white_balance_for_image(image),
                                    bw_filter,
                                    (profile_index != SOOC_PROFILE_INDEX)
                                        .then_some(self.normalize_grain_mpix)
                                        .flatten(),
                                    &processing_key,
                                );
                                queue_profile_retouch_render(
                                    image,
                                    index,
                                    render_key,
                                    profile_retouch_uses_base_output(&image.retouch, bw_filter),
                                    &mut retouch_jobs,
                                    &self.output_root,
                                    &self.cache_root,
                                );
                            }
                        }
                    } else if (!changed_bw_profile_indexes.is_empty()
                        || !newly_enabled_profile_indexes.is_empty())
                        && !direct_compressed
                    {
                        let changed_indexes = changed_bw_profile_indexes
                            .iter()
                            .chain(newly_enabled_profile_indexes.iter())
                            .copied()
                            .collect::<HashSet<_>>();
                        for index in 0..image.profiles.len() {
                            let profile_index = image.profiles[index].profile_index;
                            if !image.profiles[index].enabled
                                || !changed_indexes.contains(&profile_index)
                            {
                                continue;
                            }
                            let Some(profile) = profiles_by_index.get(&profile_index) else {
                                continue;
                            };
                            if image.profiles[index].output_path.is_none() {
                                image.profiles[index].output_path =
                                    Some(crate::app::batch_daemon::daemon_output_path(
                                        &self.input_root,
                                        &self.output_root,
                                        self.output_format,
                                        &image.raw_path,
                                        &profile.stem,
                                    )?);
                            }
                            let processing_key =
                                review_render_processing_key_for_input_with_options(
                                    &image.raw_path,
                                    profile_index,
                                    self.normalize_grain_mpix,
                                    &self.export,
                                );
                            image.profiles[index].processing_key = Some(processing_key.clone());
                            let bw_filter = effective_bw_filter_for_profile(image, profile);
                            let render_key = profile_render_key_value(
                                &image.retouch,
                                retouch_white_balance_for_image(image),
                                bw_filter,
                                (profile_index != SOOC_PROFILE_INDEX)
                                    .then_some(self.normalize_grain_mpix)
                                    .flatten(),
                                &processing_key,
                            );
                            queue_profile_retouch_render(
                                image,
                                index,
                                render_key,
                                profile_retouch_uses_base_output(&image.retouch, bw_filter),
                                &mut retouch_jobs,
                                &self.output_root,
                                &self.cache_root,
                            );
                        }
                    }
                    image.updated_at = now_string();
                    if let Some(entry) = history_review_changed(&before_image, image) {
                        history_entries.push(entry);
                    }
                }
                if let Some(advance) = advance {
                    store.apply_advance(advance);
                } else {
                    store.normalize_ui();
                }
                if let Some(entry) = history_ui_changed(store, &before_ui, &store.ui) {
                    history_entries.push(entry);
                }
                Ok((history_entries, retouch_jobs))
            })
            .await?;
        for entry in history_entries {
            self.append_history(entry)?;
        }
        self.broadcast_state()?;
        for job in retouch_jobs {
            self.retouch_scheduler.schedule(job);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn apply_ui_update(&self, update: ReviewUiUpdateRequest) -> Result<()> {
        self.database_runtime
            .block_on(self.apply_ui_update_async(update))
    }

    pub(super) async fn apply_ui_update_async(&self, update: ReviewUiUpdateRequest) -> Result<()> {
        let history_entry = self
            .update_store_async(|store| {
                let before_ui = store.ui.clone();
                store.set_ui(update)?;
                Ok(history_ui_changed(store, &before_ui, &store.ui))
            })
            .await?;
        if let Some(entry) = history_entry {
            self.append_history(entry)?;
        }
        self.broadcast_state()
    }

    pub(super) fn api_state_json(&self) -> Result<String> {
        serde_json::to_string(&self.api_state_value()?).context("serializing review API state")
    }

    pub(super) fn api_state_value(&self) -> Result<serde_json::Value> {
        let client_count = self.client_count()?;
        let store = self.store_snapshot();
        let mut images = store.images.clone();
        sort_review_images(&mut images);
        let active_image_id = store.ui.current_image_id;
        let codex_summary = review_codex_summary(&images);
        let images = images
            .iter()
            .map(|image| {
                let mut exif = image.exif.clone();
                exif.sanitize_text_fields();
                let compressed = is_rendered_input_file(&image.raw_path);
                let tiff = is_tiff_input_file(&image.raw_path);
                let profiled = image_uses_profile_pipeline(image);
                let preview_ready = if compressed {
                    self.compressed_display_preview_path_for(&image.raw_path, image.id)
                        .is_file()
                } else {
                    image.preview.status == ReviewRenderStatus::Done
                };
                let thumbnail_ready = compressed
                    && self
                        .compressed_thumbnail_path_for(&image.raw_path, image.id)
                        .is_file();
                let full_source_ready = compressed && image.raw_path.is_file();
                let sidecar_crop_source_ready = image
                    .sooc_sidecar_path
                    .as_ref()
                    .map(|source| self.crop_source_preview_path_for(source, image.id))
                    .is_some_and(|path| path.is_file());
                let profiles = image
                    .profiles
                    .iter()
                    .map(|render| {
                        let profile = store
                            .profiles
                            .iter()
                            .find(|profile| profile.index == render.profile_index);
                        let bw_filter_eligible =
                            profile.is_some_and(review_profile_bw_filter_eligible);
                        let bw_filter = profile
                            .map(|profile| effective_bw_filter_for_profile(image, profile))
                            .unwrap_or_default();
                        let base_output_ready = render
                            .output_path
                            .as_ref()
                            .map(|output| {
                                retouch_base_output(output, &self.output_root, &self.cache_root)
                            })
                            .is_some_and(|output| output.is_file());
                        let file_size_bytes = if active_image_id == Some(image.id)
                            && render.status == ReviewRenderStatus::Done
                        {
                            render
                                .output_path
                                .as_ref()
                                .and_then(|path| path.metadata().ok())
                                .map(|metadata| metadata.len())
                        } else {
                            None
                        };
                        json!({
                            "profile_index": render.profile_index,
                            "profile_stem": render.profile_stem,
                            "display_name": render.display_name,
                            "enabled": render.enabled,
                            "status": render.status,
                            "url": if render.status == ReviewRenderStatus::Done {
                                Some(format!("media/{}/{}", image.id, render.profile_index))
                            } else {
                                None
                            },
                            "base_url": if base_output_ready {
                                Some(format!("media/{}/{}/base", image.id, render.profile_index))
                            } else {
                                None
                            },
                            "error": render.error,
                            "duration_ms": render.duration_ms,
                            "file_size_bytes": file_size_bytes,
                            "width": render.width,
                            "height": render.height,
                            "retouch_pending": render.render_key.is_some(),
                            "dcp_profile_filename": effective_dcp_profile_filename(image, render),
                            "bw_filter_eligible": bw_filter_eligible,
                            "bw_filter": bw_filter,
                            "updated_at": render.updated_at,
                        })
                    })
                    .collect::<Vec<_>>();
                json!({
                    "id": image.id,
                    "source_type": if compressed { "compressed" } else { "raw" },
                    "processing_mode": if profiled { "profiled" } else { "direct" },
                    "relative_path": image.relative_path,
                    "file_name": image.file_name,
                    "source_file_size_bytes": image.exif.file_size_bytes,
                    "source_width": image.exif.image_width,
                    "source_height": image.exif.image_height,
                    "exif": exif,
                    "preview_status": image.preview.status,
                    "thumbnail_url": if thumbnail_ready {
                        Some(format!("thumbnail/{}", image.id))
                    } else {
                        None
                    },
                    "preview_url": if preview_ready {
                        Some(format!("preview/{}", image.id))
                    } else {
                        None
                    },
                    "crop_source_url": if sidecar_crop_source_ready {
                        Some(format!("crop-source/{}", image.id))
                    } else if preview_ready {
                        Some(format!("preview/{}", image.id))
                    } else {
                        None
                    },
                    "crop_source_updated_at": if sidecar_crop_source_ready {
                        &image.updated_at
                    } else {
                        &image.preview.updated_at
                    },
                    "full_url": if full_source_ready && tiff {
                        Some(format!("full-preview/{}", image.id))
                    } else if full_source_ready {
                        Some(format!("original/{}", image.id))
                    } else {
                        None
                    },
                    "preview_error": image.preview.error,
                    "preview_duration_ms": image.preview.duration_ms,
                    "preview_retouch_pending": image.preview.render_key.is_some(),
                    "preview_updated_at": image.preview.updated_at,
                    "selected_profile_index": image.selected_profile_index,
                    "rating": image.rating,
                    "label": image.label,
                    "labels": image_review_labels(image),
                    "tags": image.tags,
                    "notes": image.notes,
                    "rating_source": image.rating_source,
                    "tags_source": image.tags_source,
                    "notes_source": image.notes_source,
                    "codex": {
                        "status": image.codex.status,
                        "flags": image.codex.flags,
                        "model": image.codex.model,
                        "error": image.codex.error,
                        "updated_at": image.codex.updated_at,
                    },
                    "retouch": image.retouch,
                    "publish_profile_indexes": effective_publish_profile_indexes(image),
                    "profile_bw_filters": image.profile_bw_filters,
                    "profiles": profiles,
                    "updated_at": image.updated_at,
                })
            })
            .collect::<Vec<_>>();
        let panorama_projects = self
            .panorama_projects_snapshot()
            .iter()
            .map(|project| {
                let previews = project
                    .previews
                    .iter()
                    .map(|preview| {
                        json!({
                            "matching_mode": preview.matching_mode,
                            "projection": preview.projection,
                            "status": preview.status,
                            "url": if preview.status == ReviewPanoramaPreviewStatus::Done
                                && preview.path.as_ref().is_some_and(|path| path.is_file())
                            {
                                Some(format!(
                                    "panorama-preview/{}/{}/{}",
                                    project.id, preview.matching_mode, preview.projection
                                ))
                            } else {
                                None
                            },
                            "duration_ms": preview.duration_ms,
                            "error": preview.error,
                            "updated_at": preview.updated_at,
                        })
                    })
                    .collect::<Vec<_>>();
                json!({
                    "id": project.id,
                    "name": project.name,
                    "status": project.status,
                    "matching_mode": project.matching_mode,
                    "selected_projection": project.selected_projection,
                    "output_file_name": project.output_path.as_ref().and_then(|path| path.file_name()).and_then(|name| name.to_str()),
                    "result_image_id": project.result_image_id,
                    "progress_stage": project.progress_stage,
                    "progress_completed": project.progress_completed,
                    "progress_total": project.progress_total,
                    "error": project.error,
                    "created_at": project.created_at,
                    "updated_at": project.updated_at,
                    "image_ids": project.image_ids,
                    "previews": previews,
                })
            })
            .collect::<Vec<_>>();

        Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "invocation": self.invocation,
            "profiles": store.profiles,
            "client_count": client_count,
            "codex": {
                "enabled": self.codex.is_some(),
                "flags": self.codex.as_ref().map(|config| config.flags),
                "model": self.codex.as_ref().map(|config| config.model.clone()),
                "queued": codex_summary.queued,
                "processing": codex_summary.processing,
                "done": codex_summary.done,
                "failed": codex_summary.failed,
            },
            "publish_defaults": self.publish_defaults,
            "publish_jobs": self.publish_jobs_snapshot()?,
            "capabilities": {
                "panorama": self.panorama_capability,
                "sampler": self.sampler_available(),
            },
            "panorama": {
                "busy": self.panorama_operation.load(Ordering::Acquire),
                "projects": panorama_projects,
            },
            "ui": {
                "current_image_id": store.ui.current_image_id,
                "min_rating": store.ui.min_rating,
            },
            "images": images,
            "publish_root": self.publish_root().to_string_lossy(),
        }))
    }

    pub(super) fn api_state_patch_json_since(
        &self,
        previous: &serde_json::Value,
    ) -> Result<String> {
        let current = self.api_state_value()?;
        serde_json::to_string(&review_state_patch_value(previous, &current))
            .context("serializing review API patch")
    }

    pub(super) fn media_path(&self, image_id: u64, profile_index: usize) -> Result<PathBuf> {
        let store = self.store_snapshot();
        let image = store
            .images
            .iter()
            .find(|image| image.id == image_id)
            .ok_or_else(|| anyhow!("review image {image_id} does not exist"))?;
        let render = image
            .profiles
            .iter()
            .find(|render| render.profile_index == profile_index)
            .ok_or_else(|| anyhow!("profile {profile_index} is not available"))?;
        if render.status != ReviewRenderStatus::Done {
            bail!("profile {profile_index} is not ready");
        }
        let path = render
            .output_path
            .as_ref()
            .ok_or_else(|| anyhow!("profile {profile_index} has no output path"))?;
        if !path.is_file() {
            bail!("review media is missing: {}", path.display());
        }
        Ok(path.clone())
    }

    pub(super) fn profile_base_media_path(
        &self,
        image_id: u64,
        profile_index: usize,
    ) -> Result<PathBuf> {
        let store = self.store_snapshot();
        let image = store
            .images
            .iter()
            .find(|image| image.id == image_id)
            .ok_or_else(|| anyhow!("review image {image_id} does not exist"))?;
        let render = image
            .profiles
            .iter()
            .find(|render| render.profile_index == profile_index)
            .ok_or_else(|| anyhow!("profile {profile_index} is not available"))?;
        let output = render
            .output_path
            .as_ref()
            .ok_or_else(|| anyhow!("profile {profile_index} has no output path"))?;
        let base = retouch_base_output(output, &self.output_root, &self.cache_root);
        if !base.is_file() {
            bail!(
                "profile {profile_index} base media is missing: {}",
                base.display()
            );
        }
        Ok(base)
    }

    pub(super) fn crop_source_media_path(&self, image_id: u64) -> Result<PathBuf> {
        let store = self.store_snapshot();
        let image = store
            .images
            .iter()
            .find(|image| image.id == image_id)
            .ok_or_else(|| anyhow!("review image {image_id} does not exist"))?;
        let source = image
            .sooc_sidecar_path
            .as_ref()
            .ok_or_else(|| anyhow!("review image {image_id} has no SOOC sidecar"))?;
        let path = self.crop_source_preview_path_for(source, image.id);
        if !path.is_file() {
            bail!("review crop source is missing: {}", path.display());
        }
        Ok(path)
    }

    pub(super) fn profile_hald_path(&self, profile_index: usize) -> Result<PathBuf> {
        let store = self.store_snapshot();
        let profile = store
            .profiles
            .iter()
            .find(|profile| profile.index == profile_index)
            .ok_or_else(|| anyhow!("review profile {profile_index} does not exist"))?;
        let path = profile
            .hald_path
            .as_ref()
            .ok_or_else(|| anyhow!("review profile {profile_index} has no HALD"))?;
        if !path.is_file() {
            bail!("review profile HALD is missing: {}", path.display());
        }
        Ok(path.clone())
    }

    pub(super) fn profile_pp3_text(&self, image_id: u64, profile_index: usize) -> Result<String> {
        let store = self.store_snapshot();
        let image = store
            .images
            .iter()
            .find(|image| image.id == image_id)
            .ok_or_else(|| anyhow!("review image {image_id} does not exist"))?;
        if !image
            .profiles
            .iter()
            .any(|render| render.profile_index == profile_index)
        {
            bail!("review image {image_id} has no profile {profile_index}");
        }
        let profile = store
            .profiles
            .iter()
            .find(|profile| profile.index == profile_index)
            .ok_or_else(|| anyhow!("review profile {profile_index} does not exist"))?;
        let input = safe_existing_raw_source(&image.raw_path, &self.input_root)?;
        let temp_dir = Builder::new().prefix("mini-film-review-pp3-").tempdir()?;
        let apply_args = ApplyArgs {
            raw: input.clone(),
            output: self
                .cache_root
                .join(PROFILE_DETAILS_CACHE_DIR)
                .join("profile-details.jpg"),
            profile: optional_profile_selector(&profile.selector),
            hald_dir: self.hald_dir.clone(),
            profiles_root: self.profiles_root.clone(),
            hald_level: self.hald_level,
            rawtherapee: self.rawtherapee.clone(),
            dng_fallback: self.dng_fallback.clone(),
            convert: self.convert.clone(),
            lcp_root: self.lcp_root.clone(),
            keep_intermediate: None,
            no_grain: self.no_grain,
            normalize_grain_mpix: self.normalize_grain_mpix,
            color_noise_iso_threshold: self.color_noise_iso_threshold,
            lens_corrections: self.lens_corrections,
            grain: self.grain.clone(),
            grain_preset: self.grain_preset.clone(),
            grain_seed: self.grain_seed,
            grain_engine: self.grain_engine,
            export: self.export.clone(),
            retouch: None,
            retouch_white_balance: retouch_white_balance_for_image(image),
            bw_filter: effective_bw_filter_for_profile(image, profile),
        };
        let resolved = resolve_profile(&apply_args, temp_dir.path())?;
        let dcp_profile = is_raw_input_file(&input)
            .then(|| resolve_dcp_profile(&input, &self.dng_fallback))
            .flatten();
        let profiles = rawtherapee_profiles_for_input(
            RawTherapeeProfileOptions {
                input: &input,
                retouch: Some(&image.retouch),
                retouch_white_balance: retouch_white_balance_for_image(image),
                bw_filter: effective_bw_filter_for_profile(image, profile),
                color_noise_iso_threshold: self.color_noise_iso_threshold,
                lens_corrections: self.lens_corrections,
                dcp_profile: dcp_profile.as_ref(),
            },
            &resolved,
            temp_dir.path(),
        )?;
        rawtherapee_profile_chain_text(&profiles)
    }

    pub(super) fn full_media_path(&self, image_id: u64) -> Result<PathBuf> {
        let store = self.store_snapshot();
        let image = store
            .images
            .iter()
            .find(|image| image.id == image_id)
            .ok_or_else(|| anyhow!("review image {image_id} does not exist"))?;
        if image.preview.status != ReviewRenderStatus::Done {
            bail!("review preview is not ready");
        }
        let path = image
            .preview
            .path
            .as_ref()
            .ok_or_else(|| anyhow!("review preview has no output path"))?;
        if !path.is_file() {
            bail!("review preview is missing: {}", path.display());
        }
        Ok(path.clone())
    }

    pub(super) fn preview_media_path(&self, image_id: u64) -> Result<PathBuf> {
        let store = self.store_snapshot();
        let image = store
            .images
            .iter()
            .find(|image| image.id == image_id)
            .ok_or_else(|| anyhow!("review image {image_id} does not exist"))?;
        if !is_rendered_input_file(&image.raw_path) {
            if image.preview.status != ReviewRenderStatus::Done {
                bail!("review preview is not ready");
            }
            let path = image
                .preview
                .path
                .as_ref()
                .ok_or_else(|| anyhow!("review preview has no output path"))?;
            if !path.is_file() {
                bail!("review preview is missing: {}", path.display());
            }
            return Ok(path.clone());
        }

        let path = self.compressed_display_preview_path_for(&image.raw_path, image.id);
        if !path.is_file() {
            bail!("compressed review preview is missing: {}", path.display());
        }
        Ok(path)
    }

    pub(super) fn thumbnail_media_path(&self, image_id: u64) -> Result<PathBuf> {
        let store = self.store_snapshot();
        let image = store
            .images
            .iter()
            .find(|image| image.id == image_id)
            .ok_or_else(|| anyhow!("review image {image_id} does not exist"))?;
        if !is_rendered_input_file(&image.raw_path) {
            bail!("dedicated thumbnails are only available for compressed inputs");
        }
        let path = self.compressed_thumbnail_path_for(&image.raw_path, image.id);
        if !path.is_file() {
            bail!("compressed review thumbnail is missing: {}", path.display());
        }
        Ok(path)
    }

    pub(super) fn original_media_path(&self, image_id: u64) -> Result<PathBuf> {
        let store = self.store_snapshot();
        let image = store
            .images
            .iter()
            .find(|image| image.id == image_id)
            .ok_or_else(|| anyhow!("review image {image_id} does not exist"))?;
        if !is_rendered_input_file(&image.raw_path) {
            bail!("original download is only available for compressed inputs");
        }
        if !image.raw_path.is_file() {
            bail!("original media is missing: {}", image.raw_path.display());
        }
        Ok(image.raw_path.clone())
    }

    pub(super) fn rendered_full_preview_media_path(&self, image_id: u64) -> Result<PathBuf> {
        let store = self.store_snapshot();
        let image = store
            .images
            .iter()
            .find(|image| image.id == image_id)
            .ok_or_else(|| anyhow!("review image {image_id} does not exist"))?;
        if !is_tiff_input_file(&image.raw_path) {
            bail!("full JPEG proxies are only available for TIFF inputs");
        }
        let path = self.rendered_full_preview_path_for(&image.raw_path, image.id);
        ensure_rendered_full_preview(&image.raw_path, &path, &self.convert)?;
        Ok(path)
    }

    pub(super) fn start_publish_job(&self, request: PublishRequest) -> Result<ReviewPublishJob> {
        let args = self.publish_args_from_request(&request)?;
        let id = self.next_publish_job_id.fetch_add(1, Ordering::Relaxed);
        let job = ReviewPublishJob {
            id,
            album: args.album.clone(),
            status: ReviewPublishJobStatus::Running,
            started_at: now_string(),
            finished_at: None,
            processed: 0,
            total: 0,
            step: "starting".to_string(),
            current: None,
            linked: 0,
            skipped: 0,
            galleries: 0,
            gallery_urls: Vec::new(),
            error: None,
        };

        self.update_publish_jobs(|jobs| {
            jobs.push(job.clone());
            Ok(())
        })?;
        self.append_history(history_publish_started(&job, &args))?;
        self.broadcast_state()?;

        let handle = self.clone();
        thread::Builder::new()
            .name("mini-film-review-publish".to_string())
            .spawn(move || {
                let result = spawn_review_publish_command(&args, |progress| {
                    handle.record_publish_job_progress(job.id, &progress)
                })
                .and_then(|report| {
                    handle.record_publish_job_done(job.id, &report)?;
                    Ok(())
                });
                if let Err(error) = result {
                    let _ = handle.record_publish_job_failed(job.id, &format!("{error:#}"));
                }
            })
            .context("starting review publish job thread")?;

        Ok(job)
    }

    pub(super) fn publish_args_from_request(
        &self,
        request: &PublishRequest,
    ) -> Result<ReviewPublishCommandArgs> {
        let album = request
            .album
            .clone()
            .unwrap_or_else(|| self.publish_defaults.album.clone());
        let output_format = request
            .output_format
            .as_deref()
            .map(parse_batch_output_format)
            .transpose()?
            .unwrap_or(self.output_format);
        let gallery = if let Some(gallery) = request.gallery.as_deref() {
            parse_gallery_template(gallery)?
        } else {
            self.gallery.as_ref().map(|gallery| gallery.template)
        };
        let mut export = self.export.clone();
        if let Some(jpg_quality) = request.jpg_quality {
            export.jpg_quality = jpg_quality;
        }
        if let Some(size_mode) = request.size_mode.as_deref() {
            export.resize = None;
            export.long_edge = None;
            export.max_width = None;
            export.max_height = None;
            match size_mode {
                "original" => {}
                "long-edge" => export.long_edge = request.long_edge,
                "bounds" => {
                    export.max_width = request.max_width;
                    export.max_height = request.max_height;
                }
                "geometry" => {
                    export.resize = request
                        .resize
                        .clone()
                        .filter(|resize| !resize.trim().is_empty());
                }
                other => bail!("unsupported publish size mode {other:?}"),
            }
        } else {
            if request.resize.is_some() {
                export.resize = request
                    .resize
                    .clone()
                    .filter(|resize| !resize.trim().is_empty());
            }
            if request.long_edge.is_some() {
                export.long_edge = request.long_edge;
            }
            if request.max_width.is_some() {
                export.max_width = request.max_width;
            }
            if request.max_height.is_some() {
                export.max_height = request.max_height;
            }
        }
        if let Some(subsampling) = &request.jpeg_subsampling {
            export.jpeg_subsampling = parse_jpeg_subsampling(subsampling)?;
        }
        if let Some(strip_metadata) = request.strip_metadata {
            export.strip_metadata = strip_metadata;
        }
        if let Some(progressive_jpeg) = request.progressive_jpeg {
            export.progressive_jpeg = progressive_jpeg;
        }
        validate_export_options(&export)?;
        let grain_engine = request
            .grain_engine
            .as_deref()
            .map(parse_grain_engine)
            .transpose()?
            .unwrap_or(self.grain_engine);
        let normalize_grain_mpix = match request.normalize_grain {
            None => self.normalize_grain_mpix,
            Some(false) => None,
            Some(true) => {
                let reference_mpix = request
                    .normalize_grain_mpix
                    .or(self.normalize_grain_mpix)
                    .unwrap_or(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX);
                if !reference_mpix.is_finite() || reference_mpix <= 0.0 {
                    bail!(
                        "grain normalization reference MPix must be finite and greater than zero"
                    );
                }
                Some(reference_mpix)
            }
        };
        let rerender_raw = output_format != self.output_format
            || export != self.export
            || grain_engine != self.grain_engine
            || normalize_grain_mpix != self.normalize_grain_mpix;

        Ok(ReviewPublishCommandArgs {
            state: self.state_path.clone(),
            input_root: self.input_root.clone(),
            output_root: self.output_root.clone(),
            album,
            min_rating: request.min_rating.min(5),
            labels: request
                .labels
                .iter()
                .filter(|label| **label != ReviewLabel::None)
                .map(|label| review_label_name(*label).to_string())
                .collect(),
            tags: normalize_tags(request.tags.clone()),
            output_format,
            hald_dir: self.hald_dir.clone(),
            profiles_root: self.profiles_root.clone(),
            hald_level: self.hald_level,
            rawtherapee: self.rawtherapee.clone(),
            dng_fallback: self.dng_fallback.clone(),
            lcp_root: self.lcp_root.clone(),
            convert: self.convert.clone(),
            jobs: self.jobs,
            gallery,
            gallery_thumbnail_long_edge: request
                .gallery_thumbnail_long_edge
                .or_else(|| {
                    self.gallery
                        .as_ref()
                        .map(|gallery| gallery.thumbnail_long_edge)
                })
                .unwrap_or(1024),
            gallery_columns: request
                .gallery_columns
                .or_else(|| self.gallery.as_ref().map(|gallery| gallery.columns))
                .unwrap_or(4),
            rerender_raw,
            export,
            no_grain: self.no_grain,
            color_noise_iso_threshold: self.color_noise_iso_threshold,
            lens_corrections: self.lens_corrections,
            grain: self.grain.clone(),
            grain_preset: self.grain_preset.clone(),
            grain_seed: self.grain_seed,
            grain_engine,
            normalize_grain_mpix,
            progress_events: true,
        })
    }

    pub(super) fn record_publish_job_progress(
        &self,
        job_id: u64,
        progress: &ReviewPublishProgress,
    ) -> Result<()> {
        self.update_publish_job(job_id, |job| {
            job.processed = progress.processed;
            job.total = progress.total;
            job.step.clone_from(&progress.step);
            job.current.clone_from(&progress.current);
            job.linked = progress.linked;
            job.skipped = progress.skipped;
            job.galleries = progress.galleries;
        })
    }

    pub(super) fn record_publish_job_done(
        &self,
        job_id: u64,
        report: &PublishReport,
    ) -> Result<()> {
        self.update_publish_job(job_id, |job| {
            job.status = ReviewPublishJobStatus::Done;
            job.finished_at = Some(now_string());
            job.processed = report.linked;
            job.total = report.linked;
            job.step = "done".to_string();
            job.current = None;
            job.linked = report.linked;
            job.skipped = report.skipped;
            job.galleries = report.galleries;
            job.gallery_urls = self.publish_job_gallery_urls(report);
            job.error = None;
        })
    }

    fn publish_job_gallery_urls(&self, report: &PublishReport) -> Vec<String> {
        let output_root = self.output_root();
        let mut urls = Vec::new();
        for gallery_root in &report.gallery_roots {
            let index = gallery_root.join("index.html");
            if !index.is_file() {
                continue;
            }
            let Ok(relative_root) = gallery_root.strip_prefix(output_root) else {
                continue;
            };
            if relative_root.as_os_str().is_empty() {
                continue;
            }
            let route = Path::new("outputs").join(relative_root).join("index.html");
            let path = route.to_string_lossy().into_owned();
            if !urls.contains(&path) {
                urls.push(path);
            }
        }
        urls
    }

    pub(super) fn record_publish_job_failed(&self, job_id: u64, message: &str) -> Result<()> {
        self.update_publish_job(job_id, |job| {
            job.status = ReviewPublishJobStatus::Failed;
            job.finished_at = Some(now_string());
            job.step = "failed".to_string();
            job.current = None;
            job.error = Some(message.to_string());
        })
    }

    pub(super) fn update_publish_job<F>(&self, job_id: u64, update: F) -> Result<()>
    where
        F: Fn(&mut ReviewPublishJob),
    {
        let history_entry = self.update_publish_jobs(|jobs| {
            let Some(job) = jobs.iter_mut().find(|job| job.id == job_id) else {
                bail!("review publish job {job_id} does not exist");
            };
            let before = job.clone();
            update(job);
            let after = job.clone();
            let history_entry = history_publish_changed(&before, &after);
            if jobs.len() > 20 {
                let remove = jobs.len() - 20;
                jobs.drain(0..remove);
            }
            Ok(history_entry)
        })?;
        if let Some(entry) = history_entry {
            self.append_history(entry)?;
        }
        self.broadcast_state()
    }

    pub(super) fn publish_jobs_snapshot(&self) -> Result<Vec<ReviewPublishJob>> {
        Ok((**self.publish_jobs.load()).clone())
    }

    pub(super) fn gallery_archive_spec(&self, job_id: u64) -> Result<GalleryArchiveSpec> {
        let jobs = self.publish_jobs.load();
        let job = jobs
            .iter()
            .find(|job| job.id == job_id)
            .ok_or_else(|| anyhow!("review publish job {job_id} does not exist"))?;
        if job.status != ReviewPublishJobStatus::Done {
            bail!("review publish job {job_id} has not completed");
        }
        if job.galleries == 0 || job.gallery_urls.is_empty() {
            bail!("review publish job {job_id} did not create a gallery");
        }
        let album = validate_relative_publish_album(&job.album)?;
        Ok(GalleryArchiveSpec::new(
            &self.output_root,
            &self.cache_root,
            &album,
        ))
    }

    pub(super) fn update_publish_jobs<R, F>(&self, mut update: F) -> Result<R>
    where
        F: FnMut(&mut Vec<ReviewPublishJob>) -> Result<R>,
    {
        loop {
            let current = self.publish_jobs.load_full();
            let mut next = (*current).clone();
            let result = update(&mut next)?;
            let next = Arc::new(next);
            let previous = self
                .publish_jobs
                .compare_and_swap(&current, Arc::clone(&next));
            if Arc::ptr_eq(&previous, &current) {
                return Ok(result);
            }
        }
    }

    pub(super) fn subscribe(&self) -> broadcast::Receiver<String> {
        self.subscribers.subscribe()
    }

    pub(super) fn broadcast_state(&self) -> Result<()> {
        let current = self.api_state_value()?;
        let previous = self.state_cache.load_full();
        let message = previous
            .as_deref()
            .map(|previous| review_state_patch_value(previous, &current))
            .unwrap_or_else(|| review_state_patch_value(&current, &current));
        self.state_cache.store(Some(Arc::new(current)));
        let message =
            serde_json::to_string(&message).context("serializing review broadcast patch")?;
        let _ = self.subscribers.send(message);
        Ok(())
    }

    pub(super) fn refresh_state_cache(&self) -> Result<()> {
        let state = self.api_state_value()?;
        self.state_cache.store(Some(Arc::new(state)));
        Ok(())
    }

    pub(super) fn client_count(&self) -> Result<usize> {
        Ok(self.subscribers.receiver_count())
    }

    pub(super) fn store_snapshot(&self) -> Arc<ReviewStore> {
        self.state.load_full()
    }

    pub(crate) fn render_priority_snapshot(&self) -> ReviewRenderPrioritySnapshot {
        self.store_snapshot().render_priority_snapshot()
    }

    pub(crate) fn review_image_id_for(&self, raw: &Path) -> Option<u64> {
        self.store_snapshot()
            .images
            .iter()
            .find(|image| image.raw_path == raw)
            .map(|image| image.id)
    }

    pub(crate) fn review_raw_for_image_id(&self, image_id: u64) -> Option<PathBuf> {
        self.store_snapshot()
            .images
            .iter()
            .find(|image| image.id == image_id)
            .map(|image| image.raw_path.clone())
    }

    pub(crate) fn ensure_database_healthy(&self) -> Result<()> {
        if let Some(error) = self.database.health_error() {
            bail!("review database is unhealthy: {error}");
        }
        Ok(())
    }

    pub(super) fn update_store<R, F>(&self, update: F) -> Result<R>
    where
        F: FnOnce(&mut ReviewStore) -> Result<R>,
    {
        self.database_runtime
            .block_on(self.update_store_async(update))
    }

    pub(super) async fn update_store_async<R, F>(&self, update: F) -> Result<R>
    where
        F: FnOnce(&mut ReviewStore) -> Result<R>,
    {
        self.ensure_database_healthy()?;
        let _write_guard = self.database.write_lock.lock().await;
        self.ensure_database_healthy()?;
        let current = self.state.load_full();
        let mut next = (*current).clone();
        let result = update(&mut next)?;
        self.database.apply_delta(&current, &next).await?;
        self.state.store(Arc::new(next));
        Ok(result)
    }
}

fn normalize_panorama_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        bail!("panorama name cannot be empty");
    }
    let name = name.chars().take(120).collect::<String>();
    if name == "." || name == ".." {
        bail!("invalid panorama name");
    }
    Ok(name)
}

fn panorama_file_stem(name: &str, project_id: u64) -> String {
    let stem = name
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            character if character.is_control() => '-',
            character => character,
        })
        .collect::<String>();
    let stem = stem.trim().trim_matches('.');
    if stem.is_empty() {
        format!("Panorama-{project_id}")
    } else {
        stem.to_string()
    }
}

fn unique_panorama_output(
    input_root: &Path,
    name: &str,
    project_id: u64,
    projects: &[ReviewPanoramaProject],
) -> PathBuf {
    let root = input_root.join("Panoramas");
    let stem = panorama_file_stem(name, project_id);
    for suffix in 1_u32.. {
        let file_name = if suffix == 1 {
            format!("{stem}.tif")
        } else {
            format!("{stem}-{suffix}.tif")
        };
        let candidate = root.join(file_name);
        let reserved = projects.iter().any(|project| {
            project.id != project_id && project.output_path.as_deref() == Some(candidate.as_path())
        });
        if !candidate.exists() && !reserved {
            return candidate;
        }
    }
    unreachable!("panorama output suffix space is exhausted")
}

fn review_state_patch_value(
    previous: &serde_json::Value,
    current: &serde_json::Value,
) -> serde_json::Value {
    let mut patch = serde_json::Map::new();
    patch.insert("type".to_string(), json!("patch"));
    patch.insert(
        "version".to_string(),
        current
            .get("version")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    );

    for key in [
        "profiles",
        "client_count",
        "codex",
        "publish_defaults",
        "invocation",
        "publish_jobs",
        "capabilities",
        "panorama",
        "ui",
        "publish_root",
    ] {
        if previous.get(key) != current.get(key)
            && let Some(value) = current.get(key)
        {
            patch.insert(key.to_string(), value.clone());
        }
    }

    let previous_images = image_map(previous);
    let current_images = image_map(current);
    let changed_images = current
        .get("images")
        .and_then(|images| images.as_array())
        .into_iter()
        .flatten()
        .filter(|image| {
            image.get("id").and_then(|id| id.as_u64()).is_none_or(|id| {
                previous_images
                    .get(&id)
                    .is_none_or(|previous| *previous != *image)
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    let removed_image_ids = previous_images
        .keys()
        .filter(|id| !current_images.contains_key(id))
        .map(|id| json!(id))
        .collect::<Vec<_>>();

    if !changed_images.is_empty() || !removed_image_ids.is_empty() {
        patch.insert(
            "image_ids".to_string(),
            current
                .get("images")
                .and_then(|images| images.as_array())
                .map(|images| {
                    images
                        .iter()
                        .filter_map(|image| image.get("id").and_then(|id| id.as_u64()))
                        .map(|id| json!(id))
                        .collect::<Vec<_>>()
                })
                .map(serde_json::Value::Array)
                .unwrap_or_else(|| json!([])),
        );
    }
    if !changed_images.is_empty() {
        patch.insert(
            "images".to_string(),
            serde_json::Value::Array(changed_images),
        );
    }
    if !removed_image_ids.is_empty() {
        patch.insert(
            "removed_image_ids".to_string(),
            serde_json::Value::Array(removed_image_ids),
        );
    }

    serde_json::Value::Object(patch)
}

fn image_map(state: &serde_json::Value) -> HashMap<u64, &serde_json::Value> {
    state
        .get("images")
        .and_then(|images| images.as_array())
        .into_iter()
        .flatten()
        .filter_map(|image| {
            image
                .get("id")
                .and_then(|id| id.as_u64())
                .map(|id| (id, image))
        })
        .collect()
}

fn codex_analysis_key_for_image(image: &ReviewImage, config: &ReviewCodexConfig) -> Option<String> {
    codex_analysis_key_for_image_with_config(image, config)
}

fn codex_analysis_key_for_image_with_config(
    image: &ReviewImage,
    config: &ReviewCodexConfig,
) -> Option<String> {
    if !config.flags.is_enabled() || image.preview.status != ReviewRenderStatus::Done {
        return None;
    }
    let preview = image.preview.path.as_ref()?;
    if !preview.is_file() || !review_image_renders_terminal(image) {
        return None;
    }

    let mut hasher = Sha1::new();
    hasher.update(image.raw_path.to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(preview.to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(image.preview.updated_at.as_bytes());
    hasher.update(b"\0");
    hasher.update(config.flags.key().as_bytes());
    hasher.update(b"\0");
    hasher.update(config.model.as_bytes());
    let digest = hasher.finalize();
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Some(format!("codex-v1-{hex}"))
}

fn review_image_renders_terminal(image: &ReviewImage) -> bool {
    if image_is_direct_compressed(image) {
        return matches!(
            image.preview.status,
            ReviewRenderStatus::Done | ReviewRenderStatus::Failed
        ) && image.preview.render_key.is_none();
    }
    !image.profiles.is_empty()
        && image.profiles.iter().all(|render| {
            matches!(
                render.status,
                ReviewRenderStatus::Done | ReviewRenderStatus::Failed
            ) && render.render_key.is_none()
        })
}

#[derive(Default)]
struct ReviewCodexSummary {
    queued: u64,
    processing: u64,
    done: u64,
    failed: u64,
}

fn review_codex_summary(images: &[ReviewImage]) -> ReviewCodexSummary {
    let mut summary = ReviewCodexSummary::default();
    for image in images {
        match image.codex.status {
            ReviewCodexStatus::Queued => summary.queued += 1,
            ReviewCodexStatus::Processing => summary.processing += 1,
            ReviewCodexStatus::Done => summary.done += 1,
            ReviewCodexStatus::Failed => summary.failed += 1,
            ReviewCodexStatus::Missing | ReviewCodexStatus::Skipped => {}
        }
    }
    summary
}

pub(super) fn handle_gallery_defaults(
    gallery: &Option<ReviewGalleryConfig>,
) -> ReviewGalleryDefaults {
    if let Some(gallery) = gallery {
        return ReviewGalleryDefaults {
            template: Some(gallery.template),
            thumbnail_long_edge: gallery.thumbnail_long_edge,
            columns: gallery.columns,
        };
    }
    ReviewGalleryDefaults {
        template: None,
        thumbnail_long_edge: 1024,
        columns: 4,
    }
}

pub(super) fn retouch_temp_output(output: &Path, render_key: &str) -> PathBuf {
    let extension = output
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("jpg");
    let stem = output
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("review")
        .trim_start_matches('.');
    let stem = stem
        .rfind(RETOUCH_CACHE_MARKER)
        .map_or(stem, |index| &stem[..index]);
    output.with_file_name(format!(".{stem}.retouch-{render_key}.{extension}"))
}

const RETOUCH_CACHE_MARKER: &str = ".retouch-cache-";

pub(super) fn retouch_base_output(output: &Path, output_root: &Path, cache_root: &Path) -> PathBuf {
    let retouch_root = cache_root.join(RETOUCH_CACHE_DIR);
    let output = output.strip_prefix(&retouch_root).map_or_else(
        |_| output.to_path_buf(),
        |relative| output_root.join(relative),
    );
    let Some(stem) = output.file_stem().and_then(|stem| stem.to_str()) else {
        return output;
    };
    let Some(cache_stem) = stem.strip_prefix('.') else {
        return output;
    };
    let Some(marker_index) = cache_stem.rfind(RETOUCH_CACHE_MARKER) else {
        return output;
    };
    let base_stem = &cache_stem[..marker_index];
    if base_stem.is_empty() {
        return output;
    }
    let mut file_name = base_stem.to_string();
    if let Some(extension) = output.extension().and_then(|extension| extension.to_str())
        && !extension.is_empty()
    {
        file_name.push('.');
        file_name.push_str(extension);
    }
    output.with_file_name(file_name)
}

pub(super) fn retouch_cache_output(
    output: &Path,
    render_key: &str,
    output_root: &Path,
    cache_root: &Path,
) -> PathBuf {
    let output = retouch_base_output(output, output_root, cache_root);
    let relative = output
        .strip_prefix(output_root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| {
            output
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("review.jpg"))
        });
    let output = cache_root.join(RETOUCH_CACHE_DIR).join(relative);
    let extension = output
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("jpg");
    let stem = output
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("review");
    output.with_file_name(format!(
        ".{stem}{RETOUCH_CACHE_MARKER}{render_key}.{extension}"
    ))
}

pub(super) fn retouch_render_key(
    retouch: &RetouchSettings,
    export: &ExportOptions,
) -> Option<String> {
    let normalized = retouch.clone().normalized();
    (normalized != RetouchSettings::default()).then(|| {
        let mut hasher = Sha1::new();
        hasher.update(normalized.render_key());
        hasher.update("|");
        hasher.update(review_export_processing_identity(export));
        short_render_digest(hasher)
    })
}

pub(super) fn profile_render_key(
    retouch: &RetouchSettings,
    white_balance: RetouchWhiteBalance,
    bw_filter: BwFilter,
    normalize_grain_mpix: Option<f64>,
    processing_key: &str,
) -> Option<String> {
    let normalized = retouch.clone().normalized();
    (normalized != RetouchSettings::default() || bw_filter != BwFilter::None).then(|| {
        profile_render_key_value(
            &normalized,
            white_balance,
            bw_filter,
            normalize_grain_mpix,
            processing_key,
        )
    })
}

pub(super) fn profile_render_key_value(
    retouch: &RetouchSettings,
    white_balance: RetouchWhiteBalance,
    bw_filter: BwFilter,
    normalize_grain_mpix: Option<f64>,
    processing_key: &str,
) -> String {
    let normalized = retouch.clone().normalized();
    let retouch_key = normalized.render_key_with_white_balance(white_balance);
    let mut hasher = Sha1::new();
    hasher.update(retouch_key);
    if bw_filter != BwFilter::None {
        hasher.update("|bw-filter-v2=");
        hasher.update(bw_filter.as_str());
    }
    if normalize_grain_mpix.is_some() {
        hasher.update("|");
        hasher.update(grain_normalization_identity(normalize_grain_mpix));
    }
    hasher.update("|base=");
    hasher.update(processing_key);
    short_render_digest(hasher)
}

fn short_render_digest(hasher: Sha1) -> String {
    let digest = hasher.finalize();
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(super) fn retouch_white_balance_for_image(image: &ReviewImage) -> RetouchWhiteBalance {
    RetouchWhiteBalance {
        temperature: image.exif.white_balance_temperature,
        offset: image.exif.white_balance_offset,
    }
}

fn changed_bw_filter_profile_indexes(
    before: &[ReviewProfileBwFilter],
    after: &[ReviewProfileBwFilter],
    renders: &[ReviewProfileRender],
    profiles_by_index: &HashMap<usize, ReviewProfile>,
) -> Vec<usize> {
    renders
        .iter()
        .filter_map(|render| {
            let profile_index = render.profile_index;
            let before = effective_bw_filter_from_entries(before, profile_index, profiles_by_index);
            let after = effective_bw_filter_from_entries(after, profile_index, profiles_by_index);
            (before != after).then_some(profile_index)
        })
        .collect()
}

fn effective_bw_filter_from_entries(
    entries: &[ReviewProfileBwFilter],
    profile_index: usize,
    profiles_by_index: &HashMap<usize, ReviewProfile>,
) -> BwFilter {
    if !profiles_by_index
        .get(&profile_index)
        .is_some_and(review_profile_bw_filter_eligible)
    {
        return BwFilter::None;
    }
    entries
        .iter()
        .find(|entry| entry.profile_index == profile_index)
        .map(|entry| entry.filter)
        .unwrap_or_default()
}

fn bw_filter_summary(filter: BwFilter) -> String {
    match filter {
        BwFilter::None => String::new(),
        _ => format!(" bw_filter={}", filter.as_str()),
    }
}

fn profile_retouch_uses_base_output(retouch: &RetouchSettings, bw_filter: BwFilter) -> bool {
    retouch.clone().normalized() == RetouchSettings::default() && bw_filter == BwFilter::None
}

fn profile_retouch_output(
    output: &Path,
    render_key: &str,
    use_base_output: bool,
    output_root: &Path,
    cache_root: &Path,
) -> PathBuf {
    let base_output = retouch_base_output(output, output_root, cache_root);
    if use_base_output {
        base_output
    } else {
        retouch_cache_output(&base_output, render_key, output_root, cache_root)
    }
}

pub(super) fn apply_cached_profile_output(
    render: &mut ReviewProfileRender,
    output: &Path,
    render_key: &str,
    use_base_output: bool,
    output_root: &Path,
    cache_root: &Path,
) -> bool {
    if use_base_output && render.status != ReviewRenderStatus::Done {
        return false;
    }
    let output =
        profile_retouch_output(output, render_key, use_base_output, output_root, cache_root);
    if !output.is_file() {
        return false;
    }
    render.status = ReviewRenderStatus::Done;
    render.render_key = None;
    render.output_path = Some(output.clone());
    render.error = None;
    render.duration_ms = Some(0);
    refresh_review_render_dimensions(render, &output);
    true
}

fn apply_cached_preview_output(
    preview: &mut ReviewPreview,
    output: &Path,
    render_key: &str,
    output_root: &Path,
    cache_root: &Path,
) -> bool {
    let output = retouch_cache_output(output, render_key, output_root, cache_root);
    if !output.is_file() {
        return false;
    }
    preview.status = ReviewRenderStatus::Done;
    preview.render_key = None;
    preview.path = Some(output);
    preview.error = None;
    preview.duration_ms = Some(0);
    true
}

pub(super) fn queue_profile_retouch_render(
    image: &mut ReviewImage,
    render_index: usize,
    render_key: String,
    use_base_output: bool,
    retouch_jobs: &mut Vec<ReviewRetouchRequest>,
    output_root: &Path,
    cache_root: &Path,
) {
    let output = image.profiles[render_index]
        .output_path
        .as_ref()
        .map(|output| retouch_base_output(output, output_root, cache_root));
    let render = &mut image.profiles[render_index];
    if let Some(output) = output.as_deref()
        && apply_cached_profile_output(
            render,
            output,
            &render_key,
            use_base_output,
            output_root,
            cache_root,
        )
    {
        render.updated_at = now_string();
        return;
    }
    render.status = ReviewRenderStatus::Queued;
    render.error = None;
    render.duration_ms = None;
    render.render_key = Some(render_key.clone());
    render.updated_at = now_string();
    if let Some(output) = output {
        retouch_jobs.push(ReviewRetouchRequest {
            image_id: image.id,
            raw: image.raw_path.clone(),
            profile_index: Some(render.profile_index),
            output,
            render_key,
        });
    }
}

pub(super) fn retouch_without_adjustments(retouch: &RetouchSettings) -> RetouchSettings {
    let mut retouch = retouch.clone().normalized();
    retouch.adjustments = BasicRetouchAdjustments::default();
    retouch
}

pub(super) fn apply_base_render_done(
    render: &mut ReviewProfileRender,
    output: &Path,
    duration: Duration,
) -> Option<String> {
    refresh_review_render_dimensions(render, output);
    if render.render_key.is_some()
        && matches!(
            render.status,
            ReviewRenderStatus::Queued | ReviewRenderStatus::Processing
        )
    {
        render.output_path = Some(output.to_path_buf());
        render.error = None;
        render.duration_ms = Some(duration.as_millis() as u64);
        return render.render_key.clone();
    }
    render.status = ReviewRenderStatus::Done;
    render.output_path = Some(output.to_path_buf());
    render.error = None;
    render.duration_ms = Some(duration.as_millis() as u64);
    None
}

pub(super) fn apply_profile_retouch_done(
    render: &mut ReviewProfileRender,
    output: &Path,
    duration: Duration,
    dcp_profile_filename: Option<&str>,
) {
    render.status = ReviewRenderStatus::Done;
    render.render_key = None;
    render.output_path = Some(output.to_path_buf());
    render.error = None;
    render.duration_ms = Some(duration.as_millis() as u64);
    render.dcp_profile_filename = dcp_profile_filename.map(str::to_string);
    refresh_review_render_dimensions(render, output);
}

pub(super) fn effective_dcp_profile_filename<'a>(
    image: &'a ReviewImage,
    render: &'a ReviewProfileRender,
) -> Option<&'a str> {
    if render.profile_index == SOOC_PROFILE_INDEX || !is_raw_input_file(&image.raw_path) {
        return None;
    }
    if let Some(filename) = render.dcp_profile_filename.as_deref() {
        return Some(filename);
    }
    let processing_key = render.processing_key.as_deref()?;
    image
        .profiles
        .iter()
        .filter(|candidate| {
            candidate.status == ReviewRenderStatus::Done
                && candidate.profile_index != SOOC_PROFILE_INDEX
                && candidate.processing_key.as_deref() == Some(processing_key)
        })
        .find_map(|candidate| candidate.dcp_profile_filename.as_deref())
}

fn refresh_review_render_dimensions(render: &mut ReviewProfileRender, output: &Path) {
    match image::image_dimensions(output) {
        Ok((width, height)) => {
            render.width = Some(width);
            render.height = Some(height);
        }
        Err(_) => {
            render.width = None;
            render.height = None;
        }
    }
}

pub(super) fn apply_base_preview_done(
    preview: &mut ReviewPreview,
    output: &Path,
    duration: Duration,
) -> Option<String> {
    if preview.render_key.is_some()
        && matches!(
            preview.status,
            ReviewRenderStatus::Queued | ReviewRenderStatus::Processing
        )
    {
        preview.path = Some(output.to_path_buf());
        preview.error = None;
        preview.duration_ms = Some(duration.as_millis() as u64);
        return preview.render_key.clone();
    }
    preview.status = ReviewRenderStatus::Done;
    preview.path = Some(output.to_path_buf());
    preview.error = None;
    preview.duration_ms = Some(duration.as_millis() as u64);
    None
}
