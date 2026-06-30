use super::{
    db::*, history::*, model::*, prelude::*, preview::*, publish::*, scheduler::*, server::*,
    store::*,
};

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
    let (mut store, state_path) = load_or_migrate_store(&config.output_root)?;
    let needs_exif_schema_refresh = store.needs_exif_schema_refresh();
    store.sync_profiles(config.profiles);
    if needs_exif_schema_refresh {
        store.refresh_missing_exif_data_for_schema();
        store.mark_exif_schema_refreshed();
    } else {
        store.refresh_missing_exif_data();
    }
    save_store(&state_path, &store)?;
    let history_profiles = store.profiles.clone();

    let gallery_defaults = handle_gallery_defaults(&config.gallery);
    let publish_defaults = ReviewPublishDefaults::new(
        config.publish_album,
        config.output_format,
        &config.export,
        gallery_defaults,
        config.grain_engine,
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
        input_root: config.input_root,
        output_root: config.output_root,
        hald_dir: config.hald_dir,
        profiles_root: config.profiles_root,
        hald_level: config.hald_level,
        rawtherapee: config.rawtherapee,
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
        publish_defaults,
        publish_jobs: Arc::new(ArcSwap::from_pointee(Vec::new())),
        next_publish_job_id: Arc::new(AtomicU64::new(1)),
        retouch_scheduler: Arc::new(ReviewRetouchScheduler::default()),
        codex,
        codex_scheduler: Arc::new(ReviewCodexScheduler::default()),
        invocation: config.invocation,
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
    handle.start_retouch_scheduler()?;
    handle.start_codex_scheduler()?;
    handle.schedule_ready_codex_jobs()?;

    Ok(handle)
}

impl ReviewHandle {
    pub(crate) fn state_path(&self) -> &Path {
        &self.state_path
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

    pub(super) fn preview_root(&self) -> PathBuf {
        self.output_root.join(".mini-film-review-previews")
    }

    pub(super) fn preview_path_for(&self, raw: &Path, image_id: u64) -> PathBuf {
        self.preview_root()
            .join(format!("{image_id:08}-{}.jpg", short_path_sha1(raw)))
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
        let (history_entry, preview_job) = self.update_store(|store| {
            let mut preview_job = None;
            let mut history_entry = None;
            let discovered = !store.images.iter().any(|image| image.raw_path == raw);
            let profiles = store.profiles.clone();
            let image = store.ensure_image(&self.input_root, raw)?;
            let old_sidecar = image.sooc_sidecar_path.clone();
            image.sooc_sidecar_path = sooc_sidecar.map(Path::to_path_buf);
            if image.sooc_sidecar_path != old_sidecar {
                sync_image_profile_renders(image, &profiles, false, &HashSet::new());
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
                history_entry = Some(history_image_discovered(image, discovered, preview_queued));
            }
            Ok((history_entry, preview_job))
        })?;
        if let Some(entry) = history_entry {
            self.append_history(entry)?;
        }
        self.broadcast_state()?;
        if let Some((raw, preview_path)) = preview_job {
            self.spawn_preview_job(raw, preview_path);
        }
        Ok(())
    }

    pub(crate) fn record_compressed_queued(
        &self,
        input: &Path,
        expected_output: &Path,
    ) -> Result<()> {
        let history_entries = self.update_store(|store| {
            let mut history_entries = Vec::new();
            let discovered = !store.images.iter().any(|image| image.raw_path == input);
            let image = store.ensure_image(&self.input_root, input)?;
            let before = image.preview.clone();
            image.preview.status = ReviewRenderStatus::Queued;
            image.preview.path = Some(expected_output.to_path_buf());
            image.preview.error = None;
            image.preview.duration_ms = None;
            image.preview.render_key = retouch_render_key(&image.retouch);
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
        for entry in history_entries {
            self.append_history(entry)?;
        }
        self.broadcast_state()
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
        });
        if result.is_ok()
            && let Some(render_key) = pending_retouch_key
        {
            self.schedule_retouch_job(input.to_path_buf(), None, output.to_path_buf(), render_key);
        }
        if result.is_ok() {
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
        raw: &Path,
        render_key: &str,
        mut update: F,
    ) -> Result<bool>
    where
        F: FnMut(&mut ReviewPreview),
    {
        let (updated, history_entry) = self.update_store(|store| {
            let mut updated = false;
            let mut history_entry = None;
            let image = store.ensure_image(&self.input_root, raw)?;
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
    ) -> Result<()> {
        let history_entry = self.update_store(|store| {
            let profile = store
                .profiles
                .iter()
                .find(|profile| profile.index == profile_index)
                .cloned();
            let image = store.ensure_image(&self.input_root, raw)?;
            let bw_filter = profile
                .as_ref()
                .map(|profile| effective_bw_filter_for_profile(image, profile))
                .unwrap_or_default();
            let render_key = profile_render_key(&image.retouch, bw_filter);
            let Some(render) = image
                .profiles
                .iter_mut()
                .find(|render| render.profile_index == profile_index)
            else {
                bail!("review profile index {profile_index} is not configured");
            };
            let before = render.clone();
            render.status = ReviewRenderStatus::Queued;
            render.output_path = Some(expected_output.to_path_buf());
            render.error = None;
            render.duration_ms = None;
            render.render_key = render_key;
            render.width = None;
            render.height = None;
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

    pub(crate) fn record_profile_processing(&self, raw: &Path, profile_index: usize) -> Result<()> {
        self.update_render(raw, profile_index, |render| {
            if render.render_key.is_some() {
                render.error = None;
                return;
            }
            render.status = ReviewRenderStatus::Processing;
            render.error = None;
        })
    }

    pub(crate) fn record_profile_done(
        &self,
        raw: &Path,
        profile_index: usize,
        output: &Path,
        duration: Duration,
    ) -> Result<()> {
        let mut pending_retouch_key = None;
        let result = self.update_render(raw, profile_index, |render| {
            pending_retouch_key = apply_base_render_done(render, output, duration);
            if let Some(render_key) = pending_retouch_key.as_deref()
                && apply_cached_profile_output(render, output, render_key, false)
            {
                pending_retouch_key = None;
            }
        });
        if result.is_ok()
            && let Some(render_key) = pending_retouch_key
        {
            self.schedule_retouch_job(
                raw.to_path_buf(),
                Some(profile_index),
                output.to_path_buf(),
                render_key,
            );
        }
        if result.is_ok() {
            self.maybe_schedule_codex_for_raw(raw)?;
        }
        result
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
        });
        if result.is_ok() {
            self.maybe_schedule_codex_for_raw(raw)?;
        }
        result
    }

    pub(super) fn update_render<F>(
        &self,
        raw: &Path,
        profile_index: usize,
        mut update: F,
    ) -> Result<()>
    where
        F: FnMut(&mut ReviewProfileRender),
    {
        let history_entry = self.update_store(|store| {
            let image = store.ensure_image(&self.input_root, raw)?;
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
        raw: &Path,
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
            let image = store.ensure_image(&self.input_root, raw)?;
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
        raw: &Path,
        profile_index: usize,
        render_key: &str,
    ) -> Result<Option<(ReviewProfile, RetouchSettings, BwFilter)>> {
        let store = self.store_snapshot();
        let Some(image) = store.images.iter().find(|image| image.raw_path == raw) else {
            return Ok(None);
        };
        let Some(render) = image
            .profiles
            .iter()
            .find(|render| render.profile_index == profile_index)
        else {
            return Ok(None);
        };
        if render.render_key.as_deref() != Some(render_key) {
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
        Ok(Some((profile, image.retouch.clone(), bw_filter)))
    }

    pub(super) fn sooc_retouch_task_snapshot(
        &self,
        raw: &Path,
        render_key: &str,
    ) -> Result<Option<(PathBuf, RetouchSettings)>> {
        let store = self.store_snapshot();
        let Some(image) = store.images.iter().find(|image| image.raw_path == raw) else {
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
        let Some(sidecar) = image
            .sooc_sidecar_path
            .clone()
            .filter(|path| path.is_file())
        else {
            return Ok(None);
        };
        Ok(Some((sidecar, retouch_without_adjustments(&image.retouch))))
    }

    pub(super) fn compressed_retouch_task_snapshot(
        &self,
        input: &Path,
        render_key: &str,
    ) -> Result<Option<RetouchSettings>> {
        let store = self.store_snapshot();
        let Some(image) = store.images.iter().find(|image| image.raw_path == input) else {
            return Ok(None);
        };
        if !is_jpeg_input_file(&image.raw_path) {
            return Ok(None);
        }
        if image.preview.render_key.as_deref() != Some(render_key) {
            return Ok(None);
        }
        Ok(Some(image.retouch.clone()))
    }

    pub(super) fn schedule_retouch_job(
        &self,
        raw: PathBuf,
        profile_index: Option<usize>,
        output: PathBuf,
        render_key: String,
    ) {
        self.retouch_scheduler
            .schedule(raw, profile_index, output, render_key);
    }

    pub(super) fn start_retouch_scheduler(&self) -> Result<()> {
        let handle = self.clone();
        thread::Builder::new()
            .name("mini-film-review-retouch".to_string())
            .spawn(move || {
                loop {
                    let job = handle.retouch_scheduler.next_job();
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
        let handle = self.clone();
        thread::Builder::new()
            .name("mini-film-review-codex".to_string())
            .spawn(move || {
                loop {
                    let job = handle.codex_scheduler.next_job();
                    handle.run_scheduled_codex_job(job);
                }
            })
            .context("starting review Codex scheduler thread")?;
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
                model: config.model.clone(),
                timeout: config.timeout,
                flags: config.flags,
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

    pub(super) fn run_scheduled_retouch_job(&self, job: ScheduledRetouchJob) {
        if job.profile_index == Some(SOOC_PROFILE_INDEX) {
            self.run_scheduled_sooc_retouch_job(job);
            return;
        }
        if job.profile_index.is_none() {
            self.run_scheduled_compressed_retouch_job(job);
            return;
        }
        let profile_index = job.profile_index.expect("profile retouch job has an index");
        let Ok(Some((profile, retouch, bw_filter))) =
            self.retouch_task_snapshot(&job.raw, profile_index, &job.render_key)
        else {
            return;
        };
        let use_base_output = profile_retouch_uses_base_output(&retouch, bw_filter);
        let started = Instant::now();
        let mut cached = false;
        let Ok(updated) =
            self.update_render_if_key(&job.raw, profile_index, &job.render_key, |render| {
                cached = apply_cached_profile_output(
                    render,
                    &job.output,
                    &job.render_key,
                    use_base_output,
                );
                if !cached {
                    render.status = ReviewRenderStatus::Processing;
                    render.error = None;
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
        let final_output = profile_retouch_output(&job.output, &job.render_key, use_base_output);
        let temp_output = retouch_temp_output(&final_output, &job.render_key);
        let result = self.render_retouch_output(
            &job.raw,
            &profile,
            profile_index,
            &retouch,
            bw_filter,
            &temp_output,
        );
        match result {
            Ok(()) => match self.retouch_task_snapshot(&job.raw, profile_index, &job.render_key) {
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
                        &job.raw,
                        profile_index,
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

    pub(super) fn run_scheduled_sooc_retouch_job(&self, job: ScheduledRetouchJob) {
        let Ok(Some((sidecar, retouch))) =
            self.sooc_retouch_task_snapshot(&job.raw, &job.render_key)
        else {
            return;
        };
        let started = Instant::now();
        let _ =
            self.update_render_if_key(&job.raw, SOOC_PROFILE_INDEX, &job.render_key, |render| {
                render.status = ReviewRenderStatus::Processing;
                render.error = None;
            });
        let temp_output = retouch_temp_output(&job.output, &job.render_key);
        let result = self.render_sooc_retouch_output(&sidecar, &retouch, &temp_output);
        match result {
            Ok(()) => match self.sooc_retouch_task_snapshot(&job.raw, &job.render_key) {
                Ok(Some(_)) => {
                    if let Some(parent) = job.output.parent()
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
                    if let Err(error) = fs::rename(&temp_output, &job.output) {
                        self.record_retouch_render_failed(
                            &job,
                            &temp_output,
                            started,
                            error.to_string(),
                        );
                        return;
                    }
                    let _ = self.update_render_if_key(
                        &job.raw,
                        SOOC_PROFILE_INDEX,
                        &job.render_key,
                        |render| {
                            render.status = ReviewRenderStatus::Done;
                            render.render_key = None;
                            render.output_path = Some(job.output.clone());
                            render.error = None;
                            render.duration_ms = Some(started.elapsed().as_millis() as u64);
                            refresh_review_render_dimensions(render, &job.output);
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

    pub(super) fn run_scheduled_compressed_retouch_job(&self, job: ScheduledRetouchJob) {
        let Ok(Some(retouch)) = self.compressed_retouch_task_snapshot(&job.raw, &job.render_key)
        else {
            return;
        };
        let started = Instant::now();
        let _ = self.update_preview_if_key(&job.raw, &job.render_key, |preview| {
            preview.status = ReviewRenderStatus::Processing;
            preview.error = None;
        });
        let temp_output = retouch_temp_output(&job.output, &job.render_key);
        let result = self.render_compressed_retouch_output(&job.raw, &retouch, &temp_output);
        match result {
            Ok(()) => match self.compressed_retouch_task_snapshot(&job.raw, &job.render_key) {
                Ok(Some(_)) => {
                    if let Some(parent) = job.output.parent()
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
                    if let Err(error) = fs::rename(&temp_output, &job.output) {
                        self.record_retouch_render_failed(
                            &job,
                            &temp_output,
                            started,
                            error.to_string(),
                        );
                        return;
                    }
                    let _ = self.update_preview_if_key(&job.raw, &job.render_key, |preview| {
                        preview.status = ReviewRenderStatus::Done;
                        preview.render_key = None;
                        preview.path = Some(job.output.clone());
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
            let _ = self.update_render_if_key(&job.raw, profile_index, &job.render_key, |render| {
                render.status = ReviewRenderStatus::Failed;
                render.render_key = None;
                render.error = Some(message.clone());
                render.duration_ms = Some(started.elapsed().as_millis() as u64);
            });
        } else {
            let _ = self.update_preview_if_key(&job.raw, &job.render_key, |preview| {
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
        profile_index: usize,
        retouch: &RetouchSettings,
        bw_filter: BwFilter,
        output: &Path,
    ) -> Result<()> {
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
            convert: self.convert.clone(),
            lcp_root: self.lcp_root.clone(),
            keep_intermediate: None,
            no_grain: self.no_grain,
            color_noise_iso_threshold: self.color_noise_iso_threshold,
            lens_corrections: self.lens_corrections,
            grain: self.grain.clone(),
            grain_preset: self.grain_preset.clone(),
            grain_seed: self.grain_seed,
            grain_engine: self.grain_engine,
            export: self.export.clone(),
            retouch: None,
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
            .map(|seed| review_publish_seed(seed, &raw, profile_index))
            .unwrap_or_else(|| review_publish_seed(0, &raw, profile_index));
        apply_resolved(
            ApplyJob {
                raw: &raw,
                output,
                rawtherapee: &self.rawtherapee,
                convert: &self.convert,
                keep_intermediate: None,
                no_grain: self.no_grain,
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
                bw_filter,
            },
            &resolved,
            seed,
            temp_dir.path(),
            None,
        )
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

    pub(super) fn apply_review_update(&self, update: ReviewUpdateRequest) -> Result<()> {
        let (history_entries, retouch_jobs) = self.update_store(|store| {
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
                let compressed = is_jpeg_input_file(&image.raw_path);
                if let Some(selected_profile_index) = update.selected_profile_index
                    && !compressed
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
                if let Some(retouch) = update.retouch.clone() {
                    image.retouch = retouch.normalized();
                }
                if let Some(selected_profile_index) =
                    update.selected_profile_index.filter(|_| !compressed)
                {
                    image.selected_profile_index = selected_profile_index;
                }
                if let Some(indexes) = update.publish_profile_indexes.clone() {
                    if compressed {
                        image.publish_profile_indexes = Some(Vec::new());
                    } else {
                        validate_publish_profile_indexes(&indexes, &image.profiles)?;
                        image.publish_profile_indexes =
                            Some(normalize_publish_profile_indexes(&indexes, &image.profiles));
                    }
                }
                let mut changed_bw_profile_indexes = Vec::new();
                if let Some(filters) = update.profile_bw_filters.clone() {
                    let normalized_filters = if compressed {
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
                    if compressed {
                        let render_key = image.retouch.render_key();
                        image.preview.status = ReviewRenderStatus::Queued;
                        image.preview.error = None;
                        image.preview.duration_ms = None;
                        image.preview.render_key = Some(render_key.clone());
                        image.preview.updated_at = now_string();
                        if let Some(output) = &image.preview.path {
                            retouch_jobs.push((
                                image.raw_path.clone(),
                                None,
                                output.clone(),
                                render_key.clone(),
                            ));
                        }
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
                            .map(|(index, render)| {
                                let priority =
                                    if Some(render.profile_index) == visible_profile_index {
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
                            let bw_filter = profiles_by_index
                                .get(&profile_index)
                                .map(|profile| effective_bw_filter_for_profile(image, profile))
                                .unwrap_or_default();
                            let render_key = profile_render_key_value(&image.retouch, bw_filter);
                            queue_profile_retouch_render(
                                image,
                                index,
                                render_key,
                                profile_retouch_uses_base_output(&image.retouch, bw_filter),
                                &mut retouch_jobs,
                            );
                        }
                    }
                } else if !changed_bw_profile_indexes.is_empty() && !compressed {
                    let changed_indexes = changed_bw_profile_indexes
                        .iter()
                        .copied()
                        .collect::<HashSet<_>>();
                    for index in 0..image.profiles.len() {
                        let profile_index = image.profiles[index].profile_index;
                        if !changed_indexes.contains(&profile_index) {
                            continue;
                        }
                        let Some(profile) = profiles_by_index.get(&profile_index) else {
                            continue;
                        };
                        let bw_filter = effective_bw_filter_for_profile(image, profile);
                        let render_key = profile_render_key_value(&image.retouch, bw_filter);
                        queue_profile_retouch_render(
                            image,
                            index,
                            render_key,
                            profile_retouch_uses_base_output(&image.retouch, bw_filter),
                            &mut retouch_jobs,
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
        })?;
        for entry in history_entries {
            self.append_history(entry)?;
        }
        self.broadcast_state()?;
        for (raw, profile_index, output, render_key) in retouch_jobs {
            self.schedule_retouch_job(raw, profile_index, output, render_key);
        }
        Ok(())
    }

    pub(super) fn apply_ui_update(&self, update: ReviewUiUpdateRequest) -> Result<()> {
        let history_entry = self.update_store(|store| {
            let before_ui = store.ui.clone();
            store.set_ui(update)?;
            Ok(history_ui_changed(store, &before_ui, &store.ui))
        })?;
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
        let codex_summary = review_codex_summary(&images);
        let images = images
            .iter()
            .map(|image| {
                let mut exif = image.exif.clone();
                exif.sanitize_text_fields();
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
                        json!({
                            "profile_index": render.profile_index,
                            "profile_stem": render.profile_stem,
                            "display_name": render.display_name,
                            "status": render.status,
                            "url": if render.status == ReviewRenderStatus::Done {
                                Some(format!("media/{}/{}", image.id, render.profile_index))
                            } else {
                                None
                            },
                            "error": render.error,
                            "duration_ms": render.duration_ms,
                            "width": render.width,
                            "height": render.height,
                            "retouch_pending": render.render_key.is_some(),
                            "bw_filter_eligible": bw_filter_eligible,
                            "bw_filter": bw_filter,
                            "updated_at": render.updated_at,
                        })
                    })
                    .collect::<Vec<_>>();
                json!({
                    "id": image.id,
                    "source_type": if is_jpeg_input_file(&image.raw_path) { "compressed" } else { "raw" },
                    "relative_path": image.relative_path,
                    "file_name": image.file_name,
                    "exif": exif,
                    "preview_status": image.preview.status,
                    "preview_url": if image.preview.status == ReviewRenderStatus::Done {
                        Some(format!("preview/{}", image.id))
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

    pub(super) fn preview_media_path(&self, image_id: u64) -> Result<PathBuf> {
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
        let rerender_raw = output_format != self.output_format
            || export != self.export
            || grain_engine != self.grain_engine;

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

    pub(super) fn update_store<R, F>(&self, mut update: F) -> Result<R>
    where
        F: FnMut(&mut ReviewStore) -> Result<R>,
    {
        loop {
            let current = self.state.load_full();
            let mut next = (*current).clone();
            let result = update(&mut next)?;
            let next = Arc::new(next);
            let previous = self.state.compare_and_swap(&current, Arc::clone(&next));
            if Arc::ptr_eq(&previous, &current) {
                save_store(&self.state_path, &next)?;
                return Ok(result);
            }
        }
    }
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
    if is_jpeg_input_file(&image.raw_path) {
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
    let output = retouch_base_output(output);
    let extension = output
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("jpg");
    let stem = output
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("review");
    output.with_file_name(format!(".{stem}.retouch-{render_key}.{extension}"))
}

const RETOUCH_CACHE_MARKER: &str = ".retouch-cache-";

pub(super) fn retouch_base_output(output: &Path) -> PathBuf {
    let Some(stem) = output.file_stem().and_then(|stem| stem.to_str()) else {
        return output.to_path_buf();
    };
    let Some(cache_stem) = stem.strip_prefix('.') else {
        return output.to_path_buf();
    };
    let Some(marker_index) = cache_stem.rfind(RETOUCH_CACHE_MARKER) else {
        return output.to_path_buf();
    };
    let base_stem = &cache_stem[..marker_index];
    if base_stem.is_empty() {
        return output.to_path_buf();
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

pub(super) fn retouch_cache_output(output: &Path, render_key: &str) -> PathBuf {
    let output = retouch_base_output(output);
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

pub(super) fn retouch_render_key(retouch: &RetouchSettings) -> Option<String> {
    let normalized = retouch.clone().normalized();
    (normalized != RetouchSettings::default()).then(|| normalized.render_key())
}

pub(super) fn profile_render_key(retouch: &RetouchSettings, bw_filter: BwFilter) -> Option<String> {
    let normalized = retouch.clone().normalized();
    (normalized != RetouchSettings::default() || bw_filter != BwFilter::None)
        .then(|| profile_render_key_value(&normalized, bw_filter))
}

pub(super) fn profile_render_key_value(retouch: &RetouchSettings, bw_filter: BwFilter) -> String {
    let normalized = retouch.clone().normalized();
    if bw_filter == BwFilter::None {
        return normalized.render_key();
    }
    let mut hasher = Sha1::new();
    hasher.update(normalized.render_key());
    hasher.update("|bw-filter-v2=");
    hasher.update(bw_filter.as_str());
    let digest = hasher.finalize();
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
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

fn profile_retouch_output(output: &Path, render_key: &str, use_base_output: bool) -> PathBuf {
    let base_output = retouch_base_output(output);
    if use_base_output {
        base_output
    } else {
        retouch_cache_output(&base_output, render_key)
    }
}

fn apply_cached_profile_output(
    render: &mut ReviewProfileRender,
    output: &Path,
    render_key: &str,
    use_base_output: bool,
) -> bool {
    let output = profile_retouch_output(output, render_key, use_base_output);
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

fn queue_profile_retouch_render(
    image: &mut ReviewImage,
    render_index: usize,
    render_key: String,
    use_base_output: bool,
    retouch_jobs: &mut Vec<(PathBuf, Option<usize>, PathBuf, String)>,
) {
    let output = image.profiles[render_index]
        .output_path
        .as_ref()
        .map(|output| retouch_base_output(output));
    let render = &mut image.profiles[render_index];
    if let Some(output) = output.as_deref()
        && apply_cached_profile_output(render, output, &render_key, use_base_output)
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
        retouch_jobs.push((
            image.raw_path.clone(),
            Some(render.profile_index),
            output,
            render_key,
        ));
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
