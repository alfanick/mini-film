use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha1::{Digest, Sha1};

use crate::app::review_assets::{review_index_html, review_script, review_styles};
use crate::{
    app::apply::{ApplyArgs, run_apply},
    app::batch::{FolderGalleryOptions, render_gallery_for_folder},
    app::export::validate_export_options,
    cli::{BatchOutputFormat, ExportOptions, GalleryTemplate, LensCorrections},
};

#[derive(Clone, Debug)]
pub(crate) struct ReviewConfig {
    pub(crate) address: String,
    pub(crate) input_root: PathBuf,
    pub(crate) output_root: PathBuf,
    pub(crate) hald_dir: PathBuf,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_level: u32,
    pub(crate) rawtherapee: PathBuf,
    pub(crate) output_format: BatchOutputFormat,
    pub(crate) profiles: Vec<ReviewProfile>,
    pub(crate) gallery: Option<ReviewGalleryConfig>,
    pub(crate) convert: PathBuf,
    pub(crate) export: ExportOptions,
    pub(crate) jobs: usize,
    pub(crate) publish_album: String,
    pub(crate) no_grain: bool,
    pub(crate) color_noise_iso_threshold: u32,
    pub(crate) lens_corrections: LensCorrections,
    pub(crate) grain: Option<String>,
    pub(crate) grain_preset: Option<String>,
    pub(crate) grain_seed: Option<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct ReviewGalleryConfig {
    pub(crate) template: GalleryTemplate,
    pub(crate) columns: u32,
    pub(crate) thumbnail_long_edge: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ReviewProfile {
    pub(crate) index: usize,
    pub(crate) selector: String,
    pub(crate) stem: String,
}

#[derive(Clone)]
pub(crate) struct ReviewHandle {
    state: Arc<Mutex<ReviewStore>>,
    subscribers: Arc<Mutex<Vec<Sender<String>>>>,
    state_path: PathBuf,
    input_root: PathBuf,
    output_root: PathBuf,
    hald_dir: PathBuf,
    profiles_root: PathBuf,
    hald_level: u32,
    rawtherapee: PathBuf,
    output_format: BatchOutputFormat,
    gallery: Option<ReviewGalleryConfig>,
    convert: PathBuf,
    export: ExportOptions,
    jobs: usize,
    no_grain: bool,
    color_noise_iso_threshold: u32,
    lens_corrections: LensCorrections,
    grain: Option<String>,
    grain_preset: Option<String>,
    grain_seed: Option<u64>,
    publish_defaults: ReviewPublishDefaults,
    publish_jobs: Arc<Mutex<Vec<ReviewPublishJob>>>,
    next_publish_job_id: Arc<Mutex<u64>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReviewStore {
    next_id: u64,
    profiles: Vec<ReviewProfile>,
    images: Vec<ReviewImage>,
    #[serde(default)]
    ui: ReviewUiState,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ReviewUiState {
    current_image_id: Option<u64>,
    min_rating: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReviewImage {
    id: u64,
    raw_path: PathBuf,
    relative_path: String,
    file_name: String,
    #[serde(default)]
    preview: ReviewPreview,
    selected_profile_index: usize,
    #[serde(default)]
    rating: u8,
    #[serde(default)]
    label: ReviewLabel,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    publish_profile_indexes: Option<Vec<usize>>,
    profiles: Vec<ReviewProfileRender>,
    updated_at: String,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
enum ReviewLabel {
    #[default]
    None,
    Red,
    Yellow,
    Green,
    Blue,
    Purple,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReviewPreview {
    status: ReviewRenderStatus,
    path: Option<PathBuf>,
    error: Option<String>,
    updated_at: String,
}

impl Default for ReviewPreview {
    fn default() -> Self {
        Self {
            status: ReviewRenderStatus::Missing,
            path: None,
            error: None,
            updated_at: now_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReviewProfileRender {
    profile_index: usize,
    profile_stem: String,
    status: ReviewRenderStatus,
    output_path: Option<PathBuf>,
    error: Option<String>,
    duration_ms: Option<u64>,
    updated_at: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum ReviewRenderStatus {
    Missing,
    Queued,
    Processing,
    Done,
    Failed,
}

#[derive(Debug, Deserialize)]
struct ReviewUpdateRequest {
    image_id: u64,
    rating: u8,
    label: ReviewLabel,
    tags: Vec<String>,
    #[serde(default)]
    notes: String,
    selected_profile_index: usize,
    #[serde(default)]
    publish_profile_indexes: Option<Vec<usize>>,
    #[serde(default)]
    advance_after_update: bool,
}

#[derive(Debug, Deserialize)]
struct ReviewUiUpdateRequest {
    #[serde(default)]
    current_image_id: Option<u64>,
    #[serde(default)]
    min_rating: u8,
}

#[derive(Debug, Default, Deserialize)]
struct PublishRequest {
    #[serde(default)]
    min_rating: u8,
    #[serde(default)]
    album: Option<String>,
    #[serde(default)]
    labels: Vec<ReviewLabel>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    output_format: Option<String>,
    #[serde(default)]
    gallery: Option<String>,
    #[serde(default)]
    jpg_quality: Option<u8>,
    #[serde(default)]
    size_mode: Option<String>,
    #[serde(default)]
    resize: Option<String>,
    #[serde(default)]
    long_edge: Option<u32>,
    #[serde(default)]
    max_width: Option<u32>,
    #[serde(default)]
    max_height: Option<u32>,
    #[serde(default)]
    jpeg_subsampling: Option<String>,
    #[serde(default)]
    strip_metadata: Option<bool>,
    #[serde(default)]
    progressive_jpeg: Option<bool>,
    #[serde(default)]
    gallery_thumbnail_long_edge: Option<u32>,
    #[serde(default)]
    gallery_columns: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PublishReport {
    pub(crate) linked: u64,
    pub(crate) skipped: u64,
    pub(crate) min_rating: u8,
    pub(crate) galleries: u64,
    gallery_roots: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub(crate) struct ReviewPublishCommandArgs {
    pub(crate) state: PathBuf,
    pub(crate) input_root: PathBuf,
    pub(crate) output_root: PathBuf,
    pub(crate) album: String,
    pub(crate) min_rating: u8,
    pub(crate) labels: Vec<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) output_format: BatchOutputFormat,
    pub(crate) hald_dir: PathBuf,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_level: u32,
    pub(crate) rawtherapee: PathBuf,
    pub(crate) convert: PathBuf,
    pub(crate) jobs: usize,
    pub(crate) gallery: Option<GalleryTemplate>,
    pub(crate) gallery_thumbnail_long_edge: u32,
    pub(crate) gallery_columns: u32,
    pub(crate) export: ExportOptions,
    pub(crate) rerender_raw: bool,
    pub(crate) no_grain: bool,
    pub(crate) color_noise_iso_threshold: u32,
    pub(crate) lens_corrections: LensCorrections,
    pub(crate) grain: Option<String>,
    pub(crate) grain_preset: Option<String>,
    pub(crate) grain_seed: Option<u64>,
    pub(crate) progress_events: bool,
}

#[derive(Clone, Debug, Serialize)]
struct ReviewPublishDefaults {
    album: String,
    output_format: String,
    jpg_quality: u8,
    resize: Option<String>,
    long_edge: Option<u32>,
    max_width: Option<u32>,
    max_height: Option<u32>,
    jpeg_subsampling: String,
    strip_metadata: bool,
    progressive_jpeg: bool,
    gallery: Option<String>,
    gallery_thumbnail_long_edge: u32,
    gallery_columns: u32,
}

#[derive(Clone, Debug, Serialize)]
struct ReviewPublishJob {
    id: u64,
    album: String,
    status: ReviewPublishJobStatus,
    started_at: String,
    finished_at: Option<String>,
    processed: u64,
    total: u64,
    step: String,
    current: Option<String>,
    linked: u64,
    skipped: u64,
    galleries: u64,
    error: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum ReviewPublishJobStatus {
    Running,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug)]
struct ReviewGalleryDefaults {
    template: Option<GalleryTemplate>,
    thumbnail_long_edge: u32,
    columns: u32,
}

#[derive(Clone, Debug)]
struct ReviewPublishOptions {
    album: PathBuf,
    min_rating: u8,
    labels: HashSet<ReviewLabel>,
    tags: HashSet<String>,
    output_format: BatchOutputFormat,
    hald_dir: PathBuf,
    profiles_root: PathBuf,
    hald_level: u32,
    rawtherapee: PathBuf,
    convert: PathBuf,
    jobs: usize,
    export: ExportOptions,
    rerender_raw: bool,
    no_grain: bool,
    color_noise_iso_threshold: u32,
    lens_corrections: LensCorrections,
    grain: Option<String>,
    grain_preset: Option<String>,
    grain_seed: Option<u64>,
    write_metadata: bool,
}

struct ReviewPublishOutput<'a> {
    input_root: &'a Path,
    source: &'a Path,
    destination: &'a Path,
    image: &'a ReviewImage,
    render: &'a ReviewProfileRender,
    profile: &'a ReviewProfile,
    options: &'a ReviewPublishOptions,
}

#[derive(Clone, Debug)]
struct ReviewPublishTask {
    source: PathBuf,
    destination: PathBuf,
    image: ReviewImage,
    render: ReviewProfileRender,
    profile: ReviewProfile,
    current: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ReviewPublishProgress {
    processed: u64,
    total: u64,
    linked: u64,
    skipped: u64,
    galleries: u64,
    step: String,
    current: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum ReviewPublishEvent {
    Progress { progress: ReviewPublishProgress },
    Report { report: PublishReport },
}

type ReviewPublishProgressSink<'a> = &'a (dyn Fn(ReviewPublishProgress) + Sync);

impl ReviewPublishDefaults {
    fn new(
        album: String,
        output_format: BatchOutputFormat,
        export: &ExportOptions,
        gallery: ReviewGalleryDefaults,
    ) -> Self {
        Self {
            album,
            output_format: output_format.to_string(),
            jpg_quality: export.jpg_quality,
            resize: export.resize.clone(),
            long_edge: export.long_edge,
            max_width: export.max_width,
            max_height: export.max_height,
            jpeg_subsampling: export.jpeg_subsampling.to_string(),
            strip_metadata: export.strip_metadata,
            progressive_jpeg: export.progressive_jpeg,
            gallery: gallery.template.map(|template| template.to_string()),
            gallery_thumbnail_long_edge: gallery.thumbnail_long_edge,
            gallery_columns: gallery.columns,
        }
    }
}

impl ReviewStore {
    fn new(profiles: Vec<ReviewProfile>) -> Self {
        Self {
            next_id: 1,
            profiles,
            images: Vec::new(),
            ui: ReviewUiState::default(),
        }
    }

    fn sync_profiles(&mut self, profiles: Vec<ReviewProfile>) {
        self.profiles = profiles;
        let profiles = self.profiles.clone();
        for image in &mut self.images {
            if matches!(
                image.preview.status,
                ReviewRenderStatus::Queued | ReviewRenderStatus::Processing
            ) {
                image.preview.status = ReviewRenderStatus::Missing;
                image.preview.updated_at = now_string();
            }
            sync_image_profile_renders(image, &profiles);
        }
        self.normalize_ui();
    }

    fn ensure_image(&mut self, input_root: &Path, raw: &Path) -> Result<&mut ReviewImage> {
        if let Some(index) = self.images.iter().position(|image| image.raw_path == raw) {
            return Ok(&mut self.images[index]);
        }

        let id = self.next_id;
        self.next_id += 1;
        let relative = raw
            .strip_prefix(input_root)
            .unwrap_or(raw)
            .to_string_lossy()
            .to_string();
        let file_name = raw
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_string();
        let mut image = ReviewImage {
            id,
            raw_path: raw.to_path_buf(),
            relative_path: relative,
            file_name,
            preview: ReviewPreview::default(),
            selected_profile_index: 0,
            rating: 0,
            label: ReviewLabel::None,
            tags: Vec::new(),
            notes: String::new(),
            publish_profile_indexes: None,
            profiles: Vec::new(),
            updated_at: now_string(),
        };
        sync_image_profile_renders(&mut image, &self.profiles);
        self.images.push(image);
        self.normalize_ui();
        let index = self.images.len() - 1;
        Ok(&mut self.images[index])
    }

    fn normalize_ui(&mut self) {
        self.ui.min_rating = self.ui.min_rating.min(5);
        let visible = self.visible_image_ids_at(self.ui.min_rating);
        if !self
            .ui
            .current_image_id
            .is_some_and(|id| visible.contains(&id))
        {
            self.ui.current_image_id = visible.first().copied();
        }
    }

    fn set_ui(&mut self, update: ReviewUiUpdateRequest) -> Result<()> {
        self.ui.min_rating = update.min_rating.min(5);
        if let Some(id) = update.current_image_id {
            if !self.images.iter().any(|image| image.id == id) {
                bail!("review image {id} does not exist");
            }
            self.ui.current_image_id = Some(id);
        }
        self.normalize_ui();
        Ok(())
    }

    fn planned_advance_after(&self, image_id: u64) -> ReviewAdvance {
        let visible = self.visible_image_ids_at(self.ui.min_rating);
        let Some(index) = visible.iter().position(|id| *id == image_id) else {
            return ReviewAdvance::FirstVisible;
        };
        if let Some(next) = visible.get(index + 1) {
            ReviewAdvance::Image(*next)
        } else {
            ReviewAdvance::NextPass
        }
    }

    fn apply_advance(&mut self, advance: ReviewAdvance) {
        match advance {
            ReviewAdvance::Image(id) => {
                self.ui.current_image_id = Some(id);
                self.normalize_ui();
            }
            ReviewAdvance::FirstVisible => self.normalize_ui(),
            ReviewAdvance::NextPass => {
                self.ui.min_rating = self.ui.min_rating.saturating_add(1).min(5);
                self.ui.current_image_id = self
                    .visible_image_ids_at(self.ui.min_rating)
                    .first()
                    .copied();
                self.normalize_ui();
            }
        }
    }

    fn visible_image_ids_at(&self, min_rating: u8) -> Vec<u64> {
        let mut images = self
            .images
            .iter()
            .filter(|image| image.rating >= min_rating.min(5))
            .map(|image| (image.relative_path.as_str(), image.id))
            .collect::<Vec<_>>();
        images.sort_by(|left, right| left.0.cmp(right.0));
        images.into_iter().map(|(_, id)| id).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReviewAdvance {
    Image(u64),
    FirstVisible,
    NextPass,
}

/// Start the embedded review server and return a handle daemon workers can update.
///
/// The server is a small blocking `TcpListener` running on its own thread. Daemon
/// workers update the shared review store whenever RAW files are queued or
/// rendered, and browser clients subscribe to `/api/events` for live SSE state
/// updates. This keeps review responsive while daemon workers keep processing in
/// parallel.
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

    let gallery_defaults = handle_gallery_defaults(&config.gallery);
    let publish_defaults = ReviewPublishDefaults::new(
        config.publish_album,
        config.output_format,
        &config.export,
        gallery_defaults,
    );
    let handle = ReviewHandle {
        state: Arc::new(Mutex::new(store)),
        subscribers: Arc::new(Mutex::new(Vec::new())),
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
    };

    let listener = TcpListener::bind(&config.address)
        .with_context(|| format!("binding review server to {}", config.address))?;
    let server_handle = handle.clone();
    thread::Builder::new()
        .name("mini-film-review".to_string())
        .spawn(move || run_review_listener(listener, server_handle))
        .context("starting daemon review server thread")?;

    Ok(handle)
}

impl ReviewHandle {
    pub(crate) fn state_path(&self) -> &Path {
        &self.state_path
    }

    pub(crate) fn publish_root(&self) -> PathBuf {
        self.output_root.join("reviewed")
    }

    fn preview_root(&self) -> PathBuf {
        self.output_root.join(".mini-film-review-previews")
    }

    fn preview_path_for(&self, raw: &Path, image_id: u64) -> PathBuf {
        self.preview_root()
            .join(format!("{image_id:08}-{}.jpg", short_path_sha1(raw)))
    }

    pub(crate) fn record_discovered_raw(&self, raw: &Path) -> Result<()> {
        let mut preview_job = None;
        let mut store = self.lock_store()?;
        let image = store.ensure_image(&self.input_root, raw)?;
        let preview_path = self.preview_path_for(raw, image.id);
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
        }
        save_store(&self.state_path, &store)?;
        drop(store);
        self.broadcast_state()?;
        if let Some((raw, preview_path)) = preview_job {
            self.spawn_preview_job(raw, preview_path);
        }
        Ok(())
    }

    fn spawn_preview_job(&self, raw: PathBuf, output: PathBuf) {
        let handle = self.clone();
        let _ = thread::Builder::new()
            .name("mini-film-review-preview".to_string())
            .spawn(move || {
                let start = std::time::Instant::now();
                if let Err(error) = handle.record_preview_processing(&raw) {
                    eprintln!("review preview state update failed: {error:#}");
                }
                let result = extract_embedded_preview(&raw, &output);
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

    fn record_preview_processing(&self, raw: &Path) -> Result<()> {
        self.update_preview(raw, |preview| {
            preview.status = ReviewRenderStatus::Processing;
            preview.error = None;
        })
    }

    fn record_preview_done(&self, raw: &Path, output: &Path) -> Result<()> {
        self.update_preview(raw, |preview| {
            preview.status = ReviewRenderStatus::Done;
            preview.path = Some(output.to_path_buf());
            preview.error = None;
        })
    }

    fn record_preview_failed(&self, raw: &Path, error: &str) -> Result<()> {
        self.update_preview(raw, |preview| {
            preview.status = ReviewRenderStatus::Failed;
            preview.error = Some(error.to_string());
        })
    }

    fn update_preview<F>(&self, raw: &Path, update: F) -> Result<()>
    where
        F: FnOnce(&mut ReviewPreview),
    {
        let mut store = self.lock_store()?;
        let image = store.ensure_image(&self.input_root, raw)?;
        update(&mut image.preview);
        image.preview.updated_at = now_string();
        image.updated_at = now_string();
        save_store(&self.state_path, &store)?;
        drop(store);
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
        self.update_render(raw, profile_index, |render| {
            render.status = ReviewRenderStatus::Done;
            render.output_path = Some(output.to_path_buf());
            render.error = None;
            render.duration_ms = Some(duration.as_millis() as u64);
        })
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
            render.status = ReviewRenderStatus::Failed;
            if let Some(output) = output {
                render.output_path = Some(output.to_path_buf());
            }
            render.error = Some(error.to_string());
            render.duration_ms = Some(duration.as_millis() as u64);
        })
    }

    fn update_render<F>(&self, raw: &Path, profile_index: usize, update: F) -> Result<()>
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
        update(render);
        render.updated_at = now_string();
        image.updated_at = now_string();
        save_store(&self.state_path, &store)?;
        drop(store);
        self.broadcast_state()
    }

    fn apply_review_update(&self, update: ReviewUpdateRequest) -> Result<()> {
        let mut store = self.lock_store()?;
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
            image.rating = update.rating.min(5);
            image.label = update.label;
            image.tags = normalize_tags(update.tags);
            image.notes = update.notes.trim().to_string();
            image.selected_profile_index = update.selected_profile_index;
            if let Some(indexes) = update.publish_profile_indexes {
                validate_publish_profile_indexes(&indexes, &image.profiles)?;
                image.publish_profile_indexes =
                    Some(normalize_publish_profile_indexes(&indexes, &image.profiles));
            }
            image.updated_at = now_string();
        }
        if let Some(advance) = advance {
            store.apply_advance(advance);
        } else {
            store.normalize_ui();
        }
        save_store(&self.state_path, &store)?;
        drop(store);
        self.broadcast_state()
    }

    fn apply_ui_update(&self, update: ReviewUiUpdateRequest) -> Result<()> {
        let mut store = self.lock_store()?;
        store.set_ui(update)?;
        save_store(&self.state_path, &store)?;
        drop(store);
        self.broadcast_state()
    }

    fn api_state_json(&self) -> Result<String> {
        let client_count = self.client_count()?;
        let store = self.lock_store()?;
        let mut images = store.images.clone();
        images.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let images = images
            .iter()
            .map(|image| {
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
                            "updated_at": render.updated_at,
                        })
                    })
                    .collect::<Vec<_>>();
                json!({
                    "id": image.id,
                    "relative_path": image.relative_path,
                    "file_name": image.file_name,
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
                    "tags": image.tags,
                    "notes": image.notes,
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

    fn media_path(&self, image_id: u64, profile_index: usize) -> Result<PathBuf> {
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

    fn preview_media_path(&self, image_id: u64) -> Result<PathBuf> {
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

    fn start_publish_job(&self, request: PublishRequest) -> Result<ReviewPublishJob> {
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

    fn publish_args_from_request(
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

    fn record_publish_job_progress(
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

    fn record_publish_job_done(&self, job_id: u64, report: &PublishReport) -> Result<()> {
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

    fn record_publish_job_failed(&self, job_id: u64, message: &str) -> Result<()> {
        self.update_publish_job(job_id, |job| {
            job.status = ReviewPublishJobStatus::Failed;
            job.finished_at = Some(now_string());
            job.step = "failed".to_string();
            job.current = None;
            job.error = Some(message.to_string());
        })
    }

    fn update_publish_job<F>(&self, job_id: u64, update: F) -> Result<()>
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
        update(job);
        if jobs.len() > 20 {
            let remove = jobs.len() - 20;
            jobs.drain(0..remove);
        }
        drop(jobs);
        self.broadcast_state()
    }

    fn publish_jobs_snapshot(&self) -> Result<Vec<ReviewPublishJob>> {
        Ok(self
            .publish_jobs
            .lock()
            .map_err(|_| anyhow!("review publish jobs lock poisoned"))?
            .clone())
    }

    fn subscribe(&self) -> Result<Receiver<String>> {
        let (sender, receiver) = mpsc::channel();
        {
            let mut subscribers = self
                .subscribers
                .lock()
                .map_err(|_| anyhow!("review subscribers lock poisoned"))?;
            subscribers.push(sender);
        }
        self.broadcast_state()?;
        Ok(receiver)
    }

    fn broadcast_state(&self) -> Result<()> {
        let state = self.api_state_json()?;
        let mut subscribers = self
            .subscribers
            .lock()
            .map_err(|_| anyhow!("review subscribers lock poisoned"))?;
        let before = subscribers.len();
        subscribers.retain(|subscriber| subscriber.send(state.clone()).is_ok());
        let after = subscribers.len();
        drop(subscribers);
        if after < before {
            let state = self.api_state_json()?;
            let mut subscribers = self
                .subscribers
                .lock()
                .map_err(|_| anyhow!("review subscribers lock poisoned"))?;
            subscribers.retain(|subscriber| subscriber.send(state.clone()).is_ok());
        }
        Ok(())
    }

    fn client_count(&self) -> Result<usize> {
        Ok(self
            .subscribers
            .lock()
            .map_err(|_| anyhow!("review subscribers lock poisoned"))?
            .len())
    }

    fn lock_store(&self) -> Result<std::sync::MutexGuard<'_, ReviewStore>> {
        self.state
            .lock()
            .map_err(|_| anyhow!("review state lock poisoned"))
    }
}

fn sync_image_profile_renders(image: &mut ReviewImage, profiles: &[ReviewProfile]) {
    let existing = image
        .profiles
        .iter()
        .cloned()
        .map(|render| (render.profile_index, render))
        .collect::<HashMap<_, _>>();
    image.profiles = profiles
        .iter()
        .map(|profile| {
            existing
                .get(&profile.index)
                .cloned()
                .unwrap_or_else(|| ReviewProfileRender {
                    profile_index: profile.index,
                    profile_stem: profile.stem.clone(),
                    status: ReviewRenderStatus::Missing,
                    output_path: None,
                    error: None,
                    duration_ms: None,
                    updated_at: now_string(),
                })
        })
        .collect();
    if !image
        .profiles
        .iter()
        .any(|profile| profile.profile_index == image.selected_profile_index)
    {
        image.selected_profile_index = profiles.first().map(|profile| profile.index).unwrap_or(0);
    }
    image.publish_profile_indexes = Some(effective_publish_profile_indexes(image));
}

fn effective_publish_profile_indexes(image: &ReviewImage) -> Vec<usize> {
    match &image.publish_profile_indexes {
        Some(indexes) => normalize_publish_profile_indexes(indexes, &image.profiles),
        None => image
            .profiles
            .iter()
            .map(|profile| profile.profile_index)
            .collect(),
    }
}

fn normalize_publish_profile_indexes(
    indexes: &[usize],
    profiles: &[ReviewProfileRender],
) -> Vec<usize> {
    let selected = indexes.iter().copied().collect::<HashSet<_>>();
    profiles
        .iter()
        .filter_map(|profile| {
            selected
                .contains(&profile.profile_index)
                .then_some(profile.profile_index)
        })
        .collect()
}

fn validate_publish_profile_indexes(
    indexes: &[usize],
    profiles: &[ReviewProfileRender],
) -> Result<()> {
    let valid = profiles
        .iter()
        .map(|profile| profile.profile_index)
        .collect::<HashSet<_>>();
    for index in indexes {
        if !valid.contains(index) {
            bail!("publish profile index {index} is not available");
        }
    }
    Ok(())
}

fn load_store(path: &Path) -> Result<Option<ReviewStore>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("parsing {}", path.display()))
        .map(Some)
}

fn save_store(path: &Path, store: &ReviewStore) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(store).context("serializing review state")?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, text).with_context(|| format!("writing {}", temp.display()))?;
    fs::rename(&temp, path)
        .with_context(|| format!("renaming {} to {}", temp.display(), path.display()))
}

fn now_string() -> String {
    chrono::Local::now().to_rfc3339()
}

fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() || normalized.iter().any(|existing| existing == tag) {
            continue;
        }
        normalized.push(tag.to_string());
    }
    normalized
}

fn handle_gallery_defaults(gallery: &Option<ReviewGalleryConfig>) -> ReviewGalleryDefaults {
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

fn parse_batch_output_format(raw: &str) -> Result<BatchOutputFormat> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Ok(BatchOutputFormat::Jpg),
        "tif" | "tiff" => Ok(BatchOutputFormat::Tiff),
        other => bail!("unsupported output format {other:?}; expected jpg or tiff"),
    }
}

fn parse_gallery_template(raw: &str) -> Result<Option<GalleryTemplate>> {
    let raw = raw.trim().to_ascii_lowercase();
    if raw.is_empty() || raw == "none" {
        return Ok(None);
    }
    let template = match raw.as_str() {
        "modern" => GalleryTemplate::Modern,
        "soft" => GalleryTemplate::Soft,
        "compact" => GalleryTemplate::Compact,
        "hero" => GalleryTemplate::Hero,
        "phone" => GalleryTemplate::Phone,
        "all" => GalleryTemplate::All,
        other => bail!("unsupported gallery template {other:?}"),
    };
    Ok(Some(template))
}

fn parse_jpeg_subsampling(raw: &str) -> Result<crate::cli::JpegSubsampling> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "s444" | "444" | "4:4:4" => Ok(crate::cli::JpegSubsampling::S444),
        "s422" | "422" | "4:2:2" => Ok(crate::cli::JpegSubsampling::S422),
        "s420" | "420" | "4:2:0" => Ok(crate::cli::JpegSubsampling::S420),
        other => bail!("unsupported JPEG subsampling {other:?}"),
    }
}

fn lens_corrections_arg(corrections: LensCorrections) -> String {
    let mut parts = Vec::new();
    if corrections.distortion {
        parts.push("distortion");
    }
    if corrections.ca {
        parts.push("ca");
    }
    if corrections.vignetting {
        parts.push("vignetting");
    }
    parts.join(",")
}

fn parse_review_label(raw: &str) -> Result<ReviewLabel> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "none" => Ok(ReviewLabel::None),
        "red" => Ok(ReviewLabel::Red),
        "yellow" => Ok(ReviewLabel::Yellow),
        "green" => Ok(ReviewLabel::Green),
        "blue" => Ok(ReviewLabel::Blue),
        "purple" => Ok(ReviewLabel::Purple),
        other => bail!("unsupported review label {other:?}"),
    }
}

fn normalize_tag_filter(tags: &[String]) -> HashSet<String> {
    tags.iter()
        .map(|tag| tag.trim().to_ascii_lowercase())
        .filter(|tag| !tag.is_empty())
        .collect()
}

fn validate_relative_publish_album(raw: &str) -> Result<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("publish output directory must not be empty");
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        bail!("publish output directory must be relative to the daemon output directory");
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("publish output directory cannot leave the daemon output directory");
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        bail!("publish output directory must contain a folder name");
    }
    Ok(normalized)
}

fn run_review_listener(listener: TcpListener, handle: ReviewHandle) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let handle = handle.clone();
                let _ = thread::Builder::new()
                    .name("mini-film-review-client".to_string())
                    .spawn(move || {
                        if let Err(error) = handle_review_connection(stream, &handle) {
                            eprintln!("review server connection failed: {error:#}");
                        }
                    });
            }
            Err(error) => eprintln!("review server accept failed: {error:#}"),
        }
    }
}

fn handle_review_connection(stream: TcpStream, handle: &ReviewHandle) -> Result<()> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .context("setting review read timeout")?;
    let mut reader = BufReader::new(stream);
    let request = read_http_request(&mut reader)?;
    if request.method == "GET" && review_route_path(&request.path) == "/api/events" {
        return write_event_stream(reader.into_inner(), handle);
    }
    let response = route_request(request, handle);
    write_http_response(reader.get_mut(), response)
}

struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

struct HttpResponse {
    status: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

fn read_http_request(reader: &mut BufReader<TcpStream>) -> Result<HttpRequest> {
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .context("reading HTTP request line")?;
    if request_line.trim().is_empty() {
        bail!("empty HTTP request");
    }
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| anyhow!("missing HTTP method"))?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| anyhow!("missing HTTP path"))?
        .split('?')
        .next()
        .unwrap_or("/")
        .to_string();

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .context("reading HTTP headers")?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = value
                .trim()
                .parse::<usize>()
                .context("parsing Content-Length")?;
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader
            .read_exact(&mut body)
            .context("reading HTTP request body")?;
    }

    Ok(HttpRequest { method, path, body })
}

fn route_request(request: HttpRequest, handle: &ReviewHandle) -> HttpResponse {
    let path = review_route_path(&request.path);
    match (request.method.as_str(), path.as_str()) {
        ("GET", "/") | ("GET", "/review") => {
            text_response("200 OK", "text/html; charset=utf-8", review_index_html())
        }
        ("GET", "/assets/styles.css") => {
            text_response("200 OK", "text/css; charset=utf-8", review_styles())
        }
        ("GET", "/assets/app.js") => text_response(
            "200 OK",
            "application/javascript; charset=utf-8",
            review_script(),
        ),
        ("GET", "/api/state") => match handle.api_state_json() {
            Ok(body) => text_response("200 OK", "application/json; charset=utf-8", &body),
            Err(error) => json_error("500 Internal Server Error", error),
        },
        ("POST", "/api/review") => {
            match serde_json::from_slice::<ReviewUpdateRequest>(&request.body)
                .context("parsing review update")
                .and_then(|update| handle.apply_review_update(update))
                .and_then(|()| handle.api_state_json())
            {
                Ok(body) => text_response("200 OK", "application/json; charset=utf-8", &body),
                Err(error) => json_error("400 Bad Request", error),
            }
        }
        ("POST", "/api/ui") => match serde_json::from_slice::<ReviewUiUpdateRequest>(&request.body)
            .context("parsing review UI update")
            .and_then(|update| handle.apply_ui_update(update))
            .and_then(|()| handle.api_state_json())
        {
            Ok(body) => text_response("200 OK", "application/json; charset=utf-8", &body),
            Err(error) => json_error("400 Bad Request", error),
        },
        ("POST", "/api/publish") => match parse_publish_request(&request.body)
            .and_then(|request| handle.start_publish_job(request))
            .and_then(|_| handle.api_state_json())
        {
            Ok(body) => text_response("200 OK", "application/json; charset=utf-8", &body),
            Err(error) => json_error("500 Internal Server Error", error),
        },
        _ if request.method == "GET" && path.starts_with("/media/") => {
            media_response(&path, handle)
        }
        _ if request.method == "GET" && path.starts_with("/preview/") => {
            preview_response(&path, handle)
        }
        _ => text_response("404 Not Found", "text/plain; charset=utf-8", "not found"),
    }
}

fn review_route_path(path: &str) -> String {
    for marker in ["/api/", "/assets/", "/media/", "/preview/"] {
        if let Some(index) = path.find(marker) {
            return path[index..].to_string();
        }
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }
    if trimmed.ends_with("/review") {
        return "/review".to_string();
    }
    if !trimmed
        .rsplit('/')
        .next()
        .is_some_and(|segment| segment.contains('.'))
    {
        return "/".to_string();
    }
    path.to_string()
}

fn write_event_stream(mut stream: TcpStream, handle: &ReviewHandle) -> Result<()> {
    let receiver = handle.subscribe()?;
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nX-Accel-Buffering: no\r\n\r\n"
    )
    .context("writing SSE headers")?;
    for state in receiver {
        write!(stream, "data: {state}\n\n").context("writing SSE event")?;
        stream.flush().context("flushing SSE event")?;
    }
    Ok(())
}

fn media_response(path: &str, handle: &ReviewHandle) -> HttpResponse {
    let parts = path
        .trim_start_matches("/media/")
        .split('/')
        .collect::<Vec<_>>();
    if parts.len() != 2 {
        return text_response("404 Not Found", "text/plain; charset=utf-8", "not found");
    }
    let image_id = match parts[0].parse::<u64>() {
        Ok(id) => id,
        Err(_) => {
            return text_response(
                "400 Bad Request",
                "text/plain; charset=utf-8",
                "bad image id",
            );
        }
    };
    let profile_index = match parts[1].parse::<usize>() {
        Ok(index) => index,
        Err(_) => {
            return text_response(
                "400 Bad Request",
                "text/plain; charset=utf-8",
                "bad profile index",
            );
        }
    };
    match handle
        .media_path(image_id, profile_index)
        .and_then(|path| fs::read(&path).with_context(|| format!("reading {}", path.display())))
    {
        Ok(body) => HttpResponse {
            status: "200 OK",
            content_type: "image/jpeg",
            body,
        },
        Err(error) => json_error("404 Not Found", error),
    }
}

fn preview_response(path: &str, handle: &ReviewHandle) -> HttpResponse {
    let id = path.trim_start_matches("/preview/");
    let image_id = match id.parse::<u64>() {
        Ok(id) => id,
        Err(_) => {
            return text_response(
                "400 Bad Request",
                "text/plain; charset=utf-8",
                "bad image id",
            );
        }
    };
    match handle
        .preview_media_path(image_id)
        .and_then(|path| fs::read(&path).with_context(|| format!("reading {}", path.display())))
    {
        Ok(body) => HttpResponse {
            status: "200 OK",
            content_type: "image/jpeg",
            body,
        },
        Err(error) => json_error("404 Not Found", error),
    }
}

fn parse_publish_request(body: &[u8]) -> Result<PublishRequest> {
    if body.is_empty() {
        return Ok(PublishRequest::default());
    }
    serde_json::from_slice(body).context("parsing publish request")
}

pub(crate) fn run_review_publish(args: ReviewPublishCommandArgs) -> Result<()> {
    let report = if args.progress_events {
        let emit = |progress: ReviewPublishProgress| {
            if let Ok(line) = serde_json::to_string(&ReviewPublishEvent::Progress { progress }) {
                println!("{line}");
            }
        };
        let report = publish_review_state(&args, Some(&emit))?;
        println!(
            "{}",
            serde_json::to_string(&ReviewPublishEvent::Report {
                report: report.clone()
            })
            .context("serializing review publish report event")?
        );
        report
    } else {
        publish_review_state(&args, None)?
    };
    if !args.progress_events {
        println!(
            "{}",
            serde_json::to_string(&report).context("serializing review publish report")?
        );
    }
    Ok(())
}

fn spawn_review_publish_command<F>(
    args: &ReviewPublishCommandArgs,
    mut on_progress: F,
) -> Result<PublishReport>
where
    F: FnMut(ReviewPublishProgress) -> Result<()>,
{
    let exe = env::current_exe().context("resolving current mini-film executable")?;
    let mut command = Command::new(exe);
    command
        .arg("review-publish")
        .arg("--progress-events")
        .arg("--state")
        .arg(&args.state)
        .arg("--input-root")
        .arg(&args.input_root)
        .arg("--output-root")
        .arg(&args.output_root)
        .arg("--album")
        .arg(&args.album)
        .arg("--min-rating")
        .arg(args.min_rating.to_string())
        .arg("--output-format")
        .arg(args.output_format.to_string())
        .arg("--hald-dir")
        .arg(&args.hald_dir)
        .arg("--profiles-root")
        .arg(&args.profiles_root)
        .arg("--hald-level")
        .arg(args.hald_level.to_string())
        .arg("--rawtherapee")
        .arg(&args.rawtherapee)
        .arg("--convert")
        .arg(&args.convert)
        .arg("--jobs")
        .arg(args.jobs.to_string())
        .arg("--gallery-thumbnail-long-edge")
        .arg(args.gallery_thumbnail_long_edge.to_string())
        .arg("--gallery-columns")
        .arg(args.gallery_columns.to_string())
        .arg("--jpg-quality")
        .arg(args.export.jpg_quality.to_string())
        .arg("--jpeg-subsampling")
        .arg(args.export.jpeg_subsampling.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(gallery) = args.gallery {
        command.arg("--gallery").arg(gallery.to_string());
    }
    for label in &args.labels {
        command.arg("--label").arg(label);
    }
    for tag in &args.tags {
        command.arg("--tag").arg(tag);
    }
    if let Some(resize) = &args.export.resize {
        command.arg("--resize").arg(resize);
    }
    if let Some(long_edge) = args.export.long_edge {
        command.arg("--long-edge").arg(long_edge.to_string());
    }
    if let Some(max_width) = args.export.max_width {
        command.arg("--max-width").arg(max_width.to_string());
    }
    if let Some(max_height) = args.export.max_height {
        command.arg("--max-height").arg(max_height.to_string());
    }
    if args.export.strip_metadata {
        command.arg("--strip-metadata");
    }
    if args.export.progressive_jpeg {
        command.arg("--progressive");
    }
    if args.rerender_raw {
        command.arg("--rerender-raw");
    }
    if args.no_grain {
        command.arg("--no-grain");
    }
    if args.lens_corrections.is_enabled() {
        command
            .arg("--lens-corrections")
            .arg(lens_corrections_arg(args.lens_corrections));
    }
    command
        .arg("--color-noise-iso-threshold")
        .arg(args.color_noise_iso_threshold.to_string());
    if let Some(grain) = &args.grain {
        command.arg("--grain").arg(grain);
    }
    if let Some(grain_preset) = &args.grain_preset {
        command.arg("--grain-preset").arg(grain_preset);
    }
    if let Some(grain_seed) = args.grain_seed {
        command.arg("--grain-seed").arg(grain_seed.to_string());
    }

    let mut child = command
        .spawn()
        .context("starting mini-film review-publish")?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("review-publish stdout pipe was not available"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("review-publish stderr pipe was not available"))?;
    let stderr_reader = thread::Builder::new()
        .name("mini-film-review-publish-stderr".to_string())
        .spawn(move || {
            let mut stderr_text = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut stderr_text);
            stderr_text
        })
        .context("starting review-publish stderr reader")?;

    let mut report = None;
    for line in BufReader::new(stdout).lines() {
        let line = line.context("reading review-publish progress")?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<ReviewPublishEvent>(&line) {
            Ok(ReviewPublishEvent::Progress { progress }) => on_progress(progress)?,
            Ok(ReviewPublishEvent::Report {
                report: final_report,
            }) => report = Some(final_report),
            Err(error) => {
                bail!("parsing review-publish event {line:?}: {error}");
            }
        }
    }

    let status = child.wait().context("waiting for review-publish")?;
    let stderr = stderr_reader
        .join()
        .unwrap_or_else(|_| "stderr reader thread panicked".to_string());
    if !status.success() {
        bail!(
            "review-publish failed with status {}\nstderr:\n{}",
            status,
            stderr.trim()
        );
    }
    report.ok_or_else(|| anyhow!("review-publish completed without a report event"))
}

fn publish_review_state(
    args: &ReviewPublishCommandArgs,
    progress: Option<ReviewPublishProgressSink<'_>>,
) -> Result<PublishReport> {
    validate_export_options(&args.export)?;
    if args.jobs == 0 {
        bail!("--jobs must be at least 1");
    }
    let input_root = canonical_existing_dir(&args.input_root)?;
    let output_root = canonical_existing_dir(&args.output_root)?;
    let state = fs::canonicalize(&args.state)
        .with_context(|| format!("canonicalizing review state {}", args.state.display()))?;
    ensure_path_within(&state, &output_root)?;
    let store = load_store(&state)?.ok_or_else(|| anyhow!("review state is empty"))?;
    let album = validate_relative_publish_album(&args.album)?;
    ensure_safe_dir_all(&output_root, &album)?;

    let labels = args
        .labels
        .iter()
        .map(|label| parse_review_label(label))
        .collect::<Result<HashSet<_>>>()?
        .into_iter()
        .filter(|label| *label != ReviewLabel::None)
        .collect::<HashSet<_>>();
    let tags = normalize_tag_filter(&args.tags);
    let options = ReviewPublishOptions {
        album,
        min_rating: args.min_rating.min(5),
        labels,
        tags,
        output_format: args.output_format,
        hald_dir: args.hald_dir.clone(),
        profiles_root: args.profiles_root.clone(),
        hald_level: args.hald_level,
        rawtherapee: args.rawtherapee.clone(),
        convert: args.convert.clone(),
        jobs: args.jobs,
        export: args.export.clone(),
        rerender_raw: args.rerender_raw,
        no_grain: args.no_grain,
        color_noise_iso_threshold: args.color_noise_iso_threshold,
        lens_corrections: args.lens_corrections,
        grain: args.grain.clone(),
        grain_preset: args.grain_preset.clone(),
        grain_seed: args.grain_seed,
        write_metadata: true,
    };
    let mut report = publish_store_inner(&store, &input_root, &output_root, &options, progress)?;

    if let Some(template) = args.gallery {
        let mut rendered = 0u64;
        for root in &report.gallery_roots {
            emit_publish_progress(
                progress,
                ReviewPublishProgress {
                    processed: report.linked,
                    total: report.linked,
                    linked: report.linked,
                    skipped: report.skipped,
                    galleries: rendered,
                    step: "gallery".to_string(),
                    current: root
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(ToString::to_string),
                },
            );
            render_gallery_for_folder(
                root,
                &FolderGalleryOptions {
                    convert: &args.convert,
                    template,
                    columns: args.gallery_columns,
                    thumbnail_long_edge: args.gallery_thumbnail_long_edge,
                    jobs: args.jobs,
                    export: &args.export,
                    profile_stem: root
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("review"),
                },
            )?;
            rendered += 1;
            emit_publish_progress(
                progress,
                ReviewPublishProgress {
                    processed: report.linked,
                    total: report.linked,
                    linked: report.linked,
                    skipped: report.skipped,
                    galleries: rendered,
                    step: "gallery".to_string(),
                    current: None,
                },
            );
        }
        report.galleries = rendered;
    }

    Ok(report)
}

fn text_response(status: &'static str, content_type: &'static str, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        content_type,
        body: body.as_bytes().to_vec(),
    }
}

fn json_error(status: &'static str, error: anyhow::Error) -> HttpResponse {
    text_response(
        status,
        "application/json; charset=utf-8",
        &json!({"error": error.to_string()}).to_string(),
    )
}

fn write_http_response(stream: &mut TcpStream, response: HttpResponse) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len()
    )
    .context("writing HTTP response headers")?;
    stream
        .write_all(&response.body)
        .context("writing HTTP response body")
}

fn publish_store_inner(
    store: &ReviewStore,
    input_root: &Path,
    output_root: &Path,
    options: &ReviewPublishOptions,
    progress: Option<ReviewPublishProgressSink<'_>>,
) -> Result<PublishReport> {
    let mut report = PublishReport {
        min_rating: options.min_rating,
        ..PublishReport::default()
    };
    let publish_root = ensure_safe_dir_all(output_root, &options.album)?;
    let mut tasks = Vec::new();
    for image in &store.images {
        if !image_passes_publish_filters(image, options) {
            report.skipped += 1;
            continue;
        }

        let publish_indexes = effective_publish_profile_indexes(image);
        if publish_indexes.is_empty() {
            report.skipped += 1;
            continue;
        }

        let raw_stem = Path::new(&image.relative_path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| anyhow!("review image has no valid stem: {}", image.relative_path))?;
        let default_profile_index = store.profiles.first().map(|profile| profile.index);
        for profile_index in publish_indexes {
            let Some(render) = image
                .profiles
                .iter()
                .find(|render| render.profile_index == profile_index)
            else {
                report.skipped += 1;
                continue;
            };
            if render.status != ReviewRenderStatus::Done {
                report.skipped += 1;
                continue;
            }
            let Some(source) = &render.output_path else {
                report.skipped += 1;
                continue;
            };
            let source = safe_existing_output_source(source, output_root)?;
            let file_name = review_publish_file_name(
                raw_stem,
                render,
                default_profile_index,
                options.output_format,
            )?;
            let destination_relative = options.album.join(file_name);
            let destination = safe_child_path(output_root, &destination_relative)?;
            let profile = store
                .profiles
                .iter()
                .find(|profile| profile.index == profile_index)
                .ok_or_else(|| anyhow!("review profile index {profile_index} is not configured"))?;
            tasks.push(ReviewPublishTask {
                source,
                destination,
                image: image.clone(),
                render: render.clone(),
                profile: profile.clone(),
                current: format!("{} / {}", image.file_name, render.profile_stem),
            });
        }
    }

    let total = tasks.len() as u64;
    emit_publish_progress(
        progress,
        ReviewPublishProgress {
            processed: 0,
            total,
            linked: 0,
            skipped: report.skipped,
            galleries: 0,
            step: "publish".to_string(),
            current: None,
        },
    );

    if total > 0 {
        let skipped = report.skipped;
        let processed = AtomicU64::new(0);
        let linked = AtomicU64::new(0);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(options.jobs)
            .build()
            .context("building review publish thread pool")?;
        pool.install(|| {
            tasks.par_iter().try_for_each(|task| {
                let step = if options.rerender_raw {
                    "rerender"
                } else {
                    "link"
                };
                publish_review_output(ReviewPublishOutput {
                    input_root,
                    source: &task.source,
                    destination: &task.destination,
                    image: &task.image,
                    render: &task.render,
                    profile: &task.profile,
                    options,
                })?;
                let linked_now = linked.fetch_add(1, Ordering::Relaxed) + 1;
                let processed_now = processed.fetch_add(1, Ordering::Relaxed) + 1;
                emit_publish_progress(
                    progress,
                    ReviewPublishProgress {
                        processed: processed_now,
                        total,
                        linked: linked_now,
                        skipped,
                        galleries: 0,
                        step: step.to_string(),
                        current: Some(task.current.clone()),
                    },
                );
                Ok::<_, anyhow::Error>(())
            })
        })?;
        report.linked = linked.load(Ordering::Relaxed);
        if report.linked > 0 {
            report.gallery_roots.push(publish_root);
        }
    }
    emit_publish_progress(
        progress,
        ReviewPublishProgress {
            processed: report.linked,
            total,
            linked: report.linked,
            skipped: report.skipped,
            galleries: report.galleries,
            step: "publish".to_string(),
            current: None,
        },
    );
    Ok(report)
}

fn emit_publish_progress(
    progress: Option<ReviewPublishProgressSink<'_>>,
    event: ReviewPublishProgress,
) {
    if let Some(progress) = progress {
        progress(event);
    }
}

fn image_passes_publish_filters(image: &ReviewImage, options: &ReviewPublishOptions) -> bool {
    if image.rating < options.min_rating {
        return false;
    }
    if !options.labels.is_empty() && !options.labels.contains(&image.label) {
        return false;
    }
    if !options.tags.is_empty()
        && !image
            .tags
            .iter()
            .map(|tag| tag.to_ascii_lowercase())
            .any(|tag| options.tags.contains(&tag))
    {
        return false;
    }
    true
}

fn publish_review_output(item: ReviewPublishOutput<'_>) -> Result<()> {
    if item.destination.exists() {
        fs::remove_file(item.destination)
            .with_context(|| format!("removing {}", item.destination.display()))?;
    }
    if item.options.rerender_raw {
        rerender_review_output(
            item.input_root,
            item.destination,
            item.image,
            item.profile,
            item.options,
        )?;
    } else if fs::hard_link(item.source, item.destination).is_err() {
        symlink_file(item.source, item.destination).with_context(|| {
            format!(
                "symlinking {} to {} after hardlink failed",
                item.source.display(),
                item.destination.display()
            )
        })?;
    }
    if item.options.write_metadata {
        write_review_metadata(item.destination, item.image, item.render)?;
    }
    Ok(())
}

fn rerender_review_output(
    input_root: &Path,
    destination: &Path,
    image: &ReviewImage,
    profile: &ReviewProfile,
    options: &ReviewPublishOptions,
) -> Result<()> {
    let raw = safe_existing_raw_source(&image.raw_path, input_root)?;
    run_apply(ApplyArgs {
        raw,
        output: destination.to_path_buf(),
        profile: profile.selector.clone(),
        hald_dir: options.hald_dir.clone(),
        profiles_root: options.profiles_root.clone(),
        hald_level: options.hald_level,
        rawtherapee: options.rawtherapee.clone(),
        convert: options.convert.clone(),
        keep_intermediate: None,
        no_grain: options.no_grain,
        color_noise_iso_threshold: options.color_noise_iso_threshold,
        lens_corrections: options.lens_corrections,
        grain: options.grain.clone(),
        grain_preset: options.grain_preset.clone(),
        grain_seed: options
            .grain_seed
            .map(|seed| review_publish_seed(seed, &image.raw_path, profile.index)),
        export: options.export.clone(),
    })
}

fn canonical_existing_dir(path: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("canonicalizing directory {}", path.display()))?;
    if !canonical.is_dir() {
        bail!("not a directory: {}", canonical.display());
    }
    Ok(canonical)
}

fn ensure_path_within(path: &Path, root: &Path) -> Result<()> {
    if path.starts_with(root) {
        return Ok(());
    }
    bail!(
        "path {} is outside of configured root {}",
        path.display(),
        root.display()
    )
}

fn safe_relative_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        bail!("expected relative path, got {}", path.display());
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("unsafe relative path component in {}", path.display());
            }
        }
    }
    Ok(normalized)
}

fn safe_child_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    Ok(root.join(safe_relative_path(relative)?))
}

fn ensure_safe_dir_all(root: &Path, relative: &Path) -> Result<PathBuf> {
    let relative = safe_relative_path(relative)?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            continue;
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    bail!(
                        "refusing to write through symlink directory {}",
                        current.display()
                    );
                }
                if !metadata.is_dir() {
                    bail!(
                        "publish path component is not a directory: {}",
                        current.display()
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)
                    .with_context(|| format!("creating {}", current.display()))?;
            }
            Err(error) => {
                return Err(error).with_context(|| format!("checking {}", current.display()));
            }
        }
    }
    Ok(current)
}

fn safe_existing_output_source(source: &Path, output_root: &Path) -> Result<PathBuf> {
    let source = fs::canonicalize(source)
        .with_context(|| format!("canonicalizing review output {}", source.display()))?;
    ensure_path_within(&source, output_root)?;
    if !source.is_file() {
        bail!("review output is not a file: {}", source.display());
    }
    Ok(source)
}

fn safe_existing_raw_source(raw: &Path, input_root: &Path) -> Result<PathBuf> {
    let raw = fs::canonicalize(raw)
        .with_context(|| format!("canonicalizing RAW source {}", raw.display()))?;
    ensure_path_within(&raw, input_root)?;
    if !raw.is_file() {
        bail!("review RAW source is not a file: {}", raw.display());
    }
    Ok(raw)
}

fn review_publish_seed(base_seed: u64, raw: &Path, profile_index: usize) -> u64 {
    let mut hasher = Sha1::new();
    hasher.update(base_seed.to_le_bytes());
    hasher.update(raw.to_string_lossy().as_bytes());
    hasher.update((profile_index as u64).to_le_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    u64::from_le_bytes(bytes)
}

#[cfg(unix)]
fn symlink_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, destination)
}

#[cfg(windows)]
fn symlink_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(source, destination)
}

fn review_publish_file_name(
    raw_stem: &str,
    render: &ReviewProfileRender,
    default_profile_index: Option<usize>,
    output_format: BatchOutputFormat,
) -> Result<String> {
    let stem = sanitize_filename::sanitize(raw_stem).into_owned();
    let stem = if stem.trim().is_empty() {
        "image".to_string()
    } else {
        stem
    };
    let suffix = if Some(render.profile_index) == default_profile_index {
        String::new()
    } else {
        format!("-{}", review_profile_folder_name(&render.profile_stem))
    };
    Ok(format!("{stem}{suffix}.{}", output_format.extension()))
}

fn review_profile_folder_name(profile_stem: &str) -> String {
    let folder = sanitize_filename::sanitize(profile_stem).into_owned();
    if folder.trim().is_empty() {
        "profile".to_string()
    } else {
        folder
    }
}

fn review_label_name(label: ReviewLabel) -> &'static str {
    match label {
        ReviewLabel::None => "none",
        ReviewLabel::Red => "red",
        ReviewLabel::Yellow => "yellow",
        ReviewLabel::Green => "green",
        ReviewLabel::Blue => "blue",
        ReviewLabel::Purple => "purple",
    }
}

fn write_review_metadata(
    path: &Path,
    image: &ReviewImage,
    render: &ReviewProfileRender,
) -> Result<()> {
    let mut command = Command::new("exiftool");
    command
        .arg("-overwrite_original")
        .arg("-P")
        .arg("-q")
        .arg("-q")
        .arg(format!("-Rating={}", image.rating))
        .arg(format!("-XMP:Rating={}", image.rating))
        .arg(format!("-Label={}", review_label_name(image.label)))
        .arg(format!("-XMP:Label={}", review_label_name(image.label)))
        .arg(format!("-XMP:PreservedFileName={}", image.file_name))
        .arg(format!("-XMP:Nickname={}", image.relative_path))
        .arg(format!(
            "-UserComment=mini-film {} review profile={} rating={} label={} notes={}",
            env!("CARGO_PKG_VERSION"),
            render.profile_stem,
            image.rating,
            review_label_name(image.label),
            image.notes
        ));
    if !image.notes.trim().is_empty() {
        command.arg(format!("-Description={}", image.notes.trim()));
        command.arg(format!("-ImageDescription={}", image.notes.trim()));
    }
    for tag in &image.tags {
        command.arg(format!("-Subject+={tag}"));
    }
    command.arg(path);
    let output = command
        .output()
        .with_context(|| format!("running exiftool for {}", path.display()))?;
    if !output.status.success() {
        bail!(
            "exiftool failed for {} with status {}\nstderr:\n{}",
            path.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn extract_embedded_preview(raw: &Path, output: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    for tag in ["PreviewImage", "JpgFromRaw", "OtherImage", "ThumbnailImage"] {
        let result = Command::new("exiftool")
            .arg("-b")
            .arg(format!("-{tag}"))
            .arg(raw)
            .output()
            .with_context(|| format!("extracting {tag} from {}", raw.display()))?;
        if !result.status.success() || !looks_like_jpeg(&result.stdout) {
            continue;
        }

        let temp = output.with_extension("jpg.tmp");
        fs::write(&temp, &result.stdout).with_context(|| format!("writing {}", temp.display()))?;
        fs::rename(&temp, output)
            .with_context(|| format!("renaming {} to {}", temp.display(), output.display()))?;
        return Ok(());
    }

    bail!("no embedded JPEG preview found in {}", raw.display())
}

fn looks_like_jpeg(bytes: &[u8]) -> bool {
    bytes.len() > 3 && bytes[0] == 0xff && bytes[1] == 0xd8 && bytes[2] == 0xff
}

fn short_path_sha1(path: &Path) -> String {
    let mut hasher = Sha1::new();
    hasher.update(path.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(index: usize, stem: &str) -> ReviewProfile {
        ReviewProfile {
            index,
            selector: stem.to_string(),
            stem: stem.to_string(),
        }
    }

    fn test_export_options() -> ExportOptions {
        ExportOptions {
            jpg_quality: 90,
            resize: None,
            long_edge: None,
            max_width: None,
            max_height: None,
            jpeg_subsampling: crate::cli::JpegSubsampling::S444,
            strip_metadata: false,
            progressive_jpeg: false,
        }
    }

    fn test_handle(input: PathBuf, output: PathBuf, profiles: Vec<ReviewProfile>) -> ReviewHandle {
        let export = test_export_options();
        ReviewHandle {
            state: Arc::new(Mutex::new(ReviewStore::new(profiles))),
            subscribers: Arc::new(Mutex::new(Vec::new())),
            state_path: output.join("mini-film-review.json"),
            input_root: input.clone(),
            output_root: output.clone(),
            hald_dir: output.join("hald"),
            profiles_root: input.clone(),
            hald_level: 16,
            rawtherapee: PathBuf::from("rawtherapee-cli"),
            output_format: BatchOutputFormat::Jpg,
            gallery: None,
            convert: PathBuf::from("convert"),
            export: export.clone(),
            jobs: 1,
            no_grain: false,
            color_noise_iso_threshold: 1600,
            lens_corrections: LensCorrections::default(),
            grain: None,
            grain_preset: None,
            grain_seed: Some(1),
            publish_defaults: ReviewPublishDefaults::new(
                "published".to_string(),
                BatchOutputFormat::Jpg,
                &export,
                ReviewGalleryDefaults {
                    template: None,
                    thumbnail_long_edge: 1024,
                    columns: 4,
                },
            ),
            publish_jobs: Arc::new(Mutex::new(Vec::new())),
            next_publish_job_id: Arc::new(Mutex::new(1)),
        }
    }

    fn test_publish_options(album: &str) -> ReviewPublishOptions {
        ReviewPublishOptions {
            album: PathBuf::from(album),
            min_rating: 2,
            labels: HashSet::new(),
            tags: HashSet::new(),
            output_format: BatchOutputFormat::Jpg,
            hald_dir: PathBuf::from("hald"),
            profiles_root: PathBuf::from("profiles"),
            hald_level: 16,
            rawtherapee: PathBuf::from("rawtherapee-cli"),
            convert: PathBuf::from("convert"),
            jobs: 2,
            export: test_export_options(),
            rerender_raw: false,
            no_grain: false,
            color_noise_iso_threshold: 1600,
            lens_corrections: LensCorrections::default(),
            grain: None,
            grain_preset: None,
            grain_seed: Some(1),
            write_metadata: false,
        }
    }

    #[test]
    fn review_state_defaults_to_first_profile_and_records_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("in");
        let output = temp.path().join("out");
        fs::create_dir_all(input.join("day")).unwrap();
        fs::create_dir_all(&output).unwrap();
        let raw = input.join("day").join("frame.NEF");
        fs::write(&raw, b"raw").unwrap();
        let rendered = output.join("day").join("Classic").join("frame.jpg");
        fs::create_dir_all(rendered.parent().unwrap()).unwrap();
        fs::write(&rendered, b"jpg").unwrap();

        let handle = test_handle(
            input,
            output,
            vec![profile(0, "Classic"), profile(1, "Fade")],
        );

        handle.record_discovered_raw(&raw).unwrap();
        handle
            .record_profile_done(&raw, 0, &rendered, Duration::from_millis(42))
            .unwrap();
        let text = handle.api_state_json().unwrap();
        assert!(text.contains("\"selected_profile_index\":0"));
        assert!(text.contains("\"publish_profile_indexes\":[0,1]"));
        assert!(text.contains("\"status\":\"done\""));
        assert!(text.contains("media/1/0"));
    }

    #[test]
    fn review_update_advances_shared_server_ui_state() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("in");
        let output = temp.path().join("out");
        fs::create_dir_all(input.join("day")).unwrap();
        fs::create_dir_all(&output).unwrap();
        let first = input.join("day").join("frame-1.NEF");
        let second = input.join("day").join("frame-2.NEF");
        fs::write(&first, b"raw").unwrap();
        fs::write(&second, b"raw").unwrap();

        let handle = test_handle(
            input,
            output,
            vec![profile(0, "Classic"), profile(1, "Fade")],
        );

        handle.record_discovered_raw(&first).unwrap();
        handle.record_discovered_raw(&second).unwrap();
        handle
            .apply_review_update(ReviewUpdateRequest {
                image_id: 1,
                rating: 1,
                label: ReviewLabel::Green,
                tags: vec!["keep".to_string()],
                notes: String::new(),
                selected_profile_index: 0,
                publish_profile_indexes: Some(vec![0, 1]),
                advance_after_update: true,
            })
            .unwrap();

        let state =
            serde_json::from_str::<serde_json::Value>(&handle.api_state_json().unwrap()).unwrap();
        assert_eq!(state["ui"]["current_image_id"], 2);
        assert_eq!(state["ui"]["min_rating"], 0);

        handle
            .apply_review_update(ReviewUpdateRequest {
                image_id: 2,
                rating: 0,
                label: ReviewLabel::None,
                tags: Vec::new(),
                notes: String::new(),
                selected_profile_index: 0,
                publish_profile_indexes: Some(vec![0, 1]),
                advance_after_update: true,
            })
            .unwrap();

        let state =
            serde_json::from_str::<serde_json::Value>(&handle.api_state_json().unwrap()).unwrap();
        assert_eq!(state["ui"]["current_image_id"], 1);
        assert_eq!(state["ui"]["min_rating"], 1);
    }

    #[test]
    fn review_state_reports_connected_client_count() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("in");
        let output = temp.path().join("out");
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output).unwrap();

        let handle = test_handle(input, output, vec![profile(0, "Classic")]);

        let state =
            serde_json::from_str::<serde_json::Value>(&handle.api_state_json().unwrap()).unwrap();
        assert_eq!(state["client_count"], 0);

        let client = handle.subscribe().unwrap();
        let state =
            serde_json::from_str::<serde_json::Value>(&handle.api_state_json().unwrap()).unwrap();
        assert_eq!(state["client_count"], 1);

        drop(client);
        handle.broadcast_state().unwrap();
        let state =
            serde_json::from_str::<serde_json::Value>(&handle.api_state_json().unwrap()).unwrap();
        assert_eq!(state["client_count"], 0);
    }

    #[test]
    fn review_route_path_accepts_reverse_proxy_prefixes() {
        assert_eq!(review_route_path("/api/state"), "/api/state");
        assert_eq!(review_route_path("/mini-film/api/state"), "/api/state");
        assert_eq!(
            review_route_path("/nested/mini-film/assets/app.js"),
            "/assets/app.js"
        );
        assert_eq!(review_route_path("/mini-film/media/1/0"), "/media/1/0");
        assert_eq!(review_route_path("/mini-film/preview/1"), "/preview/1");
        assert_eq!(review_route_path("/mini-film/review"), "/review");
        assert_eq!(review_route_path("/mini-film/"), "/");
    }

    #[test]
    fn publish_flat_album_filters_rating_label_and_tag() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("out");
        let source = output.join("day").join("Classic").join("frame.jpg");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"jpg").unwrap();

        let mut store = ReviewStore::new(vec![profile(0, "Classic")]);
        store.images.push(ReviewImage {
            id: 1,
            raw_path: PathBuf::from("/in/day/frame.NEF"),
            relative_path: "day/frame.NEF".to_string(),
            file_name: "frame.NEF".to_string(),
            selected_profile_index: 0,
            rating: 3,
            label: ReviewLabel::Red,
            tags: vec!["42".to_string()],
            notes: "keeper".to_string(),
            publish_profile_indexes: Some(vec![0]),
            preview: ReviewPreview::default(),
            profiles: vec![ReviewProfileRender {
                profile_index: 0,
                profile_stem: "Classic".to_string(),
                status: ReviewRenderStatus::Done,
                output_path: Some(source.clone()),
                error: None,
                duration_ms: Some(1),
                updated_at: now_string(),
            }],
            updated_at: now_string(),
        });

        let mut options = test_publish_options("published/final");
        options.labels = HashSet::from([ReviewLabel::Red]);
        options.tags = HashSet::from(["42".to_string()]);
        let report =
            publish_store_inner(&store, Path::new("/in"), &output, &options, None).unwrap();
        assert_eq!(report.linked, 1);
        assert_eq!(report.skipped, 0);
        assert!(output.join("published/final/frame.jpg").exists());
    }

    #[test]
    fn publish_flat_album_suffixes_non_default_profiles() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("out");
        let classic = output.join("day").join("Classic").join("frame.jpg");
        let fade = output.join("day").join("Fade").join("frame.jpg");
        fs::create_dir_all(classic.parent().unwrap()).unwrap();
        fs::create_dir_all(fade.parent().unwrap()).unwrap();
        fs::write(&classic, b"classic").unwrap();
        fs::write(&fade, b"fade").unwrap();

        let mut store = ReviewStore::new(vec![profile(0, "Classic"), profile(1, "Fade")]);
        store.images.push(ReviewImage {
            id: 1,
            raw_path: PathBuf::from("/in/day/frame.NEF"),
            relative_path: "day/frame.NEF".to_string(),
            file_name: "frame.NEF".to_string(),
            selected_profile_index: 0,
            rating: 2,
            label: ReviewLabel::None,
            tags: Vec::new(),
            notes: String::new(),
            publish_profile_indexes: Some(vec![1]),
            preview: ReviewPreview::default(),
            profiles: vec![
                ReviewProfileRender {
                    profile_index: 0,
                    profile_stem: "Classic".to_string(),
                    status: ReviewRenderStatus::Done,
                    output_path: Some(classic.clone()),
                    error: None,
                    duration_ms: Some(1),
                    updated_at: now_string(),
                },
                ReviewProfileRender {
                    profile_index: 1,
                    profile_stem: "Fade".to_string(),
                    status: ReviewRenderStatus::Done,
                    output_path: Some(fade.clone()),
                    error: None,
                    duration_ms: Some(1),
                    updated_at: now_string(),
                },
            ],
            updated_at: now_string(),
        });

        let options = test_publish_options("published");
        let report =
            publish_store_inner(&store, Path::new("/in"), &output, &options, None).unwrap();
        assert_eq!(report.linked, 1);
        assert!(!output.join("published/frame.jpg").exists());
        assert!(output.join("published/frame-Fade.jpg").exists());
    }

    #[test]
    fn publish_store_reports_realtime_progress() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("out");
        let classic = output.join("day").join("Classic").join("frame.jpg");
        let fade = output.join("day").join("Fade").join("frame.jpg");
        fs::create_dir_all(classic.parent().unwrap()).unwrap();
        fs::create_dir_all(fade.parent().unwrap()).unwrap();
        fs::write(&classic, b"classic").unwrap();
        fs::write(&fade, b"fade").unwrap();

        let mut store = ReviewStore::new(vec![profile(0, "Classic"), profile(1, "Fade")]);
        store.images.push(ReviewImage {
            id: 1,
            raw_path: PathBuf::from("/in/day/frame.NEF"),
            relative_path: "day/frame.NEF".to_string(),
            file_name: "frame.NEF".to_string(),
            selected_profile_index: 0,
            rating: 5,
            label: ReviewLabel::None,
            tags: Vec::new(),
            notes: String::new(),
            publish_profile_indexes: Some(vec![0, 1]),
            preview: ReviewPreview::default(),
            profiles: vec![
                ReviewProfileRender {
                    profile_index: 0,
                    profile_stem: "Classic".to_string(),
                    status: ReviewRenderStatus::Done,
                    output_path: Some(classic.clone()),
                    error: None,
                    duration_ms: Some(1),
                    updated_at: now_string(),
                },
                ReviewProfileRender {
                    profile_index: 1,
                    profile_stem: "Fade".to_string(),
                    status: ReviewRenderStatus::Done,
                    output_path: Some(fade.clone()),
                    error: None,
                    duration_ms: Some(1),
                    updated_at: now_string(),
                },
            ],
            updated_at: now_string(),
        });

        let events = Mutex::new(Vec::new());
        let progress = |event: ReviewPublishProgress| {
            events.lock().unwrap().push(event);
        };
        let options = test_publish_options("published");
        let report =
            publish_store_inner(&store, Path::new("/in"), &output, &options, Some(&progress))
                .unwrap();
        let events = events.lock().unwrap();
        assert_eq!(report.linked, 2);
        assert!(events.iter().any(|event| event.total == 2));
        assert!(events.iter().any(|event| event.processed == 2));
        assert!(events.iter().any(|event| event.step == "link"));
    }

    #[test]
    fn short_path_sha1_is_stable_and_short() {
        let first = short_path_sha1(Path::new("/tmp/frame.NEF"));
        assert_eq!(first, short_path_sha1(Path::new("/tmp/frame.NEF")));
        assert_ne!(first, short_path_sha1(Path::new("/tmp/other.NEF")));
        assert_eq!(first.len(), 16);
    }

    #[test]
    fn jpeg_detection_requires_marker() {
        assert!(looks_like_jpeg(&[0xff, 0xd8, 0xff, 0xee]));
        assert!(!looks_like_jpeg(b"not jpeg"));
    }
}
