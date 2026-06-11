use super::{
    history::*, model::*, prelude::*, preview::*, publish::*, scheduler::*, server::*, store::*,
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
    let state_path = config.output_root.join("mini-film-review.json");
    let mut store = load_store(&state_path)?.unwrap_or_else(|| ReviewStore::new(Vec::new()));
    store.sync_profiles(config.profiles);
    save_store(&state_path, &store)?;
    let history_profiles = store.profiles.clone();

    let gallery_defaults = handle_gallery_defaults(&config.gallery);
    let publish_defaults = ReviewPublishDefaults::new(
        config.publish_album,
        config.output_format,
        &config.export,
        gallery_defaults,
    );
    let (subscribers, _) = broadcast::channel(256);
    let handle = ReviewHandle {
        state: Arc::new(Mutex::new(store)),
        subscribers: Arc::new(subscribers),
        state_path,
        input_root: config.input_root,
        output_root: config.output_root,
        hald_dir: config.hald_dir,
        profiles_root: config.profiles_root,
        hald_level: config.hald_level,
        rawtherapee: config.rawtherapee,
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
        publish_defaults,
        publish_jobs: Arc::new(Mutex::new(Vec::new())),
        next_publish_job_id: Arc::new(Mutex::new(1)),
        retouch_scheduler: Arc::new(ReviewRetouchScheduler::default()),
    };
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

    pub(super) fn preview_root(&self) -> PathBuf {
        self.output_root.join(".mini-film-review-previews")
    }

    pub(super) fn preview_path_for(&self, raw: &Path, image_id: u64) -> PathBuf {
        self.preview_root()
            .join(format!("{image_id:08}-{}.jpg", short_path_sha1(raw)))
    }

    pub(crate) fn record_discovered_raw(&self, raw: &Path) -> Result<()> {
        let mut preview_job = None;
        let mut history_entry = None;
        let mut store = self.lock_store()?;
        let discovered = !store.images.iter().any(|image| image.raw_path == raw);
        let image = store.ensure_image(&self.input_root, raw)?;
        let preview_path = self.preview_path_for(raw, image.id);
        let mut preview_queued = false;
        if !preview_path.is_file()
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
        if discovered || preview_queued {
            history_entry = Some(history_image_discovered(image, discovered, preview_queued));
        }
        save_store(&self.state_path, &store)?;
        drop(store);
        if let Some(entry) = history_entry {
            self.append_history(entry)?;
        }
        self.broadcast_state()?;
        if let Some((raw, preview_path)) = preview_job {
            self.spawn_preview_job(raw, preview_path);
        }
        Ok(())
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
        self.update_preview(raw, |preview| {
            preview.status = ReviewRenderStatus::Done;
            preview.path = Some(output.to_path_buf());
            preview.error = None;
        })
    }

    pub(super) fn record_preview_failed(&self, raw: &Path, error: &str) -> Result<()> {
        self.update_preview(raw, |preview| {
            preview.status = ReviewRenderStatus::Failed;
            preview.error = Some(error.to_string());
        })
    }

    pub(super) fn update_preview<F>(&self, raw: &Path, update: F) -> Result<()>
    where
        F: FnOnce(&mut ReviewPreview),
    {
        let mut store = self.lock_store()?;
        let image = store.ensure_image(&self.input_root, raw)?;
        let before = image.preview.clone();
        update(&mut image.preview);
        image.preview.updated_at = now_string();
        image.updated_at = now_string();
        let history_entry = history_preview_changed(image, &before, &image.preview);
        save_store(&self.state_path, &store)?;
        drop(store);
        if let Some(entry) = history_entry {
            self.append_history(entry)?;
        }
        self.broadcast_state()
    }

    pub(crate) fn record_profile_queued(
        &self,
        raw: &Path,
        profile_index: usize,
        expected_output: &Path,
    ) -> Result<()> {
        self.update_render(raw, profile_index, |render| {
            render.status = ReviewRenderStatus::Queued;
            render.output_path = Some(expected_output.to_path_buf());
            render.error = None;
            render.duration_ms = None;
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
        });
        if result.is_ok()
            && let Some(render_key) = pending_retouch_key
        {
            self.schedule_retouch_job(
                raw.to_path_buf(),
                profile_index,
                output.to_path_buf(),
                render_key,
            );
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
        self.update_render(raw, profile_index, |render| {
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
        })
    }

    pub(super) fn update_render<F>(&self, raw: &Path, profile_index: usize, update: F) -> Result<()>
    where
        F: FnOnce(&mut ReviewProfileRender),
    {
        let mut store = self.lock_store()?;
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
        let history_entry = history_render_changed(image, &before, &after);
        save_store(&self.state_path, &store)?;
        drop(store);
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
        update: F,
    ) -> Result<bool>
    where
        F: FnOnce(&mut ReviewProfileRender),
    {
        let mut updated = false;
        let mut history_entry = None;
        let mut store = self.lock_store()?;
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
            save_store(&self.state_path, &store)?;
        }
        drop(store);
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
    ) -> Result<Option<(ReviewProfile, RetouchSettings)>> {
        let store = self.lock_store()?;
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
        Ok(Some((profile, image.retouch.clone())))
    }

    pub(super) fn schedule_retouch_job(
        &self,
        raw: PathBuf,
        profile_index: usize,
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

    pub(super) fn run_scheduled_retouch_job(&self, job: ScheduledRetouchJob) {
        let Ok(Some((profile, retouch))) =
            self.retouch_task_snapshot(&job.raw, job.profile_index, &job.render_key)
        else {
            return;
        };
        let started = Instant::now();
        let _ = self.update_render_if_key(&job.raw, job.profile_index, &job.render_key, |render| {
            render.status = ReviewRenderStatus::Processing;
            render.error = None;
        });
        let temp_output = retouch_temp_output(&job.output, &job.render_key);
        let result = self.render_retouch_output(
            &job.raw,
            &profile,
            job.profile_index,
            &retouch,
            &temp_output,
        );
        match result {
            Ok(()) => {
                match self.retouch_task_snapshot(&job.raw, job.profile_index, &job.render_key) {
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
                            job.profile_index,
                            &job.render_key,
                            |render| {
                                render.status = ReviewRenderStatus::Done;
                                render.render_key = None;
                                render.output_path = Some(job.output.clone());
                                render.error = None;
                                render.duration_ms = Some(started.elapsed().as_millis() as u64);
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

    pub(super) fn record_retouch_render_failed(
        &self,
        job: &ScheduledRetouchJob,
        temp_output: &Path,
        started: Instant,
        message: String,
    ) {
        let _ = self.update_render_if_key(&job.raw, job.profile_index, &job.render_key, |render| {
            render.status = ReviewRenderStatus::Failed;
            render.render_key = None;
            render.error = Some(message);
            render.duration_ms = Some(started.elapsed().as_millis() as u64);
        });
        let _ = fs::remove_file(temp_output);
    }

    pub(super) fn render_retouch_output(
        &self,
        raw: &Path,
        profile: &ReviewProfile,
        profile_index: usize,
        retouch: &RetouchSettings,
        output: &Path,
    ) -> Result<()> {
        let raw = safe_existing_raw_source(raw, &self.input_root)?;
        let temp_dir = Builder::new()
            .prefix("mini-film-review-retouch-")
            .tempdir()?;
        let apply_args = ApplyArgs {
            raw: raw.clone(),
            output: output.to_path_buf(),
            profile: profile.selector.clone(),
            hald_dir: self.hald_dir.clone(),
            profiles_root: self.profiles_root.clone(),
            hald_level: self.hald_level,
            rawtherapee: self.rawtherapee.clone(),
            convert: self.convert.clone(),
            keep_intermediate: None,
            no_grain: self.no_grain,
            color_noise_iso_threshold: self.color_noise_iso_threshold,
            lens_corrections: self.lens_corrections,
            grain: self.grain.clone(),
            grain_preset: self.grain_preset.clone(),
            grain_seed: self.grain_seed,
            export: self.export.clone(),
            retouch: None,
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
                color_noise_iso_threshold: self.color_noise_iso_threshold,
                lens_corrections: self.lens_corrections,
                export: &self.export,
                quiet: true,
                exif_comment: Some(format!(
                    "mini-film {} usage=review profile={} {}",
                    env!("CARGO_PKG_VERSION"),
                    profile.stem,
                    retouch.summary()
                )),
                retouch: Some(retouch),
            },
            &resolved,
            seed,
            temp_dir.path(),
            None,
        )
    }

    pub(super) fn apply_review_update(&self, update: ReviewUpdateRequest) -> Result<()> {
        let mut retouch_jobs = Vec::new();
        let mut history_entries = Vec::new();
        let mut store = self.lock_store()?;
        let before_ui = store.ui.clone();
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
            if !image
                .profiles
                .iter()
                .any(|profile| profile.profile_index == update.selected_profile_index)
            {
                bail!(
                    "selected profile index {} is not available for image {}",
                    update.selected_profile_index,
                    update.image_id
                );
            }
            let before_image = image.clone();
            image.rating = update.rating.min(5);
            image.labels = if update.labels.is_empty() {
                normalize_review_labels([update.label])
            } else {
                normalize_review_labels(update.labels)
            };
            image.label = first_review_label(&image.labels);
            image.tags = normalize_tags(update.tags);
            image.notes = update.notes.trim().to_string();
            let retouch_changed = update
                .retouch
                .as_ref()
                .is_some_and(|retouch| retouch.clone().normalized() != image.retouch);
            if let Some(retouch) = update.retouch {
                image.retouch = retouch.normalized();
            }
            image.selected_profile_index = update.selected_profile_index;
            if let Some(indexes) = update.publish_profile_indexes {
                validate_publish_profile_indexes(&indexes, &image.profiles)?;
                image.publish_profile_indexes =
                    Some(normalize_publish_profile_indexes(&indexes, &image.profiles));
            }
            if retouch_changed {
                let render_key = image.retouch.render_key();
                let publish_indexes = effective_publish_profile_indexes(image);
                let visible_profile_index =
                    preferred_preview_profile_index(image, &publish_indexes);
                let publish_index_set = publish_indexes.iter().copied().collect::<HashSet<_>>();
                let mut render_order = image
                    .profiles
                    .iter()
                    .enumerate()
                    .map(|(index, render)| {
                        let priority = if Some(render.profile_index) == visible_profile_index {
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
                    let render = &mut image.profiles[index];
                    render.status = ReviewRenderStatus::Queued;
                    render.error = None;
                    render.duration_ms = None;
                    render.render_key = Some(render_key.clone());
                    render.updated_at = now_string();
                    if let Some(output) = &render.output_path {
                        retouch_jobs.push((
                            image.raw_path.clone(),
                            render.profile_index,
                            output.clone(),
                            render_key.clone(),
                        ));
                    }
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
        if let Some(entry) = history_ui_changed(&store, &before_ui, &store.ui) {
            history_entries.push(entry);
        }
        save_store(&self.state_path, &store)?;
        drop(store);
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
        let mut store = self.lock_store()?;
        let before_ui = store.ui.clone();
        store.set_ui(update)?;
        let history_entry = history_ui_changed(&store, &before_ui, &store.ui);
        save_store(&self.state_path, &store)?;
        drop(store);
        if let Some(entry) = history_entry {
            self.append_history(entry)?;
        }
        self.broadcast_state()
    }

    pub(super) fn api_state_json(&self) -> Result<String> {
        let client_count = self.client_count()?;
        let store = self.lock_store()?;
        let mut images = store.images.clone();
        images.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let images = images
            .iter()
            .map(|image| {
                let mut exif = image.exif.clone();
                exif.sanitize_text_fields();
                let profiles = image
                    .profiles
                    .iter()
                    .map(|render| {
                        json!({
                            "profile_index": render.profile_index,
                            "profile_stem": render.profile_stem,
                            "status": render.status,
                            "url": if render.status == ReviewRenderStatus::Done {
                                Some(format!("media/{}/{}", image.id, render.profile_index))
                            } else {
                                None
                            },
                            "error": render.error,
                            "duration_ms": render.duration_ms,
                            "retouch_pending": render.render_key.is_some(),
                            "updated_at": render.updated_at,
                        })
                    })
                    .collect::<Vec<_>>();
                json!({
                    "id": image.id,
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
                    "preview_updated_at": image.preview.updated_at,
                    "selected_profile_index": image.selected_profile_index,
                    "rating": image.rating,
                    "label": image.label,
                    "labels": image_review_labels(image),
                    "tags": image.tags,
                    "notes": image.notes,
                    "retouch": image.retouch,
                    "publish_profile_indexes": effective_publish_profile_indexes(image),
                    "profiles": profiles,
                    "updated_at": image.updated_at,
                })
            })
            .collect::<Vec<_>>();

        serde_json::to_string(&json!({
            "version": env!("CARGO_PKG_VERSION"),
            "profiles": store.profiles,
            "client_count": client_count,
            "publish_defaults": self.publish_defaults,
            "publish_jobs": self.publish_jobs_snapshot()?,
            "ui": {
                "current_image_id": store.ui.current_image_id,
                "min_rating": store.ui.min_rating,
            },
            "images": images,
            "publish_root": self.publish_root().to_string_lossy(),
        }))
        .context("serializing review API state")
    }

    pub(super) fn media_path(&self, image_id: u64, profile_index: usize) -> Result<PathBuf> {
        let store = self.lock_store()?;
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

    pub(super) fn preview_media_path(&self, image_id: u64) -> Result<PathBuf> {
        let store = self.lock_store()?;
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
        let mut id = self
            .next_publish_job_id
            .lock()
            .map_err(|_| anyhow!("review publish job id lock poisoned"))?;
        let job = ReviewPublishJob {
            id: *id,
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
            error: None,
        };
        *id += 1;
        drop(id);

        self.publish_jobs
            .lock()
            .map_err(|_| anyhow!("review publish jobs lock poisoned"))?
            .push(job.clone());
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
            rerender_raw: output_format != self.output_format || export != self.export,
            export,
            no_grain: self.no_grain,
            color_noise_iso_threshold: self.color_noise_iso_threshold,
            lens_corrections: self.lens_corrections,
            grain: self.grain.clone(),
            grain_preset: self.grain_preset.clone(),
            grain_seed: self.grain_seed,
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
            job.error = None;
        })
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
        F: FnOnce(&mut ReviewPublishJob),
    {
        let mut jobs = self
            .publish_jobs
            .lock()
            .map_err(|_| anyhow!("review publish jobs lock poisoned"))?;
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
        drop(jobs);
        if let Some(entry) = history_entry {
            self.append_history(entry)?;
        }
        self.broadcast_state()
    }

    pub(super) fn publish_jobs_snapshot(&self) -> Result<Vec<ReviewPublishJob>> {
        Ok(self
            .publish_jobs
            .lock()
            .map_err(|_| anyhow!("review publish jobs lock poisoned"))?
            .clone())
    }

    pub(super) fn subscribe(&self) -> broadcast::Receiver<String> {
        self.subscribers.subscribe()
    }

    pub(super) fn broadcast_state(&self) -> Result<()> {
        let state = self.api_state_json()?;
        let _ = self.subscribers.send(state);
        Ok(())
    }

    pub(super) fn client_count(&self) -> Result<usize> {
        Ok(self.subscribers.receiver_count())
    }

    pub(super) fn lock_store(&self) -> Result<std::sync::MutexGuard<'_, ReviewStore>> {
        self.state
            .lock()
            .map_err(|_| anyhow!("review state lock poisoned"))
    }
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
        .unwrap_or("review");
    output.with_file_name(format!(".{stem}.retouch-{render_key}.{extension}"))
}

pub(super) fn apply_base_render_done(
    render: &mut ReviewProfileRender,
    output: &Path,
    duration: Duration,
) -> Option<String> {
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
