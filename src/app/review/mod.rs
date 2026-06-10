use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha1::{Digest, Sha1};

use crate::app::review_assets::{review_index_html, review_script, review_styles};
use crate::{
    app::batch::{FolderGalleryOptions, render_gallery_for_folder},
    cli::{BatchOutputFormat, ExportOptions, GalleryTemplate},
};

#[derive(Clone, Debug)]
pub(crate) struct ReviewConfig {
    pub(crate) address: String,
    pub(crate) input_root: PathBuf,
    pub(crate) output_root: PathBuf,
    pub(crate) output_format: BatchOutputFormat,
    pub(crate) profiles: Vec<ReviewProfile>,
    pub(crate) gallery: Option<ReviewGalleryConfig>,
}

#[derive(Clone, Debug)]
pub(crate) struct ReviewGalleryConfig {
    pub(crate) convert: PathBuf,
    pub(crate) template: GalleryTemplate,
    pub(crate) columns: u32,
    pub(crate) thumbnail_long_edge: u32,
    pub(crate) jobs: usize,
    pub(crate) export: ExportOptions,
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
    output_format: BatchOutputFormat,
    gallery: Option<ReviewGalleryConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReviewStore {
    next_id: u64,
    profiles: Vec<ReviewProfile>,
    images: Vec<ReviewImage>,
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
    #[serde(default)]
    advance_token: Option<String>,
    #[serde(default)]
    advance_client_id: Option<String>,
    profiles: Vec<ReviewProfileRender>,
    updated_at: String,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
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
    client_id: Option<String>,
    #[serde(default)]
    advance_after_update: bool,
}

#[derive(Debug, Default, Deserialize)]
struct PublishRequest {
    #[serde(default)]
    min_rating: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PublishReport {
    pub(crate) linked: u64,
    pub(crate) skipped: u64,
    pub(crate) min_rating: u8,
    pub(crate) galleries: u64,
    gallery_roots: Vec<PathBuf>,
}

impl ReviewStore {
    fn new(profiles: Vec<ReviewProfile>) -> Self {
        Self {
            next_id: 1,
            profiles,
            images: Vec::new(),
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
            advance_token: None,
            advance_client_id: None,
            profiles: Vec::new(),
            updated_at: now_string(),
        };
        sync_image_profile_renders(&mut image, &self.profiles);
        self.images.push(image);
        let index = self.images.len() - 1;
        Ok(&mut self.images[index])
    }
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

    let handle = ReviewHandle {
        state: Arc::new(Mutex::new(store)),
        subscribers: Arc::new(Mutex::new(Vec::new())),
        state_path,
        input_root: config.input_root,
        output_root: config.output_root,
        output_format: config.output_format,
        gallery: config.gallery,
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
        let updated_at = now_string();
        if update.advance_after_update {
            let client_id = update
                .client_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .unwrap_or("anonymous")
                .to_string();
            image.advance_client_id = Some(client_id.clone());
            image.advance_token = Some(format!("{}:{}:{updated_at}", image.id, client_id));
        }
        image.updated_at = updated_at;
        save_store(&self.state_path, &store)?;
        drop(store);
        self.broadcast_state()
    }

    fn api_state_json(&self) -> Result<String> {
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
                    "advance_token": image.advance_token,
                    "advance_client_id": image.advance_client_id,
                    "profiles": profiles,
                    "updated_at": image.updated_at,
                })
            })
            .collect::<Vec<_>>();

        serde_json::to_string(&json!({
            "version": env!("CARGO_PKG_VERSION"),
            "profiles": store.profiles,
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

    pub(crate) fn publish(&self, min_rating: u8) -> Result<PublishReport> {
        let store = self.lock_store()?.clone();
        let mut report = publish_store(
            &store,
            &self.output_root,
            self.output_format,
            min_rating.min(5),
        )?;
        if let Some(gallery) = &self.gallery {
            let mut rendered = 0u64;
            for root in &report.gallery_roots {
                render_gallery_for_folder(
                    root,
                    &FolderGalleryOptions {
                        convert: &gallery.convert,
                        template: gallery.template,
                        columns: gallery.columns,
                        thumbnail_long_edge: gallery.thumbnail_long_edge,
                        jobs: gallery.jobs,
                        export: &gallery.export,
                        profile_stem: root
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("review"),
                    },
                )?;
                rendered += 1;
            }
            report.galleries = rendered;
        }
        Ok(report)
    }

    fn subscribe(&self) -> Result<Receiver<String>> {
        let (sender, receiver) = mpsc::channel();
        let state = self.api_state_json()?;
        sender
            .send(state)
            .map_err(|_| anyhow!("review event subscriber disconnected"))?;
        self.subscribers
            .lock()
            .map_err(|_| anyhow!("review subscribers lock poisoned"))?
            .push(sender);
        Ok(receiver)
    }

    fn broadcast_state(&self) -> Result<()> {
        let state = self.api_state_json()?;
        let mut subscribers = self
            .subscribers
            .lock()
            .map_err(|_| anyhow!("review subscribers lock poisoned"))?;
        subscribers.retain(|subscriber| subscriber.send(state.clone()).is_ok());
        Ok(())
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
        ("POST", "/api/review") => match serde_json::from_slice::<ReviewUpdateRequest>(&request.body)
            .context("parsing review update")
            .and_then(|update| handle.apply_review_update(update))
            .and_then(|()| handle.api_state_json())
        {
            Ok(body) => text_response("200 OK", "application/json; charset=utf-8", &body),
            Err(error) => json_error("400 Bad Request", error),
        },
        ("POST", "/api/publish") => match parse_publish_request(&request.body)
            .and_then(|request| handle.publish(request.min_rating))
        {
            Ok(report) => text_response(
                "200 OK",
                "application/json; charset=utf-8",
                &json!({"linked": report.linked, "skipped": report.skipped, "min_rating": report.min_rating, "galleries": report.galleries}).to_string(),
            ),
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

fn publish_store(
    store: &ReviewStore,
    output_root: &Path,
    output_format: BatchOutputFormat,
    min_rating: u8,
) -> Result<PublishReport> {
    publish_store_inner(store, output_root, output_format, min_rating, true)
}

fn publish_store_inner(
    store: &ReviewStore,
    output_root: &Path,
    output_format: BatchOutputFormat,
    min_rating: u8,
    write_metadata: bool,
) -> Result<PublishReport> {
    let mut report = PublishReport {
        min_rating,
        ..PublishReport::default()
    };
    let publish_root = output_root.join("reviewed");
    for image in &store.images {
        let publish_indexes = effective_publish_profile_indexes(image);
        if publish_indexes.is_empty() {
            report.skipped += 1;
            continue;
        }

        let relative = Path::new(&image.relative_path);
        let parent = relative.parent().unwrap_or_else(|| Path::new(""));
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
            if !source.is_file() {
                report.skipped += 1;
                continue;
            }
            if write_metadata {
                write_review_metadata(source, image, render)?;
            }
            let profile_folder = review_profile_folder_name(&render.profile_stem);
            let profile_parent = Path::new(&profile_folder).join(parent);
            let file_name = review_output_file_name(source, output_format)?;

            let rating_root = publish_root.join("ratings").join(image.rating.to_string());
            report.gallery_roots.push(rating_root.clone());
            link_review_output(
                source,
                &rating_root.join(&profile_parent).join(&file_name),
                &mut report,
            )?;

            let selected_root = publish_root.join("selected");
            report.gallery_roots.push(selected_root.clone());
            link_review_output(
                source,
                &selected_root.join(&profile_parent).join(&file_name),
                &mut report,
            )?;

            if image.rating >= min_rating {
                let final_root = publish_root.join("final");
                report.gallery_roots.push(final_root.clone());
                link_review_output(
                    source,
                    &final_root.join(&profile_parent).join(&file_name),
                    &mut report,
                )?;
            }

            if image.label != ReviewLabel::None {
                let label_root = publish_root
                    .join("labels")
                    .join(review_label_name(image.label))
                    .join(format!("rating-{}", image.rating));
                report.gallery_roots.push(label_root.clone());
                link_review_output(
                    source,
                    &label_root.join(&profile_parent).join(&file_name),
                    &mut report,
                )?;
            }

            for tag in &image.tags {
                let tag = sanitize_filename::sanitize(tag).into_owned();
                if tag.is_empty() {
                    continue;
                }
                let tag_root = publish_root
                    .join("tags")
                    .join(tag)
                    .join(format!("rating-{}", image.rating));
                report.gallery_roots.push(tag_root.clone());
                link_review_output(
                    source,
                    &tag_root.join(&profile_parent).join(&file_name),
                    &mut report,
                )?;
            }
        }
    }
    dedupe_paths(&mut report.gallery_roots);
    Ok(report)
}

fn dedupe_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
}

fn review_output_file_name(source: &Path, output_format: BatchOutputFormat) -> Result<String> {
    let stem = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| anyhow!("output has no valid file stem: {}", source.display()))?;
    Ok(format!("{stem}.{}", output_format.extension()))
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

fn link_review_output(source: &Path, destination: &Path, report: &mut PublishReport) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    if destination.exists() {
        fs::remove_file(destination)
            .with_context(|| format!("removing {}", destination.display()))?;
    }
    fs::hard_link(source, destination).with_context(|| {
        format!(
            "hardlinking {} to {}",
            source.display(),
            destination.display()
        )
    })?;
    report.linked += 1;
    Ok(())
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

        let handle = ReviewHandle {
            state: Arc::new(Mutex::new(ReviewStore::new(vec![
                profile(0, "Classic"),
                profile(1, "Fade"),
            ]))),
            subscribers: Arc::new(Mutex::new(Vec::new())),
            state_path: output.join("mini-film-review.json"),
            input_root: input,
            output_root: output,
            output_format: BatchOutputFormat::Jpg,
            gallery: None,
        };

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
    fn review_update_can_mark_collaborative_auto_advance() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("in");
        let output = temp.path().join("out");
        fs::create_dir_all(input.join("day")).unwrap();
        fs::create_dir_all(&output).unwrap();
        let raw = input.join("day").join("frame.NEF");
        fs::write(&raw, b"raw").unwrap();

        let handle = ReviewHandle {
            state: Arc::new(Mutex::new(ReviewStore::new(vec![
                profile(0, "Classic"),
                profile(1, "Fade"),
            ]))),
            subscribers: Arc::new(Mutex::new(Vec::new())),
            state_path: output.join("mini-film-review.json"),
            input_root: input,
            output_root: output,
            output_format: BatchOutputFormat::Jpg,
            gallery: None,
        };

        handle.record_discovered_raw(&raw).unwrap();
        handle
            .apply_review_update(ReviewUpdateRequest {
                image_id: 1,
                rating: 1,
                label: ReviewLabel::Green,
                tags: vec!["keep".to_string()],
                notes: String::new(),
                selected_profile_index: 0,
                publish_profile_indexes: Some(vec![0, 1]),
                client_id: Some("client-a".to_string()),
                advance_after_update: true,
            })
            .unwrap();

        let state =
            serde_json::from_str::<serde_json::Value>(&handle.api_state_json().unwrap()).unwrap();
        let image = &state["images"][0];
        assert_eq!(image["advance_client_id"], "client-a");
        assert!(
            image["advance_token"]
                .as_str()
                .unwrap()
                .starts_with("1:client-a:")
        );
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
    fn publish_hardlinks_selected_label_and_tags() {
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
            advance_token: None,
            advance_client_id: None,
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

        let report =
            publish_store_inner(&store, &output, BatchOutputFormat::Jpg, 2, false).unwrap();
        assert_eq!(report.linked, 5);
        assert!(
            output
                .join("reviewed/selected/Classic/day/frame.jpg")
                .exists()
        );
        assert!(output.join("reviewed/final/Classic/day/frame.jpg").exists());
        assert!(
            output
                .join("reviewed/ratings/3/Classic/day/frame.jpg")
                .exists()
        );
        assert!(
            output
                .join("reviewed/labels/red/rating-3/Classic/day/frame.jpg")
                .exists()
        );
        assert!(
            output
                .join("reviewed/tags/42/rating-3/Classic/day/frame.jpg")
                .exists()
        );
    }

    #[test]
    fn publish_uses_selected_publish_profiles_not_preview_profile() {
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
            advance_token: None,
            advance_client_id: None,
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

        let report =
            publish_store_inner(&store, &output, BatchOutputFormat::Jpg, 2, false).unwrap();
        assert_eq!(report.linked, 3);
        assert!(
            !output
                .join("reviewed/selected/Classic/day/frame.jpg")
                .exists()
        );
        assert!(output.join("reviewed/selected/Fade/day/frame.jpg").exists());
        assert!(output.join("reviewed/final/Fade/day/frame.jpg").exists());
        assert!(
            output
                .join("reviewed/ratings/2/Fade/day/frame.jpg")
                .exists()
        );
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
